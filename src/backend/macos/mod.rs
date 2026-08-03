mod automation;
mod library;
mod parser;
mod script;

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;

use crate::{
    backend::{
        BackendCommand, BackendError, BackendUpdate, Capabilities, Capability, MusicBackend,
    },
    domain::{
        BackendAvailability, BackendSnapshot, CollectionLoadState, PlaybackSnapshot,
        PlaybackStatus, PlaylistId, RepeatMode, Track, TrackId,
    },
};

use self::{
    automation::{AutomationError, AutomationRunner, SystemAutomationRunner},
    library::{
        derive_library, normalize_identifier, persistent_playlist_selector,
        persistent_track_selector, raw_playlist_to_domain, raw_track_to_domain,
    },
    parser::{RawMusicState, RawScriptError, RawTrack, parse_output},
    script::{ScriptRequest, TrackSelector},
};

const COLLECTION_BATCH_SIZE: usize = 200;

#[derive(Clone, Debug, Eq, PartialEq)]
enum LoadPhase {
    DiscoverPlaylists,
    Library { next: usize, total: Option<usize> },
    Ready,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PendingPlaylistLoad {
    id: PlaylistId,
    next: usize,
    total: Option<usize>,
}

/// Music.app backend driven only by the locally installed, public scripting dictionary.
///
/// Playback polling and collection loading are serialized on the backend worker. Collection
/// queries use bounded property arrays; the render loop never performs Apple Events.
pub struct MacOsMusicBackend {
    runner: Arc<dyn AutomationRunner>,
    installed: bool,
    snapshot: BackendSnapshot,
    track_key: Option<String>,
    phase: LoadPhase,
    pending_playlist: Option<PendingPlaylistLoad>,
}

impl MacOsMusicBackend {
    #[must_use]
    pub fn new() -> Self {
        let runner: Arc<dyn AutomationRunner> = Arc::new(SystemAutomationRunner);
        let installed = runner.is_installed();
        Self::with_runner(runner, installed)
    }

    #[must_use]
    pub fn search_local_library(&self, query: &str, limit: usize) -> Vec<TrackId> {
        library::search_track_ids(&self.snapshot.library, query, limit)
    }

    fn with_runner(runner: Arc<dyn AutomationRunner>, installed: bool) -> Self {
        let mut snapshot = BackendSnapshot::default();
        if !installed {
            snapshot.availability = BackendAvailability::Unavailable;
            snapshot.library_status =
                CollectionLoadState::Error("Music.app is not installed on this Mac".to_owned());
            snapshot.playlist_status = snapshot.library_status.clone();
        }
        Self {
            runner,
            installed,
            snapshot,
            track_key: None,
            phase: LoadPhase::DiscoverPlaylists,
            pending_playlist: None,
        }
    }

    async fn query(&self, request: ScriptRequest) -> Result<RawMusicState, BackendFailure> {
        if !self.installed {
            return Err(BackendFailure::Unavailable);
        }

        tracing::trace!(?request, "Music.app query started");
        let runner = Arc::clone(&self.runner);
        let request_for_runner = request.clone();
        let output = tokio::task::spawn_blocking(move || runner.run(request_for_runner))
            .await
            .map_err(|error| BackendFailure::Error(format!("automation task failed: {error}")))?
            .map_err(classify_automation_error)?;
        let raw = parse_output(&output).map_err(|error| {
            BackendFailure::Error(format!("invalid Music.app response: {error}"))
        })?;
        if let Some(error) = raw.error.as_ref() {
            return Err(classify_script_error(error));
        }
        tracing::trace!(
            ?request,
            running = raw.running,
            state = raw.state.as_deref().unwrap_or("missing"),
            position = raw.position.unwrap_or_default(),
            "Music.app response received"
        );
        Ok(raw)
    }

    fn apply_raw_playback(&mut self, raw: &RawMusicState) {
        let incoming_identity = track_identity(raw.track.as_ref());
        let identity_missing_transiently = raw.running
            && incoming_identity.is_none()
            && self.track_key.is_some()
            && !matches!(raw.state.as_deref(), Some("stopped"));
        let identity = if identity_missing_transiently {
            self.track_key.clone()
        } else {
            incoming_identity
        };
        let track_changed = identity != self.track_key;
        if track_changed {
            tracing::debug!(
                identity = %identity_log_token(identity.as_ref()),
                "Music.app track changed"
            );
        }
        let cached_track = if track_changed {
            None
        } else {
            self.snapshot.playback.current_track.clone()
        };
        let previous_status = self.snapshot.playback.status;
        let (availability, playback) = playback_from_raw(raw, cached_track);
        self.track_key = identity;
        self.snapshot.availability = availability;
        self.snapshot.playback = playback;
        if self.snapshot.playback.status != previous_status {
            tracing::debug!(
                previous = ?previous_status,
                current = ?self.snapshot.playback.status,
                "Music.app playback state changed"
            );
        }
    }

    fn apply_failure(&mut self, failure: BackendFailure) {
        tracing::debug!(?failure, "Music.app synchronization failed");
        self.track_key = None;
        self.snapshot.availability = availability_for_failure(failure);
        self.snapshot.playback = PlaybackSnapshot::default();
    }

    fn playback_update(&self) -> BackendUpdate {
        BackendUpdate::Playback {
            availability: self.snapshot.availability.clone(),
            playback: self.snapshot.playback.clone(),
        }
    }

    async fn poll(&mut self) -> BackendUpdate {
        match self.query(ScriptRequest::Poll).await {
            Ok(raw) => self.apply_raw_playback(&raw),
            Err(failure) => self.apply_failure(failure),
        }
        self.playback_update()
    }

    async fn discover_playlists(&mut self) -> BackendUpdate {
        let raw = match self.query(ScriptRequest::DiscoverPlaylists).await {
            Ok(raw) => raw,
            Err(failure) => {
                self.apply_failure(failure);
                return self.playback_update();
            }
        };
        self.apply_raw_playback(&raw);
        let Some(raw_playlists) = raw.playlists else {
            return self.playback_update();
        };
        let playlists = raw_playlists
            .into_iter()
            .map(raw_playlist_to_domain)
            .collect::<Vec<_>>();
        self.snapshot.playlists = playlists.clone();
        self.snapshot.playlist_status = CollectionLoadState::Loaded {
            total: playlists.len(),
        };
        self.snapshot.library.clear();
        self.snapshot.library_status = CollectionLoadState::Loading {
            loaded: 0,
            total: 0,
        };
        self.phase = LoadPhase::Library {
            next: 0,
            total: None,
        };
        BackendUpdate::Playlists {
            availability: self.snapshot.availability.clone(),
            playback: self.snapshot.playback.clone(),
            playlists,
        }
    }

    async fn load_library_batch(&mut self, start: usize, total: Option<usize>) -> BackendUpdate {
        let raw = match self
            .query(ScriptRequest::LibraryBatch {
                start,
                limit: COLLECTION_BATCH_SIZE,
                total,
            })
            .await
        {
            Ok(raw) => raw,
            Err(failure) => {
                self.apply_failure(failure);
                return self.playback_update();
            }
        };
        self.apply_raw_playback(&raw);
        let Some(batch) = raw.library_batch else {
            return self.playback_update();
        };
        let tracks = batch
            .tracks
            .into_iter()
            .map(raw_track_to_domain)
            .collect::<Vec<_>>();
        if batch.start == 0 {
            self.snapshot.library.clear();
        }
        self.snapshot.library.extend(tracks.iter().cloned());
        let loaded = batch.start.saturating_add(tracks.len()).min(batch.total);
        let complete = loaded >= batch.total;
        let derived = if complete {
            derive_library(&self.snapshot.library)
        } else {
            Default::default()
        };
        if complete {
            self.snapshot.artists = derived.artists.clone();
            self.snapshot.albums = derived.albums.clone();
            self.snapshot.recently_added = derived.recently_added.clone();
            self.snapshot.library_status = CollectionLoadState::Loaded { total: batch.total };
            self.phase = LoadPhase::Ready;
        } else {
            self.snapshot.library_status = CollectionLoadState::Loading {
                loaded,
                total: batch.total,
            };
            self.phase = LoadPhase::Library {
                next: loaded,
                total: Some(batch.total),
            };
        }
        BackendUpdate::LibraryBatch {
            availability: self.snapshot.availability.clone(),
            playback: self.snapshot.playback.clone(),
            tracks,
            loaded,
            total: batch.total,
            complete,
            artists: derived.artists,
            albums: derived.albums,
            recently_added: derived.recently_added,
        }
    }

    async fn load_playlist_batch(&mut self, load: PendingPlaylistLoad) -> BackendUpdate {
        let Some(persistent_id) = persistent_playlist_selector(&load.id).map(str::to_owned) else {
            self.pending_playlist = None;
            return self.playback_update();
        };
        let raw = match self
            .query(ScriptRequest::PlaylistBatch {
                playlist_persistent_id: persistent_id,
                start: load.next,
                limit: COLLECTION_BATCH_SIZE,
                total: load.total,
            })
            .await
        {
            Ok(raw) => raw,
            Err(failure) => {
                self.apply_failure(failure);
                self.pending_playlist = None;
                return self.playback_update();
            }
        };
        self.apply_raw_playback(&raw);
        let Some(batch) = raw.playlist_batch else {
            return self.playback_update();
        };
        let tracks = batch
            .tracks
            .into_iter()
            .map(raw_track_to_domain)
            .collect::<Vec<_>>();
        let loaded = batch.start.saturating_add(tracks.len()).min(batch.total);
        let complete = loaded >= batch.total;
        if let Some(playlist) = self
            .snapshot
            .playlists
            .iter_mut()
            .find(|playlist| playlist.id == load.id)
        {
            if batch.start == 0 {
                playlist.tracks.clear();
            }
            playlist.tracks.extend(tracks.iter().cloned());
            playlist.track_count = batch.total;
            playlist.tracks_loaded = complete;
        }
        self.pending_playlist = if complete {
            None
        } else {
            Some(PendingPlaylistLoad {
                id: load.id.clone(),
                next: loaded,
                total: Some(batch.total),
            })
        };
        BackendUpdate::PlaylistBatch {
            availability: self.snapshot.availability.clone(),
            playback: self.snapshot.playback.clone(),
            playlist_id: load.id,
            tracks,
            loaded,
            total: batch.total,
            complete,
        }
    }

    async fn run_playback_command(&mut self, request: ScriptRequest) -> BackendUpdate {
        match self.query(request).await {
            Ok(raw) => self.apply_raw_playback(&raw),
            Err(failure) => self.apply_failure(failure),
        }
        self.playback_update()
    }
}

impl Default for MacOsMusicBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MusicBackend for MacOsMusicBackend {
    fn name(&self) -> &'static str {
        "Music.app"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::macos()
    }

    fn poll_interval(&self) -> Duration {
        Duration::from_millis(500)
    }

    async fn snapshot(&mut self) -> Result<BackendSnapshot, BackendError> {
        match self.query(ScriptRequest::FullState).await {
            Ok(raw) => {
                self.apply_raw_playback(&raw);
                self.phase = LoadPhase::DiscoverPlaylists;
                self.snapshot.playlist_status = CollectionLoadState::Loading {
                    loaded: 0,
                    total: 0,
                };
                self.snapshot.library_status = CollectionLoadState::NotStarted;
            }
            Err(failure) => self.apply_failure(failure),
        }
        Ok(self.snapshot.clone())
    }

    async fn execute(&mut self, command: BackendCommand) -> Result<BackendUpdate, BackendError> {
        let request = match command {
            BackendCommand::OpenPlayer => ScriptRequest::OpenPlayer,
            BackendCommand::Play => ScriptRequest::Play,
            BackendCommand::Pause => ScriptRequest::Pause,
            BackendCommand::PlayPause => ScriptRequest::PlayPause,
            BackendCommand::PlayTrack(track_id) => {
                let Some((property, value)) = persistent_track_selector(&track_id) else {
                    return Err(BackendError::TrackNotFound(track_id));
                };
                let selector = if property == "persistentID" {
                    TrackSelector::PersistentId(value.to_owned())
                } else {
                    TrackSelector::DatabaseId(value.to_owned())
                };
                ScriptRequest::PlayTrack(selector)
            }
            BackendCommand::PlayPlaylistTrack {
                playlist_id,
                track_id,
            } => {
                let Some(playlist_persistent_id) = persistent_playlist_selector(&playlist_id)
                else {
                    return Err(BackendError::PlaylistNotFound(playlist_id));
                };
                let Some((property, value)) = persistent_track_selector(&track_id) else {
                    return Err(BackendError::TrackNotFound(track_id));
                };
                let track = if property == "persistentID" {
                    TrackSelector::PersistentId(value.to_owned())
                } else {
                    TrackSelector::DatabaseId(value.to_owned())
                };
                ScriptRequest::PlayPlaylistTrack {
                    playlist_persistent_id: playlist_persistent_id.to_owned(),
                    track,
                }
            }
            BackendCommand::PlayPlaylist(playlist_id) => {
                let Some(persistent_id) = persistent_playlist_selector(&playlist_id) else {
                    return Err(BackendError::PlaylistNotFound(playlist_id));
                };
                ScriptRequest::PlayPlaylist(persistent_id.to_owned())
            }
            BackendCommand::LoadPlaylist(playlist_id) => {
                if self
                    .snapshot
                    .playlists
                    .iter()
                    .find(|playlist| playlist.id == playlist_id)
                    .is_some_and(|playlist| playlist.tracks_loaded)
                {
                    return Ok(self.playback_update());
                }
                let load = PendingPlaylistLoad {
                    id: playlist_id,
                    next: 0,
                    total: None,
                };
                self.pending_playlist = Some(load.clone());
                return Ok(self.load_playlist_batch(load).await);
            }
            BackendCommand::Next => ScriptRequest::Next,
            BackendCommand::Previous => ScriptRequest::Previous,
            BackendCommand::SeekBy(seconds) => ScriptRequest::SeekBy(seconds),
            BackendCommand::SetVolume(volume) => ScriptRequest::SetVolume(volume),
            BackendCommand::ToggleMute => {
                return Err(BackendError::Unsupported(Capability::Mute));
            }
            BackendCommand::ToggleShuffle => ScriptRequest::ToggleShuffle,
            BackendCommand::CycleRepeat => ScriptRequest::CycleRepeat,
            BackendCommand::ToggleFavoriteCurrent => {
                return Err(BackendError::Unsupported(Capability::Favorite));
            }
            BackendCommand::Enqueue(_) => {
                return Err(BackendError::Unsupported(Capability::QueueWrite));
            }
            BackendCommand::RemoveQueueItem(_) | BackendCommand::MoveQueueItem { .. } => {
                return Err(BackendError::Unsupported(Capability::QueueReorder));
            }
        };
        Ok(self.run_playback_command(request).await)
    }

    async fn tick(&mut self, _elapsed: Duration) -> Result<Option<BackendUpdate>, BackendError> {
        let update = if let Some(load) = self.pending_playlist.clone() {
            self.load_playlist_batch(load).await
        } else {
            match self.phase.clone() {
                LoadPhase::DiscoverPlaylists => self.discover_playlists().await,
                LoadPhase::Library { next, total } => self.load_library_batch(next, total).await,
                LoadPhase::Ready => self.poll().await,
            }
        };
        Ok(Some(update))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BackendFailure {
    NotRunning,
    Unavailable,
    PermissionDenied,
    Error(String),
}

fn classify_script_error(error: &RawScriptError) -> BackendFailure {
    if error.number == Some(-1743) || contains_permission_error(&error.message) {
        BackendFailure::PermissionDenied
    } else if error.number == Some(-600) {
        BackendFailure::NotRunning
    } else {
        BackendFailure::Error(error.message.clone())
    }
}

#[cfg(target_os = "macos")]
fn classify_automation_error(error: AutomationError) -> BackendFailure {
    match error {
        AutomationError::Failed { stderr, .. } if contains_permission_error(&stderr) => {
            BackendFailure::PermissionDenied
        }
        AutomationError::Failed { stderr, .. } if stderr.contains("-600") => {
            BackendFailure::NotRunning
        }
        other => BackendFailure::Error(other.to_string()),
    }
}

#[cfg(not(target_os = "macos"))]
fn classify_automation_error(error: AutomationError) -> BackendFailure {
    match error {
        AutomationError::UnsupportedPlatform => BackendFailure::Unavailable,
    }
}

fn contains_permission_error(message: &str) -> bool {
    let lowercase = message.to_lowercase();
    lowercase.contains("-1743")
        || lowercase.contains("not authorized")
        || lowercase.contains("not permitted")
}

fn availability_for_failure(failure: BackendFailure) -> BackendAvailability {
    match failure {
        BackendFailure::NotRunning => BackendAvailability::NotRunning,
        BackendFailure::Unavailable => BackendAvailability::Unavailable,
        BackendFailure::PermissionDenied => BackendAvailability::PermissionDenied,
        BackendFailure::Error(message) => BackendAvailability::Error(message),
    }
}

fn playback_from_raw(
    raw: &RawMusicState,
    cached_track: Option<Track>,
) -> (BackendAvailability, PlaybackSnapshot) {
    if !raw.running {
        return (BackendAvailability::NotRunning, PlaybackSnapshot::default());
    }
    let current_track = cached_track.or_else(|| raw.track.clone().map(raw_track_to_domain));
    let normalized_state = raw
        .state
        .as_deref()
        .map(|state| state.trim().to_lowercase());
    let (status, availability) = match normalized_state.as_deref() {
        Some("playing" | "fast forwarding" | "rewinding") => {
            (PlaybackStatus::Playing, BackendAvailability::Available)
        }
        Some("paused") => (PlaybackStatus::Paused, BackendAvailability::Available),
        Some("stopped") => (PlaybackStatus::Stopped, BackendAvailability::Available),
        Some(state) => (
            PlaybackStatus::Stopped,
            BackendAvailability::Error(format!("unknown Music.app player state '{state}'")),
        ),
        None => (
            PlaybackStatus::Stopped,
            BackendAvailability::Error("Music.app response omitted player state".to_owned()),
        ),
    };
    let volume = raw
        .volume
        .filter(|value| value.is_finite())
        .unwrap_or_default()
        .round()
        .clamp(0.0, 100.0) as u8;
    let repeat = match raw.repeat_mode.as_deref() {
        Some("one") => RepeatMode::One,
        Some("all") => RepeatMode::All,
        _ => RepeatMode::Off,
    };
    (
        availability,
        PlaybackSnapshot {
            status,
            current_entry_id: None,
            current_track,
            position: duration_from_seconds(raw.position.unwrap_or_default()),
            volume,
            muted: raw.muted.unwrap_or(false),
            shuffle: raw.shuffle.unwrap_or(false),
            repeat,
        },
    )
}

fn track_identity(raw: Option<&RawTrack>) -> Option<String> {
    let raw = raw?;
    if let Some(id) = raw.persistent_id.as_deref().and_then(normalize_identifier) {
        return Some(format!("persistent:{id}"));
    }
    if let Some(id) = raw.database_id.as_deref().and_then(normalize_identifier) {
        return Some(format!("database:{id}"));
    }
    let name = raw.name.as_deref().map_or("", str::trim);
    let artist = raw.artist.as_deref().map_or("", str::trim);
    let album = raw.album.as_deref().map_or("", str::trim);
    let duration_millis = raw
        .duration
        .filter(|duration| duration.is_finite() && *duration >= 0.0)
        .map_or(0, |duration| (duration * 1_000.0).round() as u64);
    if name.is_empty() && artist.is_empty() && album.is_empty() && duration_millis == 0 {
        return None;
    }
    Some(format!(
        "metadata:{name}\u{1f}{artist}\u{1f}{album}\u{1f}{duration_millis}"
    ))
}

fn duration_from_seconds(seconds: f64) -> Duration {
    if seconds.is_finite() && seconds > 0.0 {
        Duration::from_secs_f64(seconds)
    } else {
        Duration::ZERO
    }
}

fn identity_log_token(identity: Option<&String>) -> String {
    let Some(identity) = identity else {
        return "none".to_owned();
    };
    let mut hasher = DefaultHasher::new();
    identity.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, sync::Mutex};

    use super::*;

    struct NeverRunner;

    impl AutomationRunner for NeverRunner {
        fn is_installed(&self) -> bool {
            false
        }

        fn run(&self, _request: ScriptRequest) -> Result<String, AutomationError> {
            panic!("an unavailable backend must not invoke automation")
        }
    }

    struct SequenceRunner {
        responses: Mutex<VecDeque<(ScriptRequest, String)>>,
    }

    impl SequenceRunner {
        fn new(responses: impl IntoIterator<Item = (ScriptRequest, &'static str)>) -> Self {
            Self {
                responses: Mutex::new(
                    responses
                        .into_iter()
                        .map(|(request, response)| (request, response.to_owned()))
                        .collect(),
                ),
            }
        }
    }

    impl AutomationRunner for SequenceRunner {
        fn is_installed(&self) -> bool {
            true
        }

        fn run(&self, request: ScriptRequest) -> Result<String, AutomationError> {
            let (expected, response) = self
                .responses
                .lock()
                .expect("response lock")
                .pop_front()
                .expect("script response");
            assert_eq!(request, expected);
            Ok(response)
        }
    }

    #[tokio::test]
    async fn unavailable_backend_is_explicit_and_never_invokes_automation() {
        let runner: Arc<dyn AutomationRunner> = Arc::new(NeverRunner);
        let mut backend = MacOsMusicBackend::with_runner(runner, false);
        let snapshot = backend.snapshot().await.expect("snapshot");
        assert_eq!(snapshot.availability, BackendAvailability::Unavailable);
    }

    #[tokio::test]
    async fn playlist_and_library_discovery_are_progressive() {
        let runner: Arc<dyn AutomationRunner> = Arc::new(SequenceRunner::new([
            (
                ScriptRequest::FullState,
                r#"{"running":true,"state":"paused","position":1,"volume":50}"#,
            ),
            (
                ScriptRequest::DiscoverPlaylists,
                r#"{"running":true,"state":"paused","position":1,"volume":50,"playlists":[{"persistentId":"P1","name":"Real","kind":"userPlaylist","smart":false}]}"#,
            ),
            (
                ScriptRequest::LibraryBatch {
                    start: 0,
                    limit: COLLECTION_BATCH_SIZE,
                    total: None,
                },
                r#"{"running":true,"state":"playing","position":2,"volume":50,"libraryBatch":{"start":0,"total":1,"tracks":[{"persistentId":"T1","name":"Song","artist":"Artist","album":"Album","duration":60}]}}"#,
            ),
        ]));
        let mut backend = MacOsMusicBackend::with_runner(runner, true);
        backend.snapshot().await.expect("initial state");

        let playlists = backend
            .tick(Duration::ZERO)
            .await
            .expect("tick")
            .expect("update");
        assert!(matches!(playlists, BackendUpdate::Playlists { .. }));
        let library = backend
            .tick(Duration::ZERO)
            .await
            .expect("tick")
            .expect("update");
        let BackendUpdate::LibraryBatch {
            loaded,
            total,
            complete,
            playback,
            ..
        } = library
        else {
            panic!("library batch")
        };
        assert_eq!((loaded, total, complete), (1, 1, true));
        assert_eq!(playback.status, PlaybackStatus::Playing);
        assert_eq!(backend.snapshot.playlists[0].name, "Real");
        assert_eq!(backend.snapshot.library[0].title, "Song");
        assert_eq!(backend.snapshot.artists.len(), 1);
    }

    #[tokio::test]
    async fn selected_track_uses_identifier_based_music_app_playback() {
        let runner: Arc<dyn AutomationRunner> = Arc::new(SequenceRunner::new([(
            ScriptRequest::PlayTrack(TrackSelector::PersistentId("ABC".to_owned())),
            r#"{"running":true,"state":"playing","position":0,"volume":50}"#,
        )]));
        let mut backend = MacOsMusicBackend::with_runner(runner, true);
        let update = backend
            .execute(BackendCommand::PlayTrack(crate::domain::TrackId::new(
                "musicapp:persistent:ABC",
            )))
            .await
            .expect("play selected track");
        assert!(matches!(update, BackendUpdate::Playback { .. }));
        assert_eq!(backend.snapshot.playback.status, PlaybackStatus::Playing);
    }

    #[test]
    fn maps_external_playback_state_without_a_local_boolean() {
        let raw = parse_output(
            r#"{"running":true,"state":"playing","position":12.5,"volume":72,"track":{"persistentId":"ABC","name":"Track","artist":"Artist","album":"Album","duration":180}}"#,
        )
        .expect("raw state");
        let (_, playback) = playback_from_raw(&raw, None);
        assert_eq!(playback.status, PlaybackStatus::Playing);
        assert_eq!(
            playback.current_track.expect("track").id.as_str(),
            "musicapp:persistent:ABC"
        );
    }

    #[tokio::test]
    #[ignore = "requires a running local Music.app and Automation consent"]
    async fn live_discovers_playlists_and_one_bounded_library_batch() {
        let mut backend = MacOsMusicBackend::new();
        let initial = backend.snapshot().await.expect("initial Music.app state");
        assert_eq!(initial.availability, BackendAvailability::Available);

        let playlist_update = backend
            .tick(Duration::ZERO)
            .await
            .expect("playlist query")
            .expect("playlist update");
        let BackendUpdate::Playlists { playlists, .. } = playlist_update else {
            panic!(
                "playlist discovery failed with availability {:?}",
                backend.snapshot.availability
            )
        };
        assert!(!playlists.is_empty(), "Music.app should expose playlists");

        let library_update = backend
            .tick(Duration::ZERO)
            .await
            .expect("library query")
            .expect("library update");
        let BackendUpdate::LibraryBatch {
            tracks,
            loaded,
            total,
            ..
        } = library_update
        else {
            panic!(
                "library batch failed with availability {:?}",
                backend.snapshot.availability
            )
        };
        assert!(total >= loaded);
        assert_eq!(loaded, tracks.len());
        assert!(tracks.len() <= COLLECTION_BATCH_SIZE);
    }

    #[tokio::test]
    #[ignore = "requires a running local Music.app and Automation consent"]
    async fn live_loads_tracks_from_a_real_playlist() {
        let mut backend = MacOsMusicBackend::new();
        backend.snapshot().await.expect("initial Music.app state");
        let update = backend
            .tick(Duration::ZERO)
            .await
            .expect("playlist discovery")
            .expect("playlist update");
        let BackendUpdate::Playlists { playlists, .. } = update else {
            panic!("playlist discovery failed")
        };
        let candidates = playlists
            .into_iter()
            .filter(|playlist| {
                !matches!(
                    playlist.kind,
                    crate::domain::PlaylistKind::Folder
                        | crate::domain::PlaylistKind::Library
                        | crate::domain::PlaylistKind::Smart
                )
            })
            .take(30)
            .collect::<Vec<_>>();
        assert!(
            !candidates.is_empty(),
            "Music.app should expose a playable playlist"
        );

        for playlist in candidates {
            let update = backend
                .execute(BackendCommand::LoadPlaylist(playlist.id))
                .await
                .expect("playlist track batch");
            if let BackendUpdate::PlaylistBatch { tracks, total, .. } = update {
                assert!(tracks.len() <= COLLECTION_BATCH_SIZE);
                if (1..=20).contains(&total) {
                    assert!(!tracks.is_empty());
                    assert!(
                        tracks.iter().all(|track| track.title != "Unknown Track"),
                        "playlist fallback must retain real metadata"
                    );
                    return;
                }
            }
        }
        panic!("no small non-empty playlist was available for the live smoke test");
    }

    #[tokio::test]
    #[ignore = "changes Music.app playback briefly and requires Automation consent"]
    async fn live_plays_selected_playlist_track_and_playlist_then_restores_playback() {
        let mut backend = MacOsMusicBackend::new();
        backend.snapshot().await.expect("initial Music.app state");
        let original_status = backend.snapshot.playback.status;
        let original_track = backend
            .snapshot
            .playback
            .current_track
            .as_ref()
            .map(|track| track.id.clone());
        let update = backend
            .tick(Duration::ZERO)
            .await
            .expect("playlist discovery")
            .expect("playlist update");
        let BackendUpdate::Playlists { playlists, .. } = update else {
            panic!("playlist discovery failed")
        };

        let mut selected = None;
        for playlist in playlists.into_iter().take(30) {
            if matches!(
                playlist.kind,
                crate::domain::PlaylistKind::Folder
                    | crate::domain::PlaylistKind::Library
                    | crate::domain::PlaylistKind::Smart
            ) {
                continue;
            }
            let update = backend
                .execute(BackendCommand::LoadPlaylist(playlist.id.clone()))
                .await
                .expect("playlist track batch");
            if let BackendUpdate::PlaylistBatch { tracks, total, .. } = update
                && (1..=20).contains(&total)
                && let Some(track) = tracks.first()
            {
                selected = Some((playlist.id, track.id.clone()));
                break;
            }
        }
        let (playlist_id, track_id) = selected.expect("small playable playlist");

        backend
            .execute(BackendCommand::PlayPlaylistTrack {
                playlist_id: playlist_id.clone(),
                track_id: track_id.clone(),
            })
            .await
            .expect("play exact playlist track");
        tokio::time::sleep(Duration::from_millis(750)).await;
        let BackendUpdate::Playback { playback, .. } = backend.poll().await else {
            panic!("playback update")
        };
        assert_eq!(playback.status, PlaybackStatus::Playing);
        assert_eq!(playback.current_track.expect("playing track").id, track_id);

        backend
            .execute(BackendCommand::PlayPlaylist(playlist_id))
            .await
            .expect("play exact playlist");
        tokio::time::sleep(Duration::from_millis(750)).await;
        let BackendUpdate::Playback { playback, .. } = backend.poll().await else {
            panic!("playlist playback update")
        };
        assert_eq!(playback.status, PlaybackStatus::Playing);

        if let Some(original_track) = original_track {
            backend
                .execute(BackendCommand::PlayTrack(original_track))
                .await
                .expect("restore original track");
            if original_status != PlaybackStatus::Playing {
                backend
                    .execute(BackendCommand::Pause)
                    .await
                    .expect("restore paused playback");
            }
        }
    }
}
