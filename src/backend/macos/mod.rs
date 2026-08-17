mod automation;
mod cache;
mod library;
mod parser;
mod script;

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;

use crate::{
    backend::{
        BackendCommand, BackendError, BackendUpdate, Capabilities, Capability, MusicBackend,
    },
    domain::{
        AlbumId, Artwork, ArtworkKey, ArtworkMediaType, ArtworkResult, BackendAvailability,
        BackendSnapshot, CollectionLoadState, PlaybackContext, PlaybackSnapshot, PlaybackStatus,
        PlaylistId, PlaylistLoadState, RepeatMode, Track, TrackId,
    },
};

use self::{
    automation::{AutomationError, AutomationRunner, SystemAutomationRunner},
    library::{
        derive_library, normalize_identifier, persistent_playlist_selector,
        persistent_track_selector, raw_playlist_to_domain, raw_track_to_domain,
    },
    parser::{RawArtwork, RawMusicState, RawScriptError, RawTrack, parse_output},
    script::{ScriptRequest, TrackSelector},
};

const COLLECTION_BATCH_SIZE: usize = 400;
/// A small first response makes Playlist Detail useful before Music.app has read every row.
/// Subsequent batches remain larger to preserve total-load throughput.
const PLAYLIST_INITIAL_BATCH_SIZE: usize = 40;
const PLAYLIST_CONTINUATION_BATCH_SIZE: usize = 200;
const MAX_ARTWORK_BYTES: usize = 2 * 1024 * 1024;
const INTERNAL_TRANSITION_GRACE: Duration = Duration::from_secs(3);
const PREVIOUS_RESTART_THRESHOLD: Duration = Duration::from_secs(3);
const COMPLETION_TOLERANCE: Duration = Duration::from_secs(2);
const TRANSITION_WATCH_THRESHOLD: Duration = Duration::from_millis(1_500);
const TRANSITION_WATCH_MIN: Duration = Duration::from_secs(1);
const TRANSITION_WATCH_MAX: Duration = Duration::from_secs(3);
const TRANSITION_WATCH_PADDING: Duration = Duration::from_secs(1);

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

#[derive(Clone, Debug, Eq, PartialEq)]
struct SessionTrack {
    id: TrackId,
    selector: TrackSelector,
    source_index: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PlaybackSessionSource {
    Playlist {
        playlist_id: PlaylistId,
        complete: bool,
        known_source_len: usize,
    },
    Album {
        album_id: AlbumId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ExpectedTransition {
    from: Option<TrackSelector>,
    to: TrackSelector,
    deadline: Instant,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PlaybackSession {
    source: PlaybackSessionSource,
    tracks: Vec<SessionTrack>,
    index: usize,
    /// The confirmed Music.app shuffle mode committed to this synthesized session.
    shuffle_enabled: bool,
    /// One ordering seed per enabled shuffle cycle; polls must not reshuffle it.
    shuffle_seed: Option<u64>,
    transition: Option<ExpectedTransition>,
    waiting_for_more: bool,
}

impl PlaybackSession {
    fn context(&self) -> PlaybackContext {
        let ordered_track_ids = self.tracks.iter().map(|track| track.id.clone()).collect();
        match &self.source {
            PlaybackSessionSource::Playlist {
                playlist_id,
                complete,
                ..
            } => PlaybackContext::Playlist {
                playlist_id: playlist_id.clone(),
                ordered_track_ids,
                current_index: self.index,
                current_source_index: self.tracks[self.index].source_index,
                complete: *complete,
            },
            PlaybackSessionSource::Album { album_id } => PlaybackContext::Album {
                album_id: album_id.clone(),
                ordered_track_ids,
                current_index: self.index,
            },
        }
    }

    const fn source_is_complete(&self) -> bool {
        match &self.source {
            PlaybackSessionSource::Playlist { complete, .. } => *complete,
            PlaybackSessionSource::Album { .. } => true,
        }
    }

    fn next_index(&self, repeat: RepeatMode) -> Option<usize> {
        if repeat == RepeatMode::One {
            Some(self.index)
        } else if self.index + 1 < self.tracks.len() {
            Some(self.index + 1)
        } else if self.source_is_complete() && repeat == RepeatMode::All {
            Some(0)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SessionAdvance {
    Play(usize),
    WaitForMore,
    End,
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
    playback_session: Option<PlaybackSession>,
    pending_context_error: Option<String>,
    cache_path: Option<PathBuf>,
    has_cached_library: bool,
}

/// Read-only metadata for the local Music.app library cache.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalCacheStatus {
    pub path: Option<PathBuf>,
    pub schema_version: Option<u32>,
    pub tracks: Option<usize>,
    pub playlists: Option<usize>,
    pub last_updated_unix_seconds: Option<u64>,
    pub readable: bool,
}

/// Result of explicitly removing only the persistent local metadata cache.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalCacheClearResult {
    Removed,
    NotFound,
    Unavailable,
}

impl MacOsMusicBackend {
    #[must_use]
    pub fn new() -> Self {
        let runner: Arc<dyn AutomationRunner> = Arc::new(SystemAutomationRunner);
        let installed = runner.is_installed();
        Self::with_runner_and_cache(runner, installed, cache::default_path())
    }

    #[must_use]
    pub fn local_cache_status() -> LocalCacheStatus {
        cache::inspect(cache::default_path())
    }

    pub fn clear_local_cache() -> std::io::Result<LocalCacheClearResult> {
        cache::clear(cache::default_path())
    }

    #[must_use]
    pub fn search_local_library(&self, query: &str, limit: usize) -> Vec<TrackId> {
        library::search_track_ids(&self.snapshot.library, query, limit)
    }

    #[cfg(test)]
    fn with_runner(runner: Arc<dyn AutomationRunner>, installed: bool) -> Self {
        Self::with_runner_and_cache(runner, installed, None)
    }

    fn with_runner_and_cache(
        runner: Arc<dyn AutomationRunner>,
        installed: bool,
        cache_path: Option<PathBuf>,
    ) -> Self {
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
            playback_session: None,
            pending_context_error: None,
            cache_path,
            has_cached_library: false,
        }
    }

    fn install_cached_library(&mut self, cached: cache::CachedLibrary) {
        self.snapshot.library = cached.library;
        self.snapshot.playlists = cached.playlists;
        let derived = derive_library(&self.snapshot.library);
        self.snapshot.artists = derived.artists;
        self.snapshot.albums = derived.albums;
        self.snapshot.recently_added = derived.recently_added;
        self.snapshot.recently_played = derived.recently_played;
        self.snapshot.library_status = CollectionLoadState::Cached {
            total: self.snapshot.library.len(),
        };
        self.snapshot.playlist_status = CollectionLoadState::Loaded {
            total: self.snapshot.playlists.len(),
        };
        self.has_cached_library = true;
    }

    fn schedule_library_cache_persist(&self) {
        let Some(path) = self.cache_path.clone() else {
            return;
        };
        let library = self.snapshot.library.clone();
        let track_count = library.len();
        let playlists = self.snapshot.playlists.clone();
        // Cache serialization and atomic disk I/O are deliberately detached from the single
        // automation worker. Waiting for this write here used to delay the next Space/Enter
        // command after a full refresh had completed.
        tokio::spawn(async move {
            let started = Instant::now();
            match tokio::task::spawn_blocking(move || cache::store(&path, &library, &playlists))
                .await
            {
                Ok(Ok(())) => tracing::debug!(
                    tracks = track_count,
                    cache_write_ms = started.elapsed().as_secs_f64() * 1_000.0,
                    "updated local Music.app library cache"
                ),
                Ok(Err(error)) => {
                    tracing::debug!(%error, "could not update local Music.app library cache")
                }
                Err(error) => {
                    tracing::debug!(%error, "local Music.app cache writer stopped unexpectedly")
                }
            }
        });
    }

    async fn query(&self, request: ScriptRequest) -> Result<RawMusicState, BackendFailure> {
        if !self.installed {
            return Err(BackendFailure::Unavailable);
        }

        tracing::trace!(?request, "Music.app query started");
        let automation_started = Instant::now();
        let runner = Arc::clone(&self.runner);
        let request_for_runner = request.clone();
        let output = tokio::task::spawn_blocking(move || runner.run(request_for_runner))
            .await
            .map_err(|error| BackendFailure::Error(format!("automation task failed: {error}")))?
            .map_err(classify_automation_error)?;
        let automation_elapsed = automation_started.elapsed();
        let parse_started = Instant::now();
        let raw = parse_output(&output).map_err(|error| {
            BackendFailure::Error(format!("invalid Music.app response: {error}"))
        })?;
        let parse_elapsed = parse_started.elapsed();
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
        tracing::debug!(
            operation = ?request,
            automation_ms = automation_elapsed.as_secs_f64() * 1_000.0,
            parse_ms = parse_elapsed.as_secs_f64() * 1_000.0,
            response_bytes = output.len(),
            "Music.app query timing"
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
        self.reconcile_session_current_track();
        self.sync_playback_context();
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
        self.playback_session = None;
        self.sync_playback_context();
    }

    fn playback_update(&self) -> BackendUpdate {
        BackendUpdate::Playback {
            availability: self.snapshot.availability.clone(),
            playback: self.snapshot.playback.clone(),
        }
    }

    fn library_refresh_failure(&mut self, failure: BackendFailure) -> BackendUpdate {
        tracing::debug!(?failure, "Music.app library refresh failed");
        self.snapshot.availability = availability_for_failure(failure.clone());
        self.snapshot.library_status =
            CollectionLoadState::Error(refresh_failure_message(&failure));
        self.phase = LoadPhase::Ready;
        BackendUpdate::LibraryRefreshFailed {
            availability: self.snapshot.availability.clone(),
            playback: self.snapshot.playback.clone(),
            message: refresh_failure_message(&failure),
        }
    }

    fn sync_playback_context(&mut self) {
        self.snapshot.playback.context = self
            .playback_session
            .as_ref()
            .map_or(PlaybackContext::NoContext, PlaybackSession::context);
    }

    /// Reconcile only the cursor of an existing synthesized session with Music.app.
    /// The ordered vector remains backend-owned; a current track outside that vector is left for
    /// normal external-change handling to cancel rather than recreate the session.
    fn reconcile_session_current_track(&mut self) {
        let Some(current_track) = self.snapshot.playback.current_track.as_ref() else {
            return;
        };
        let Some(session) = self.playback_session.as_mut() else {
            return;
        };
        if let Some(transition) = &session.transition {
            // During an exact-track command Music.app can briefly report the previous source
            // track.  Its prepared session index remains authoritative until the transition
            // target is observed or the regular transition reconciliation decides otherwise.
            if track_selector(&current_track.id).is_ok_and(|selector| selector != transition.to) {
                return;
            }
        }
        let Some(index) = session
            .tracks
            .iter()
            .position(|track| track.id == current_track.id)
        else {
            return;
        };
        if session.index != index {
            tracing::debug!(
                previous_index = session.index,
                current_index = index,
                track_id = %current_track.id,
                "synthesized session position reconciled from Music.app"
            );
            session.index = index;
            session.waiting_for_more = false;
        }
    }

    fn cancel_playback_session(&mut self, reason: &str) {
        if self.playback_session.take().is_some() {
            tracing::debug!(reason, "synthesized playback context ended");
            self.sync_playback_context();
        }
    }

    fn reconcile_removed_playlist_entry(
        &mut self,
        playlist_id: &PlaylistId,
        source_index: usize,
        track_id: &TrackId,
    ) {
        let Some(session) = self.playback_session.as_mut() else {
            return;
        };
        let PlaybackSessionSource::Playlist {
            playlist_id: session_playlist_id,
            known_source_len,
            ..
        } = &mut session.source
        else {
            return;
        };
        if session_playlist_id != playlist_id {
            return;
        }
        *known_source_len = known_source_len.saturating_sub(1);
        let Some(position) = session
            .tracks
            .iter()
            .position(|track| track.source_index == source_index && track.id == *track_id)
        else {
            return;
        };
        let keep_current = position == session.index
            && self
                .snapshot
                .playback
                .current_track
                .as_ref()
                .is_some_and(|track| track.id == *track_id);
        if !keep_current {
            session.tracks.remove(position);
            if position < session.index {
                session.index = session.index.saturating_sub(1);
            }
        }
        for track in &mut session.tracks {
            if track.source_index > source_index {
                track.source_index = track.source_index.saturating_sub(1);
            }
        }
        session.waiting_for_more = false;
        self.sync_playback_context();
    }

    fn session_failure_update(&mut self, prefix: &str, failure: BackendFailure) -> BackendUpdate {
        let detail = failure_message(&failure);
        match failure {
            BackendFailure::NotRunning => {
                self.snapshot.availability = BackendAvailability::NotRunning;
            }
            BackendFailure::Unavailable => {
                self.snapshot.availability = BackendAvailability::Unavailable;
            }
            BackendFailure::PermissionDenied => {
                self.snapshot.availability = BackendAvailability::PermissionDenied;
            }
            BackendFailure::Error(_) => {}
        }
        self.playback_session = None;
        self.sync_playback_context();
        BackendUpdate::PlaybackContextFailed {
            availability: self.snapshot.availability.clone(),
            playback: self.snapshot.playback.clone(),
            message: format!("{prefix}: {detail}"),
        }
    }

    fn defer_session_failure(&mut self, prefix: &str, failure: BackendFailure) {
        let BackendUpdate::PlaybackContextFailed { message, .. } =
            self.session_failure_update(prefix, failure)
        else {
            unreachable!("session failures always create a context failure update")
        };
        self.pending_context_error = Some(message);
    }

    async fn poll(&mut self) -> BackendUpdate {
        match self.query(ScriptRequest::Poll).await {
            Ok(raw) => {
                let previous = self.snapshot.playback.clone();
                self.apply_raw_playback(&raw);
                self.reconcile_session_shuffle();
                if let Err(failure) = self.reconcile_playback_session(&raw, &previous).await {
                    return self.session_failure_update(
                        "Failed to continue synthesized playback",
                        failure,
                    );
                }
            }
            Err(failure) => self.apply_failure(failure),
        }
        self.playback_update()
    }

    async fn play_session_index(&mut self, index: usize) -> Result<(), BackendFailure> {
        let Some(session) = self.playback_session.as_ref() else {
            return Ok(());
        };
        let Some(target) = session.tracks.get(index).cloned() else {
            return Err(BackendFailure::Error(
                "session track index is no longer available".to_owned(),
            ));
        };
        let from = self
            .snapshot
            .playback
            .current_track
            .as_ref()
            .and_then(|track| track_selector(&track.id).ok());
        let request = match &session.source {
            PlaybackSessionSource::Playlist { playlist_id, .. } => {
                let Some(persistent_id) = persistent_playlist_selector(playlist_id) else {
                    return Err(BackendFailure::Error(
                        "playlist identity is no longer available".to_owned(),
                    ));
                };
                ScriptRequest::PlayPlaylistTrackOnce {
                    playlist_persistent_id: persistent_id.to_owned(),
                    track: target.selector.clone(),
                }
            }
            PlaybackSessionSource::Album { .. } => {
                ScriptRequest::PlayTrackOnce(target.selector.clone())
            }
        };
        if let Some(session) = self.playback_session.as_mut() {
            session.index = index;
            session.waiting_for_more = false;
            session.transition = Some(ExpectedTransition {
                from,
                to: target.selector.clone(),
                deadline: Instant::now() + INTERNAL_TRANSITION_GRACE,
            });
        }
        self.sync_playback_context();
        let raw = self.query(request).await?;
        self.apply_raw_playback(&raw);
        if raw_track_matches_selector(raw.track.as_ref(), &target.selector)
            && let Some(session) = self.playback_session.as_mut()
        {
            session.transition = None;
        }
        self.sync_playback_context();
        Ok(())
    }

    fn create_album_session(
        &mut self,
        album_id: AlbumId,
        track_ids: Vec<TrackId>,
    ) -> Result<(), BackendError> {
        let mut tracks = track_ids
            .into_iter()
            .enumerate()
            .map(|(source_index, id)| {
                track_selector(&id).map(|selector| SessionTrack {
                    id,
                    selector,
                    source_index,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if tracks.is_empty() {
            return Err(BackendError::AlbumNotFound(album_id));
        }
        let shuffle_enabled = self.snapshot.playback.shuffle;
        let shuffle_seed = shuffle_enabled.then(|| {
            let mut hasher = DefaultHasher::new();
            album_id.hash(&mut hasher);
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
                .hash(&mut hasher);
            hasher.finish()
        });
        if let Some(seed) = shuffle_seed {
            let current = tracks.remove(0);
            tracks.sort_by_key(|track| shuffled_track_rank(seed, track));
            if tracks.len() > 1
                && tracks
                    .iter()
                    .map(|track| track.source_index)
                    .eq(1..tracks.len() + 1)
            {
                tracks.rotate_left(1);
            }
            tracks.insert(0, current);
        }
        self.playback_session = Some(PlaybackSession {
            source: PlaybackSessionSource::Album { album_id },
            tracks,
            index: 0,
            shuffle_enabled,
            shuffle_seed,
            transition: None,
            waiting_for_more: false,
        });
        self.sync_playback_context();
        Ok(())
    }

    fn create_playlist_session(
        &mut self,
        playlist_id: PlaylistId,
        track_ids: Vec<TrackId>,
        selected_index: usize,
        complete: bool,
    ) -> Result<(), BackendError> {
        let selected_id = track_ids
            .get(selected_index)
            .cloned()
            .ok_or_else(|| BackendError::PlaylistNotFound(playlist_id.clone()))?;
        let known_source_len = track_ids.len();
        let mut tracks = track_ids
            .into_iter()
            .enumerate()
            .filter_map(|(source_index, id)| {
                track_selector(&id).ok().map(|selector| SessionTrack {
                    id,
                    selector,
                    source_index,
                })
            })
            .collect::<Vec<_>>();
        let Some(mut session_index) = tracks
            .iter()
            .position(|track| track.source_index == selected_index && track.id == selected_id)
        else {
            return Err(BackendError::TrackNotFound(selected_id));
        };
        let shuffle_seed = self
            .snapshot
            .playback
            .shuffle
            .then(|| playlist_shuffle_seed(&playlist_id));
        if let Some(seed) = shuffle_seed {
            // Keep the selected stable track at its real session position.  Only the
            // unplayed tail is shuffled; prior entries remain history rather than becoming
            // future candidates again.
            let mut future = tracks.split_off(session_index.saturating_add(1));
            let canonical_future = future.clone();
            future.sort_by_key(|track| shuffled_track_rank(seed, track));
            if future.len() > 1 && future == canonical_future {
                future.rotate_left(1);
            }
            tracks.extend(future);
        }
        session_index = tracks
            .iter()
            .position(|track| track.id == selected_id)
            .ok_or_else(|| BackendError::TrackNotFound(selected_id.clone()))?;
        self.playback_session = Some(PlaybackSession {
            source: PlaybackSessionSource::Playlist {
                playlist_id,
                complete,
                known_source_len,
            },
            tracks,
            index: session_index,
            shuffle_enabled: shuffle_seed.is_some(),
            shuffle_seed,
            transition: None,
            waiting_for_more: false,
        });
        self.sync_playback_context();
        Ok(())
    }

    fn extend_playlist_session(&mut self, playlist_id: &PlaylistId) {
        let Some(playlist) = self
            .snapshot
            .playlists
            .iter()
            .find(|playlist| playlist.id == *playlist_id)
        else {
            return;
        };
        let ids = playlist
            .tracks
            .iter()
            .map(|track| track.id.clone())
            .collect::<Vec<_>>();
        let complete = playlist.contents_state.is_complete();
        let Some(session) = self.playback_session.as_mut() else {
            return;
        };
        let PlaybackSessionSource::Playlist {
            playlist_id: session_playlist_id,
            complete: session_complete,
            known_source_len,
        } = &mut session.source
        else {
            return;
        };
        if session_playlist_id != playlist_id || ids.len() <= *known_source_len {
            *session_complete = complete;
            self.sync_playback_context();
            return;
        }
        let mut additions = ids
            .into_iter()
            .enumerate()
            .skip(*known_source_len)
            .filter_map(|(source_index, id)| {
                track_selector(&id).ok().map(|selector| SessionTrack {
                    id,
                    selector,
                    source_index,
                })
            })
            .collect::<Vec<_>>();
        *known_source_len = playlist.tracks.len();
        *session_complete = complete;
        if session.shuffle_enabled {
            let seed = session.shuffle_seed.unwrap_or_else(|| {
                let seed = playlist_shuffle_seed(session_playlist_id);
                session.shuffle_seed = Some(seed);
                seed
            });
            let tail_start = session.index.saturating_add(1);
            let mut tail = session
                .tracks
                .split_off(tail_start.min(session.tracks.len()));
            tail.append(&mut additions);
            tail.sort_by_key(|track| shuffled_track_rank(seed, track));
            session.tracks.extend(tail);
        } else {
            session.tracks.append(&mut additions);
        }
        self.sync_playback_context();
    }

    fn reconcile_session_shuffle(&mut self) {
        let Some(session) = self.playback_session.as_mut() else {
            return;
        };
        let shuffle_before = session.shuffle_enabled;
        let tail_start = session.index.saturating_add(1).min(session.tracks.len());
        let future_before = session.tracks[tail_start..]
            .iter()
            .map(|track| track.id.to_string())
            .collect::<Vec<_>>();
        let shuffle = self.snapshot.playback.shuffle;
        if shuffle == shuffle_before {
            tracing::debug!(shuffle, future_unchanged = true, "synthesized session poll");
            return;
        }
        let mut tail = session.tracks.split_off(tail_start);
        let original_tail = tail.clone();
        if shuffle {
            let seed = match &session.source {
                PlaybackSessionSource::Playlist { playlist_id, .. } => {
                    playlist_shuffle_seed(playlist_id)
                }
                PlaybackSessionSource::Album { album_id } => {
                    let mut hasher = DefaultHasher::new();
                    album_id.hash(&mut hasher);
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_nanos()
                        .hash(&mut hasher);
                    hasher.finish()
                }
            };
            session.shuffle_seed = Some(seed);
            tail.sort_by_key(|track| shuffled_track_rank(seed, track));
        } else {
            session.shuffle_seed = None;
            tail.sort_by_key(|track| track.source_index);
        }
        if shuffle && tail.len() > 1 && tail == original_tail {
            tail.rotate_left(1);
        }
        session.tracks.extend(tail);
        session.shuffle_enabled = shuffle;
        tracing::debug!(
            shuffle_before,
            shuffle_after = shuffle,
            current = %session.tracks[session.index].id,
            history = ?session.tracks[..session.index].iter().map(|track| track.id.to_string()).collect::<Vec<_>>(),
            future_before = ?future_before,
            future_after = ?session.tracks[session.index.saturating_add(1)..].iter().map(|track| track.id.to_string()).collect::<Vec<_>>(),
            "synthesized session shuffle toggle"
        );
        self.sync_playback_context();
    }

    fn automatic_session_advance(&mut self) -> SessionAdvance {
        let Some(session) = self.playback_session.as_mut() else {
            return SessionAdvance::End;
        };
        if let Some(index) = session.next_index(self.snapshot.playback.repeat) {
            return SessionAdvance::Play(index);
        }
        if !session.source_is_complete() {
            session.waiting_for_more = true;
            self.sync_playback_context();
            return SessionAdvance::WaitForMore;
        }
        SessionAdvance::End
    }

    async fn resume_waiting_session(&mut self) -> Result<(), BackendFailure> {
        let should_resume = self
            .playback_session
            .as_ref()
            .is_some_and(|session| session.waiting_for_more);
        if !should_resume {
            return Ok(());
        }
        match self.automatic_session_advance() {
            SessionAdvance::Play(index) => self.play_session_index(index).await,
            SessionAdvance::WaitForMore => Ok(()),
            SessionAdvance::End => {
                self.cancel_playback_session("playlist reached its final available track");
                Ok(())
            }
        }
    }

    async fn reconcile_playback_session(
        &mut self,
        raw: &RawMusicState,
        previous: &PlaybackSnapshot,
    ) -> Result<(), BackendFailure> {
        let Some(session) = self.playback_session.as_ref() else {
            return Ok(());
        };
        if let Some(transition) = session.transition.clone() {
            if raw_track_matches_selector(raw.track.as_ref(), &transition.to) {
                if let Some(session) = self.playback_session.as_mut() {
                    session.transition = None;
                }
                self.sync_playback_context();
                return Ok(());
            }
            let still_on_source = transition
                .from
                .as_ref()
                .is_some_and(|from| raw_track_matches_selector(raw.track.as_ref(), from));
            if Instant::now() <= transition.deadline && (still_on_source || raw.track.is_none()) {
                return Ok(());
            }
            self.cancel_playback_session(
                "Music.app selected a track outside the expected transition",
            );
            return Ok(());
        }

        let expected = session.tracks[session.index].selector.clone();
        let matches_expected = raw_track_matches_selector(raw.track.as_ref(), &expected);
        if raw.track.is_some() && !matches_expected {
            self.cancel_playback_session("Music.app selected a different track");
            return Ok(());
        }
        if self.snapshot.playback.status != PlaybackStatus::Stopped {
            return Ok(());
        }
        let previous_matches_expected = previous.current_track.as_ref().is_some_and(|track| {
            track_selector(&track.id).is_ok_and(|selector| selector == expected)
        });
        if !(matches_expected || (raw.track.is_none() && previous_matches_expected))
            || !is_natural_completion(previous, &self.snapshot.playback)
        {
            if previous.status == PlaybackStatus::Playing {
                self.cancel_playback_session("Music.app stopped before the expected natural end");
            }
            return Ok(());
        }

        match self.automatic_session_advance() {
            SessionAdvance::Play(index) => self.play_session_index(index).await,
            SessionAdvance::WaitForMore => Ok(()),
            SessionAdvance::End => {
                self.cancel_playback_session("synthesized collection reached its final track");
                Ok(())
            }
        }
    }

    fn session_near_transition(&self) -> bool {
        self.playback_session.is_some()
            && self.snapshot.playback.status == PlaybackStatus::Playing
            && self
                .snapshot
                .playback
                .current_track
                .as_ref()
                .is_some_and(|track| {
                    track
                        .duration
                        .saturating_sub(self.snapshot.playback.position)
                        <= TRANSITION_WATCH_THRESHOLD
                })
    }

    fn playlist_transition_wait_ms(&self) -> u64 {
        let remaining =
            self.snapshot
                .playback
                .current_track
                .as_ref()
                .map_or(Duration::ZERO, |track| {
                    track
                        .duration
                        .saturating_sub(self.snapshot.playback.position)
                });
        let wait = remaining
            .saturating_add(TRANSITION_WATCH_PADDING)
            .max(TRANSITION_WATCH_MIN)
            .min(TRANSITION_WATCH_MAX);
        u64::try_from(wait.as_millis()).unwrap_or(u64::MAX)
    }

    fn playlist_transition_plan(
        &self,
    ) -> Option<(PlaylistId, TrackSelector, TrackSelector, usize)> {
        let session = self.playback_session.as_ref()?;
        let PlaybackSessionSource::Playlist { playlist_id, .. } = &session.source else {
            return None;
        };
        let target_index = session.next_index(self.snapshot.playback.repeat)?;
        Some((
            playlist_id.clone(),
            session.tracks[session.index].selector.clone(),
            session.tracks[target_index].selector.clone(),
            target_index,
        ))
    }

    async fn poll_playlist_transition(&mut self) -> BackendUpdate {
        let Some((playlist_id, expected, target, target_index)) = self.playlist_transition_plan()
        else {
            return self.poll().await;
        };
        let Some(playlist_persistent_id) =
            persistent_playlist_selector(&playlist_id).map(str::to_owned)
        else {
            self.cancel_playback_session("playlist transition identity became unavailable");
            return self.playback_update();
        };
        let previous = self.snapshot.playback.clone();
        let max_wait_ms = self.playlist_transition_wait_ms();
        match self
            .query(ScriptRequest::PollPlaylistTransition {
                playlist_persistent_id,
                expected,
                target,
                max_wait_ms,
            })
            .await
        {
            Ok(raw) => {
                if raw.session_advanced
                    && let Some(session) = self.playback_session.as_mut()
                {
                    session.index = target_index;
                    session.transition = None;
                    session.waiting_for_more = false;
                }
                self.apply_raw_playback(&raw);
                self.reconcile_session_shuffle();
                if !raw.session_advanced
                    && let Err(failure) = self.reconcile_playback_session(&raw, &previous).await
                {
                    return self
                        .session_failure_update("Failed to continue playlist playback", failure);
                }
                self.sync_playback_context();
                self.playback_update()
            }
            Err(failure) => {
                self.session_failure_update("Failed to continue playlist playback", failure)
            }
        }
    }

    async fn discover_playlists(&mut self) -> BackendUpdate {
        let raw = match self.query(ScriptRequest::DiscoverPlaylists).await {
            Ok(raw) => raw,
            Err(failure) => {
                return self.library_refresh_failure(failure);
            }
        };
        let previous = self.snapshot.playback.clone();
        self.apply_raw_playback(&raw);
        if let Err(failure) = self.reconcile_playback_session(&raw, &previous).await {
            self.defer_session_failure("Failed to continue synthesized playback", failure);
        }
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
        if !self.has_cached_library {
            self.snapshot.library.clear();
        }
        self.snapshot.library_status = if self.has_cached_library {
            CollectionLoadState::Refreshing {
                loaded: 0,
                total: 0,
            }
        } else {
            CollectionLoadState::Loading {
                loaded: 0,
                total: 0,
            }
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
                return self.library_refresh_failure(failure);
            }
        };
        let previous = self.snapshot.playback.clone();
        self.apply_raw_playback(&raw);
        if let Err(failure) = self.reconcile_playback_session(&raw, &previous).await {
            self.defer_session_failure("Failed to continue synthesized playback", failure);
        }
        let Some(batch) = raw.library_batch else {
            return self.playback_update();
        };
        let conversion_started = Instant::now();
        let tracks = batch
            .tracks
            .into_iter()
            .map(raw_track_to_domain)
            .collect::<Vec<_>>();
        let conversion_elapsed = conversion_started.elapsed();
        let merge_started = Instant::now();
        if batch.start == 0 {
            self.snapshot.library.clear();
        }
        self.snapshot.library.extend(tracks.iter().cloned());
        let merge_elapsed = merge_started.elapsed();
        let loaded = batch.start.saturating_add(tracks.len()).min(batch.total);
        let complete = loaded >= batch.total;
        let derived = if complete {
            let started = Instant::now();
            let derived = derive_library(&self.snapshot.library);
            tracing::debug!(
                derive_ms = started.elapsed().as_secs_f64() * 1_000.0,
                tracks = self.snapshot.library.len(),
                "local library derivation timing"
            );
            derived
        } else {
            Default::default()
        };
        tracing::debug!(
            start = batch.start,
            tracks = tracks.len(),
            conversion_ms = conversion_elapsed.as_secs_f64() * 1_000.0,
            backend_merge_ms = merge_elapsed.as_secs_f64() * 1_000.0,
            complete,
            "local library batch processing timing"
        );
        if complete {
            self.snapshot.artists = derived.artists.clone();
            self.snapshot.albums = derived.albums.clone();
            self.snapshot.recently_added = derived.recently_added.clone();
            self.snapshot.recently_played = derived.recently_played.clone();
            self.snapshot.library_status = CollectionLoadState::Loaded { total: batch.total };
            self.phase = LoadPhase::Ready;
            self.schedule_library_cache_persist();
        } else {
            self.snapshot.library_status = if self.has_cached_library {
                CollectionLoadState::Refreshing {
                    loaded,
                    total: batch.total,
                }
            } else {
                CollectionLoadState::Loading {
                    loaded,
                    total: batch.total,
                }
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
            authoritative_tracks: complete.then(|| self.snapshot.library.clone()),
            loaded,
            total: batch.total,
            complete,
            artists: derived.artists,
            albums: derived.albums,
            recently_added: derived.recently_added,
            recently_played: derived.recently_played,
        }
    }

    async fn load_playlist_batch(&mut self, load: PendingPlaylistLoad) -> BackendUpdate {
        let batch_started = Instant::now();
        let batch_limit = if load.next == 0 {
            PLAYLIST_INITIAL_BATCH_SIZE
        } else {
            PLAYLIST_CONTINUATION_BATCH_SIZE
        };
        let Some(persistent_id) = persistent_playlist_selector(&load.id).map(str::to_owned) else {
            self.pending_playlist = None;
            return self.playback_update();
        };
        let raw = match self
            .query(ScriptRequest::PlaylistBatch {
                playlist_persistent_id: persistent_id,
                start: load.next,
                limit: batch_limit,
                total: load.total,
            })
            .await
        {
            Ok(raw) => raw,
            Err(failure) => {
                self.pending_playlist = None;
                let message = failure_message(&failure);
                match failure {
                    BackendFailure::NotRunning => {
                        self.snapshot.availability = BackendAvailability::NotRunning;
                    }
                    BackendFailure::Unavailable => {
                        self.snapshot.availability = BackendAvailability::Unavailable;
                    }
                    BackendFailure::PermissionDenied => {
                        self.snapshot.availability = BackendAvailability::PermissionDenied;
                    }
                    BackendFailure::Error(_) => {}
                }
                if let Some(playlist) = self
                    .snapshot
                    .playlists
                    .iter_mut()
                    .find(|playlist| playlist.id == load.id)
                {
                    playlist.contents_state = PlaylistLoadState::Error(message.clone());
                }
                return BackendUpdate::PlaylistLoadFailed {
                    availability: self.snapshot.availability.clone(),
                    playback: self.snapshot.playback.clone(),
                    playlist_id: load.id,
                    message,
                };
            }
        };
        let previous = self.snapshot.playback.clone();
        self.apply_raw_playback(&raw);
        let Some(batch) = raw.playlist_batch.clone() else {
            self.pending_playlist = None;
            let message = "Music.app omitted the requested playlist batch".to_owned();
            return BackendUpdate::PlaylistLoadFailed {
                availability: self.snapshot.availability.clone(),
                playback: self.snapshot.playback.clone(),
                playlist_id: load.id,
                message,
            };
        };
        let tracks = batch
            .tracks
            .into_iter()
            .map(raw_track_to_domain)
            .collect::<Vec<_>>();
        let loaded = batch.start.saturating_add(tracks.len()).min(batch.total);
        let complete = loaded >= batch.total;
        if tracks.is_empty() && !complete {
            self.pending_playlist = None;
            let message = format!(
                "Music.app returned no tracks at offset {} before the reported total {}",
                batch.start, batch.total
            );
            if let Some(playlist) = self
                .snapshot
                .playlists
                .iter_mut()
                .find(|playlist| playlist.id == load.id)
            {
                playlist.contents_state = PlaylistLoadState::Error(message.clone());
            }
            return BackendUpdate::PlaylistLoadFailed {
                availability: self.snapshot.availability.clone(),
                playback: self.snapshot.playback.clone(),
                playlist_id: load.id,
                message,
            };
        }
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
            playlist.contents_state = if complete && batch.total == 0 {
                PlaylistLoadState::Empty
            } else if complete {
                PlaylistLoadState::Loaded { total: batch.total }
            } else if loaded == 0 {
                PlaylistLoadState::Loading {
                    loaded,
                    total: Some(batch.total),
                }
            } else {
                PlaylistLoadState::PartiallyLoaded {
                    loaded,
                    total: batch.total,
                }
            };
        }
        self.extend_playlist_session(&load.id);
        if let Err(failure) = self.reconcile_playback_session(&raw, &previous).await {
            self.defer_session_failure("Failed to continue playlist playback", failure);
        } else if let Err(failure) = self.resume_waiting_session().await {
            self.defer_session_failure("Failed to continue playlist playback", failure);
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
        tracing::debug!(
            playlist_id = %load.id,
            start = batch.start,
            limit = batch_limit,
            tracks = tracks.len(),
            loaded,
            total = batch.total,
            complete,
            batch_total_ms = batch_started.elapsed().as_secs_f64() * 1_000.0,
            "Music.app playlist batch timing"
        );
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

    async fn load_track_artwork(&mut self, key: ArtworkKey, track_id: TrackId) -> BackendUpdate {
        let result = match track_selector(&track_id) {
            Ok(track) => match self
                .query(ScriptRequest::LoadTrackArtwork {
                    track,
                    max_bytes: MAX_ARTWORK_BYTES,
                })
                .await
            {
                Ok(raw) => {
                    let resolver = raw
                        .artwork
                        .as_ref()
                        .and_then(|artwork| artwork.resolver.clone())
                        .unwrap_or_else(|| "none".to_owned());
                    let attempts = raw
                        .artwork
                        .as_ref()
                        .map_or_else(Vec::new, |artwork| artwork.attempts.clone());
                    self.apply_raw_playback(&raw);
                    let result = artwork_result(raw.artwork);
                    tracing::debug!(
                        key = ?key,
                        track_id = %track_id,
                        resolver = %resolver,
                        attempts = ?attempts,
                        "Music.app artwork resolver completed"
                    );
                    result
                }
                Err(failure) => self.apply_artwork_failure(failure),
            },
            Err(_) => ArtworkResult::Invalid("Track has no stable Music.app identifier".to_owned()),
        };
        match &result {
            ArtworkResult::Ready(artwork) => tracing::debug!(
                key = ?key,
                track_id = %track_id,
                bytes = artwork.bytes.len(),
                format = ?artwork.media_type,
                "Music.app artwork extracted"
            ),
            ArtworkResult::Missing => {
                tracing::debug!(key = ?key, track_id = %track_id, "Music.app returned no artwork")
            }
            ArtworkResult::Transient(message) => {
                tracing::debug!(key = ?key, track_id = %track_id, %message, "Music.app artwork resolution was transient")
            }
            ArtworkResult::TooLarge { encoded_bytes } => {
                tracing::debug!(key = ?key, track_id = %track_id, %encoded_bytes, "Music.app artwork exceeded limit")
            }
            ArtworkResult::Invalid(message) => {
                tracing::debug!(key = ?key, track_id = %track_id, %message, "Music.app artwork was invalid")
            }
        }
        BackendUpdate::Artwork {
            availability: self.snapshot.availability.clone(),
            playback: self.snapshot.playback.clone(),
            key,
            result,
        }
    }

    fn apply_artwork_failure(&mut self, failure: BackendFailure) -> ArtworkResult {
        let message = match failure {
            BackendFailure::NotRunning => {
                self.snapshot.availability = BackendAvailability::NotRunning;
                "Music.app is not running".to_owned()
            }
            BackendFailure::Unavailable => {
                self.snapshot.availability = BackendAvailability::Unavailable;
                "Music.app is unavailable".to_owned()
            }
            BackendFailure::PermissionDenied => {
                self.snapshot.availability = BackendAvailability::PermissionDenied;
                "Permission to read Music.app artwork was denied".to_owned()
            }
            BackendFailure::Error(message) => message,
        };
        ArtworkResult::Invalid(format!("Music.app artwork query failed: {message}"))
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
        let cache_load = self
            .cache_path
            .clone()
            .map(|path| tokio::task::spawn_blocking(move || cache::load(&path)));
        match self.query(ScriptRequest::FullState).await {
            Ok(raw) => {
                self.apply_raw_playback(&raw);
                self.phase = LoadPhase::DiscoverPlaylists;
            }
            Err(failure) => self.apply_failure(failure),
        }
        if let Some(cache_load) = cache_load
            && let Ok(Some(cached)) = cache_load.await
        {
            self.install_cached_library(cached);
        }
        if !self.has_cached_library && self.installed {
            self.snapshot.playlist_status = CollectionLoadState::Loading {
                loaded: 0,
                total: 0,
            };
            self.snapshot.library_status = CollectionLoadState::NotStarted;
        }
        Ok(self.snapshot.clone())
    }

    async fn execute(&mut self, command: BackendCommand) -> Result<BackendUpdate, BackendError> {
        let request = match command {
            BackendCommand::RefreshLibrary => {
                if !matches!(self.phase, LoadPhase::Ready) {
                    return Ok(BackendUpdate::Notice {
                        availability: self.snapshot.availability.clone(),
                        playback: self.snapshot.playback.clone(),
                        message: "Library refresh already in progress".to_owned(),
                    });
                }
                self.snapshot.library_status = CollectionLoadState::Refreshing {
                    loaded: 0,
                    total: 0,
                };
                self.phase = LoadPhase::DiscoverPlaylists;
                return Ok(BackendUpdate::LibraryRefreshStarted {
                    availability: self.snapshot.availability.clone(),
                    playback: self.snapshot.playback.clone(),
                });
            }
            BackendCommand::OpenPlayer => ScriptRequest::OpenPlayer,
            BackendCommand::Play => ScriptRequest::Play,
            BackendCommand::Pause => ScriptRequest::Pause,
            BackendCommand::Stop => {
                self.cancel_playback_session("application shutdown requested playback stop");
                let update = self.run_playback_command(ScriptRequest::Stop).await;
                return Ok(BackendUpdate::Stopped {
                    availability: self.snapshot.availability.clone(),
                    playback: match update {
                        BackendUpdate::Playback { playback, .. } => playback,
                        _ => self.snapshot.playback.clone(),
                    },
                });
            }
            BackendCommand::PlayPause => ScriptRequest::PlayPause,
            BackendCommand::PlayTrack(track_id) => {
                self.cancel_playback_session("a standalone track was selected");
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
                ordered_track_ids,
                selected_index,
                complete,
            } => {
                self.cancel_playback_session("a new playlist track was selected");
                let session_playlist_id = playlist_id.clone();
                self.create_playlist_session(
                    playlist_id,
                    ordered_track_ids,
                    selected_index,
                    complete,
                )?;
                self.extend_playlist_session(&session_playlist_id);
                let index = self
                    .playback_session
                    .as_ref()
                    .map_or(0, |session| session.index);
                return Ok(match self.play_session_index(index).await {
                    Ok(()) => self.playback_update(),
                    Err(failure) => {
                        self.session_failure_update("Failed to start playlist playback", failure)
                    }
                });
            }
            BackendCommand::PlayPlaylist(playlist_id) => {
                self.cancel_playback_session("a new playlist playback context was selected");
                if let Some(playlist) = self
                    .snapshot
                    .playlists
                    .iter()
                    .find(|playlist| playlist.id == playlist_id)
                    .filter(|playlist| !playlist.tracks.is_empty())
                {
                    let track_ids = playlist
                        .tracks
                        .iter()
                        .map(|track| track.id.clone())
                        .collect::<Vec<_>>();
                    let complete = playlist.contents_state.is_complete();
                    self.create_playlist_session(playlist_id, track_ids, 0, complete)?;
                    return Ok(match self.play_session_index(0).await {
                        Ok(()) => self.playback_update(),
                        Err(failure) => self.session_failure_update(
                            "Failed to start synthesized playlist playback",
                            failure,
                        ),
                    });
                }
                let Some(persistent_id) = persistent_playlist_selector(&playlist_id) else {
                    return Err(BackendError::PlaylistNotFound(playlist_id));
                };
                ScriptRequest::PlayPlaylist(persistent_id.to_owned())
            }
            BackendCommand::PlayAlbum {
                album_id,
                track_ids,
            } => {
                self.cancel_playback_session("a new album was selected");
                self.create_album_session(album_id, track_ids)?;
                return Ok(match self.play_session_index(0).await {
                    Ok(()) => self.playback_update(),
                    Err(failure) => {
                        self.session_failure_update("Failed to start album playback", failure)
                    }
                });
            }
            BackendCommand::LoadTrackArtwork { key, track_id } => {
                return Ok(self.load_track_artwork(key, track_id).await);
            }
            BackendCommand::LoadPlaylist(playlist_id) => {
                if self
                    .snapshot
                    .playlists
                    .iter()
                    .find(|playlist| playlist.id == playlist_id)
                    .is_some_and(|playlist| playlist.contents_state.is_complete())
                {
                    return Ok(self.playback_update());
                }
                if let Some(playlist) = self
                    .snapshot
                    .playlists
                    .iter_mut()
                    .find(|playlist| playlist.id == playlist_id)
                {
                    playlist.contents_state = PlaylistLoadState::Loading {
                        loaded: playlist.tracks.len(),
                        total: (playlist.track_count > 0).then_some(playlist.track_count),
                    };
                }
                let load = PendingPlaylistLoad {
                    id: playlist_id,
                    next: 0,
                    total: None,
                };
                self.pending_playlist = Some(load.clone());
                return Ok(self.load_playlist_batch(load).await);
            }
            BackendCommand::RemovePlaylistTrack {
                playlist_id,
                index,
                expected_track_id,
            } => {
                let Some(playlist) = self
                    .snapshot
                    .playlists
                    .iter()
                    .find(|playlist| playlist.id == playlist_id)
                else {
                    return Err(BackendError::PlaylistNotFound(playlist_id));
                };
                if playlist.kind != crate::domain::PlaylistKind::User {
                    return Err(BackendError::Unsupported(Capability::PlaylistTrackRemove));
                }
                if playlist
                    .tracks
                    .get(index)
                    .is_none_or(|track| track.id != expected_track_id)
                {
                    return Err(BackendError::OperationFailed(
                        "Playlist changed; selected entry no longer matches".to_owned(),
                    ));
                }
                let persistent_id = persistent_playlist_selector(&playlist_id)
                    .ok_or_else(|| BackendError::PlaylistNotFound(playlist_id.clone()))?;
                let expected = track_selector(&expected_track_id)?;
                let raw = self
                    .query(ScriptRequest::RemovePlaylistTrack {
                        playlist_persistent_id: persistent_id.to_owned(),
                        index,
                        expected,
                    })
                    .await
                    .map_err(|failure| BackendError::OperationFailed(failure_message(&failure)))?;
                self.apply_raw_playback(&raw);
                if let Some(playlist) = self
                    .snapshot
                    .playlists
                    .iter_mut()
                    .find(|playlist| playlist.id == playlist_id)
                {
                    playlist.tracks.remove(index);
                    playlist.track_count = playlist.track_count.saturating_sub(1);
                    playlist.contents_state = if playlist.tracks.is_empty() {
                        PlaylistLoadState::Empty
                    } else {
                        PlaylistLoadState::Loaded {
                            total: playlist.track_count,
                        }
                    };
                }
                self.reconcile_removed_playlist_entry(&playlist_id, index, &expected_track_id);
                return Ok(BackendUpdate::PlaylistTrackRemoved {
                    availability: self.snapshot.availability.clone(),
                    playback: self.snapshot.playback.clone(),
                    playlist_id,
                    index,
                    expected_track_id,
                });
            }
            BackendCommand::Next => {
                if let Some(session) = &self.playback_session {
                    let next = session.next_index(self.snapshot.playback.repeat);
                    if let Some(next) = next {
                        tracing::debug!(
                            next = %session.tracks[next].id,
                            source = "session",
                            "synthesized manual advance"
                        );
                        return Ok(match self.play_session_index(next).await {
                            Ok(()) => self.playback_update(),
                            Err(failure) => self.session_failure_update(
                                "Failed to move to the next collection track",
                                failure,
                            ),
                        });
                    }
                    if !session.source_is_complete() {
                        if let Some(session) = self.playback_session.as_mut() {
                            session.waiting_for_more = true;
                        }
                        self.sync_playback_context();
                        self.run_playback_command(ScriptRequest::Pause).await;
                        return Ok(BackendUpdate::Notice {
                            availability: self.snapshot.availability.clone(),
                            playback: self.snapshot.playback.clone(),
                            message: "Loading next playlist track…".to_owned(),
                        });
                    }
                    self.cancel_playback_session("Next reached the final collection track");
                    return Ok(self.run_playback_command(ScriptRequest::Pause).await);
                }
                ScriptRequest::Next
            }
            BackendCommand::Previous => {
                if let Some(session) = &self.playback_session {
                    let previous = if self.snapshot.playback.position > PREVIOUS_RESTART_THRESHOLD {
                        session.index
                    } else {
                        session.index.saturating_sub(1)
                    };
                    return Ok(match self.play_session_index(previous).await {
                        Ok(()) => self.playback_update(),
                        Err(failure) => self.session_failure_update(
                            "Failed to move to the previous collection track",
                            failure,
                        ),
                    });
                }
                ScriptRequest::Previous
            }
            BackendCommand::SeekBy(seconds) => ScriptRequest::SeekBy(seconds),
            BackendCommand::SetVolume(volume) => ScriptRequest::SetVolume(volume),
            BackendCommand::ToggleMute => {
                return Err(BackendError::Unsupported(Capability::Mute));
            }
            BackendCommand::ToggleShuffle => {
                let shuffle_before = self.snapshot.playback.shuffle;
                self.run_playback_command(ScriptRequest::ToggleShuffle)
                    .await;
                self.reconcile_session_shuffle();
                tracing::debug!(
                    session_active = self.playback_session.is_some(),
                    shuffle_before,
                    shuffle_after = self.snapshot.playback.shuffle,
                    "Music.app shuffle command reconciled"
                );
                return Ok(self.playback_update());
            }
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
        if let Some(message) = self.pending_context_error.take() {
            return Ok(Some(BackendUpdate::PlaybackContextFailed {
                availability: self.snapshot.availability.clone(),
                playback: self.snapshot.playback.clone(),
                message,
            }));
        }
        let waiting_for_playlist = self
            .playback_session
            .as_ref()
            .is_some_and(|session| session.waiting_for_more);
        let update = if waiting_for_playlist && let Some(load) = self.pending_playlist.clone() {
            self.load_playlist_batch(load).await
        } else if self.session_near_transition() {
            self.poll_playlist_transition().await
        } else if let Some(load) = self.pending_playlist.clone() {
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

fn failure_message(failure: &BackendFailure) -> String {
    match failure {
        BackendFailure::NotRunning => "Music.app is not running".to_owned(),
        BackendFailure::Unavailable => "Music.app is unavailable".to_owned(),
        BackendFailure::PermissionDenied => {
            "macOS denied Automation access to Music.app".to_owned()
        }
        BackendFailure::Error(message) => message.clone(),
    }
}

fn refresh_failure_message(failure: &BackendFailure) -> String {
    match failure {
        BackendFailure::NotRunning => "Music.app is not running".to_owned(),
        BackendFailure::Unavailable => "Music.app is unavailable".to_owned(),
        BackendFailure::PermissionDenied => "Automation access to Music.app was denied".to_owned(),
        BackendFailure::Error(_) => "Music.app could not refresh the library".to_owned(),
    }
}

fn playlist_shuffle_seed(playlist_id: &PlaylistId) -> u64 {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut hasher = DefaultHasher::new();
    playlist_id.hash(&mut hasher);
    timestamp.hash(&mut hasher);
    hasher.finish()
}

fn shuffled_track_rank(seed: u64, track: &SessionTrack) -> u64 {
    let mut hasher = DefaultHasher::new();
    seed.hash(&mut hasher);
    track.source_index.hash(&mut hasher);
    track.id.hash(&mut hasher);
    hasher.finish()
}

fn is_natural_completion(previous: &PlaybackSnapshot, current: &PlaybackSnapshot) -> bool {
    previous.status == PlaybackStatus::Playing
        && current.status == PlaybackStatus::Stopped
        && (snapshot_is_near_end(previous) || snapshot_is_near_end(current))
}

fn snapshot_is_near_end(snapshot: &PlaybackSnapshot) -> bool {
    snapshot.current_track.as_ref().is_some_and(|track| {
        !track.duration.is_zero()
            && snapshot.position.saturating_add(COMPLETION_TOLERANCE) >= track.duration
    })
}

fn track_selector(track_id: &TrackId) -> Result<TrackSelector, BackendError> {
    let Some((property, value)) = persistent_track_selector(track_id) else {
        return Err(BackendError::TrackNotFound(track_id.clone()));
    };
    Ok(if property == "persistentID" {
        TrackSelector::PersistentId(value.to_owned())
    } else {
        TrackSelector::DatabaseId(value.to_owned())
    })
}

fn raw_track_matches_selector(raw: Option<&RawTrack>, selector: &TrackSelector) -> bool {
    let Some(raw) = raw else {
        return false;
    };
    match selector {
        TrackSelector::PersistentId(expected) => raw
            .persistent_id
            .as_deref()
            .and_then(normalize_identifier)
            .is_some_and(|actual| actual == expected),
        TrackSelector::DatabaseId(expected) => raw
            .database_id
            .as_deref()
            .and_then(normalize_identifier)
            .is_some_and(|actual| actual == expected),
    }
}

fn artwork_result(raw: Option<RawArtwork>) -> ArtworkResult {
    let Some(raw) = raw else {
        return ArtworkResult::Missing;
    };
    if raw.too_large {
        return ArtworkResult::TooLarge {
            encoded_bytes: raw.encoded_bytes.unwrap_or_default(),
        };
    }
    if raw.transient {
        return ArtworkResult::Transient(
            raw.reason.unwrap_or_else(|| {
                "Music.app artwork object is temporarily unavailable".to_owned()
            }),
        );
    }
    if raw.missing {
        return ArtworkResult::Missing;
    }
    let Some(hex) = raw.raw_data else {
        return ArtworkResult::Invalid(
            "Music.app returned an unreadable artwork descriptor".to_owned(),
        );
    };
    if hex.len() % 2 != 0 || hex.len() / 2 > MAX_ARTWORK_BYTES {
        return ArtworkResult::Invalid("Music.app returned an invalid artwork size".to_owned());
    }
    let mut bytes = Vec::with_capacity(hex.len() / 2);
    for pair in hex.as_bytes().chunks_exact(2) {
        let Some(high) = hex_digit(pair[0]) else {
            return ArtworkResult::Invalid("Artwork descriptor was not hexadecimal".to_owned());
        };
        let Some(low) = hex_digit(pair[1]) else {
            return ArtworkResult::Invalid("Artwork descriptor was not hexadecimal".to_owned());
        };
        bytes.push((high << 4) | low);
    }
    let media_type = if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        ArtworkMediaType::Jpeg
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        ArtworkMediaType::Png
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        ArtworkMediaType::Gif
    } else {
        ArtworkMediaType::Unknown
    };
    ArtworkResult::Ready(Artwork { media_type, bytes })
}

const fn hex_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
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
            context: PlaybackContext::NoContext,
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
    use std::{
        collections::VecDeque,
        fs,
        sync::Mutex,
        time::{Instant, SystemTime},
    };

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

        fn new_owned(responses: impl IntoIterator<Item = (ScriptRequest, String)>) -> Self {
            Self {
                responses: Mutex::new(responses.into_iter().collect()),
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

    struct TransitionRecordingRunner {
        requests: Mutex<Vec<ScriptRequest>>,
        current_id: Mutex<String>,
        shuffle: Mutex<bool>,
    }

    impl TransitionRecordingRunner {
        fn new() -> Self {
            Self {
                requests: Mutex::new(Vec::new()),
                current_id: Mutex::new(String::new()),
                shuffle: Mutex::new(false),
            }
        }
    }

    impl AutomationRunner for TransitionRecordingRunner {
        fn is_installed(&self) -> bool {
            true
        }

        fn run(&self, request: ScriptRequest) -> Result<String, AutomationError> {
            self.requests
                .lock()
                .expect("request lock")
                .push(request.clone());
            let track = match &request {
                ScriptRequest::PlayPlaylistTrackOnce { track, .. }
                | ScriptRequest::PollPlaylistTransition { target: track, .. } => track,
                ScriptRequest::ToggleShuffle => {
                    let mut shuffle = self.shuffle.lock().expect("shuffle lock");
                    *shuffle = !*shuffle;
                    let current = self.current_id.lock().expect("current lock").clone();
                    return Ok(format!(
                        r#"{{"running":true,"state":"playing","position":0,"volume":50,"shuffle":{shuffle},"track":{{"persistentId":"{current}","name":"{current}","artist":"Artist","album":"Playlist","duration":60}}}}"#
                    ));
                }
                ScriptRequest::Poll => {
                    let current = self.current_id.lock().expect("current lock").clone();
                    let shuffle = *self.shuffle.lock().expect("shuffle lock");
                    return Ok(format!(
                        r#"{{"running":true,"state":"playing","position":1,"volume":50,"shuffle":{shuffle},"track":{{"persistentId":"{current}","name":"{current}","artist":"Artist","album":"Playlist","duration":60}}}}"#
                    ));
                }
                _ => panic!("unexpected transition test request: {request:?}"),
            };
            let id = match track {
                TrackSelector::PersistentId(id) | TrackSelector::DatabaseId(id) => id.clone(),
            };
            *self.current_id.lock().expect("current lock") = id.clone();
            let shuffle = *self.shuffle.lock().expect("shuffle lock");
            Ok(format!(
                r#"{{"running":true,"state":"playing","position":0,"volume":50,"shuffle":{shuffle},"sessionAdvanced":true,"track":{{"persistentId":"{id}","name":"{id}","artist":"Artist","album":"Playlist","duration":60}}}}"#
            ))
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
    async fn snapshot_hydrates_last_known_library_and_playlist_metadata_before_refresh() {
        let cache_path = std::env::temp_dir()
            .join(format!(
                "apple-music-tui-cache-hydration-{}",
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("clock")
                    .as_nanos()
            ))
            .join("cache.json");
        let mut track = Track::new(
            "musicapp:persistent:T1",
            "Cached",
            "Artist",
            "Album",
            Duration::from_secs(60),
        );
        track.metadata.origin = crate::domain::DataOrigin::LocalMusicApp;
        let mut playlist = crate::domain::Playlist::unloaded(
            "musicapp:playlist:persistent:P1",
            "Cached playlist",
            Some("Last known metadata".to_owned()),
            crate::domain::PlaylistKind::User,
            None,
        );
        playlist.origin = crate::domain::DataOrigin::LocalMusicApp;
        cache::store(&cache_path, &[track.clone()], &[playlist.clone()]).expect("cache");
        let runner: Arc<dyn AutomationRunner> = Arc::new(SequenceRunner::new([(
            ScriptRequest::FullState,
            r#"{"running":true,"state":"paused","position":1,"volume":50}"#,
        )]));
        let mut backend =
            MacOsMusicBackend::with_runner_and_cache(runner, true, Some(cache_path.clone()));

        let snapshot = backend.snapshot().await.expect("snapshot");
        assert_eq!(snapshot.library, vec![track]);
        assert_eq!(snapshot.playlists, vec![playlist]);
        assert_eq!(
            snapshot.library_status,
            CollectionLoadState::Cached { total: 1 }
        );
        assert!(backend.playback_session.is_none());
        let _ = fs::remove_dir_all(cache_path.parent().expect("cache parent"));
    }

    #[tokio::test]
    async fn manual_refresh_starts_once_and_keeps_existing_snapshot_until_batches_arrive() {
        let runner: Arc<dyn AutomationRunner> = Arc::new(SequenceRunner::new([]));
        let mut backend = MacOsMusicBackend::with_runner(runner, true);
        backend.snapshot.library.push(Track::new(
            "musicapp:persistent:cached",
            "Cached",
            "Artist",
            "Album",
            Duration::from_secs(1),
        ));
        backend.phase = LoadPhase::Ready;

        assert!(matches!(
            backend
                .execute(BackendCommand::RefreshLibrary)
                .await
                .expect("start refresh"),
            BackendUpdate::LibraryRefreshStarted { .. }
        ));
        assert_eq!(backend.snapshot.library.len(), 1);
        assert!(matches!(
            backend.snapshot.library_status,
            CollectionLoadState::Refreshing { .. }
        ));
        assert!(matches!(
            backend.execute(BackendCommand::RefreshLibrary).await.expect("coalesce refresh"),
            BackendUpdate::Notice { message, .. } if message == "Library refresh already in progress"
        ));
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

    #[tokio::test]
    async fn synthesized_album_playback_advances_exact_order_after_stopped_track() {
        let runner: Arc<dyn AutomationRunner> = Arc::new(SequenceRunner::new([
            (
                ScriptRequest::PlayTrackOnce(TrackSelector::PersistentId("A".to_owned())),
                r#"{"running":true,"state":"playing","position":0,"volume":50,"track":{"persistentId":"A","name":"One","artist":"Artist","album":"Album","duration":60}}"#,
            ),
            (
                ScriptRequest::Poll,
                r#"{"running":true,"state":"stopped","position":60,"volume":50,"track":{"persistentId":"A","name":"One","artist":"Artist","album":"Album","duration":60}}"#,
            ),
            (
                ScriptRequest::PlayTrackOnce(TrackSelector::PersistentId("B".to_owned())),
                r#"{"running":true,"state":"playing","position":0,"volume":50,"track":{"persistentId":"B","name":"Two","artist":"Artist","album":"Album","duration":70}}"#,
            ),
        ]));
        let mut backend = MacOsMusicBackend::with_runner(runner, true);

        backend
            .execute(BackendCommand::PlayAlbum {
                album_id: AlbumId::new("album"),
                track_ids: vec![
                    TrackId::new("musicapp:persistent:A"),
                    TrackId::new("musicapp:persistent:B"),
                ],
            })
            .await
            .expect("start album");
        let update = backend.poll().await;

        let BackendUpdate::Playback { playback, .. } = update else {
            panic!("playback update")
        };
        assert_eq!(playback.status, PlaybackStatus::Playing);
        assert_eq!(
            playback.current_track.expect("second track").id,
            TrackId::new("musicapp:persistent:B")
        );
        assert_eq!(backend.playback_session.expect("album session").index, 1);
    }

    #[tokio::test]
    async fn playlist_session_started_in_the_middle_advances_multiple_tracks_then_ends() {
        let runner: Arc<dyn AutomationRunner> = Arc::new(SequenceRunner::new([
            (
                ScriptRequest::PlayPlaylistTrackOnce {
                    playlist_persistent_id: "P".to_owned(),
                    track: TrackSelector::PersistentId("T2".to_owned()),
                },
                r#"{"running":true,"state":"playing","position":0,"volume":50,"repeat":"off","track":{"persistentId":"T2","name":"Same Title","artist":"Cloud Artist","album":"Playlist","duration":60}}"#,
            ),
            (
                ScriptRequest::Poll,
                r#"{"running":true,"state":"stopped","position":0,"volume":50,"repeat":"off"}"#,
            ),
            (
                ScriptRequest::PlayPlaylistTrackOnce {
                    playlist_persistent_id: "P".to_owned(),
                    track: TrackSelector::PersistentId("T3".to_owned()),
                },
                r#"{"running":true,"state":"playing","position":0,"volume":50,"repeat":"off","track":{"persistentId":"T3","name":"Same Title","artist":"Cloud Artist","album":"Playlist","duration":70}}"#,
            ),
            (
                ScriptRequest::Poll,
                r#"{"running":true,"state":"stopped","position":70,"volume":50,"repeat":"off","track":{"persistentId":"T3","name":"Same Title","artist":"Cloud Artist","album":"Playlist","duration":70}}"#,
            ),
            (
                ScriptRequest::PlayPlaylistTrackOnce {
                    playlist_persistent_id: "P".to_owned(),
                    track: TrackSelector::PersistentId("T4".to_owned()),
                },
                r#"{"running":true,"state":"playing","position":0,"volume":50,"repeat":"off","track":{"persistentId":"T4","name":"Final","artist":"Cloud Artist","album":"Playlist","duration":80}}"#,
            ),
            (
                ScriptRequest::Poll,
                r#"{"running":true,"state":"stopped","position":80,"volume":50,"repeat":"off","track":{"persistentId":"T4","name":"Final","artist":"Cloud Artist","album":"Playlist","duration":80}}"#,
            ),
        ]));
        let mut backend = MacOsMusicBackend::with_runner(runner, true);
        backend
            .execute(BackendCommand::PlayPlaylistTrack {
                playlist_id: PlaylistId::new("musicapp:playlist:persistent:P"),
                ordered_track_ids: ["T1", "T2", "T3", "T4"]
                    .into_iter()
                    .map(|id| TrackId::new(format!("musicapp:persistent:{id}")))
                    .collect(),
                selected_index: 1,
                complete: true,
            })
            .await
            .expect("start middle playlist track");
        assert!(matches!(
            backend.snapshot.playback.context,
            PlaybackContext::Playlist {
                current_index: 1,
                ..
            }
        ));

        backend.snapshot.playback.position = Duration::from_secs(59);
        backend.poll().await;
        assert_eq!(
            backend
                .snapshot
                .playback
                .current_track
                .as_ref()
                .expect("third track")
                .id,
            TrackId::new("musicapp:persistent:T3")
        );
        backend.poll().await;
        assert_eq!(
            backend
                .snapshot
                .playback
                .current_track
                .as_ref()
                .expect("fourth track")
                .id,
            TrackId::new("musicapp:persistent:T4")
        );
        backend.poll().await;
        assert_eq!(backend.snapshot.playback.status, PlaybackStatus::Stopped);
        assert_eq!(
            backend.snapshot.playback.context,
            PlaybackContext::NoContext
        );
    }

    #[tokio::test]
    async fn playlist_session_honors_repeat_all_and_repeat_one_at_the_end() {
        for (repeat, expected) in [("all", "T1"), ("one", "T2")] {
            let start = format!(
                r#"{{"running":true,"state":"playing","position":0,"volume":50,"repeat":"{repeat}","track":{{"persistentId":"T2","name":"Two","artist":"Artist","album":"Playlist","duration":60}}}}"#
            );
            let stopped = format!(
                r#"{{"running":true,"state":"stopped","position":60,"volume":50,"repeat":"{repeat}","track":{{"persistentId":"T2","name":"Two","artist":"Artist","album":"Playlist","duration":60}}}}"#
            );
            let continued = format!(
                r#"{{"running":true,"state":"playing","position":0,"volume":50,"repeat":"{repeat}","track":{{"persistentId":"{expected}","name":"Continued","artist":"Artist","album":"Playlist","duration":60}}}}"#
            );
            let runner: Arc<dyn AutomationRunner> = Arc::new(SequenceRunner::new_owned([
                (
                    ScriptRequest::PlayPlaylistTrackOnce {
                        playlist_persistent_id: "P".to_owned(),
                        track: TrackSelector::PersistentId("T2".to_owned()),
                    },
                    start,
                ),
                (ScriptRequest::Poll, stopped),
                (
                    ScriptRequest::PlayPlaylistTrackOnce {
                        playlist_persistent_id: "P".to_owned(),
                        track: TrackSelector::PersistentId(expected.to_owned()),
                    },
                    continued,
                ),
            ]));
            let mut backend = MacOsMusicBackend::with_runner(runner, true);
            backend
                .execute(BackendCommand::PlayPlaylistTrack {
                    playlist_id: PlaylistId::new("musicapp:playlist:persistent:P"),
                    ordered_track_ids: vec![
                        TrackId::new("musicapp:persistent:T1"),
                        TrackId::new("musicapp:persistent:T2"),
                    ],
                    selected_index: 1,
                    complete: true,
                })
                .await
                .expect("start final track");
            backend.poll().await;
            assert_eq!(
                backend
                    .snapshot
                    .playback
                    .current_track
                    .as_ref()
                    .expect("repeated track")
                    .id,
                TrackId::new(format!("musicapp:persistent:{expected}"))
            );
        }
    }

    #[tokio::test]
    async fn playlist_next_previous_and_expected_transition_keep_session_synchronized() {
        let runner: Arc<dyn AutomationRunner> = Arc::new(SequenceRunner::new([
            (
                ScriptRequest::PlayPlaylistTrackOnce {
                    playlist_persistent_id: "P".to_owned(),
                    track: TrackSelector::PersistentId("T2".to_owned()),
                },
                r#"{"running":true,"state":"playing","position":0,"volume":50,"track":{"persistentId":"T2","name":"Two","artist":"Artist","album":"Playlist","duration":60}}"#,
            ),
            (
                ScriptRequest::PlayPlaylistTrackOnce {
                    playlist_persistent_id: "P".to_owned(),
                    track: TrackSelector::PersistentId("T3".to_owned()),
                },
                r#"{"running":true,"state":"playing","position":0,"volume":50,"track":{"persistentId":"T2","name":"Two","artist":"Artist","album":"Playlist","duration":60}}"#,
            ),
            (
                ScriptRequest::Poll,
                r#"{"running":true,"state":"playing","position":1,"volume":50,"track":{"persistentId":"T3","name":"Three","artist":"Artist","album":"Playlist","duration":60}}"#,
            ),
            (
                ScriptRequest::PlayPlaylistTrackOnce {
                    playlist_persistent_id: "P".to_owned(),
                    track: TrackSelector::PersistentId("T2".to_owned()),
                },
                r#"{"running":true,"state":"playing","position":0,"volume":50,"track":{"persistentId":"T2","name":"Two","artist":"Artist","album":"Playlist","duration":60}}"#,
            ),
        ]));
        let mut backend = MacOsMusicBackend::with_runner(runner, true);
        backend
            .execute(BackendCommand::PlayPlaylistTrack {
                playlist_id: PlaylistId::new("musicapp:playlist:persistent:P"),
                ordered_track_ids: ["T1", "T2", "T3"]
                    .into_iter()
                    .map(|id| TrackId::new(format!("musicapp:persistent:{id}")))
                    .collect(),
                selected_index: 1,
                complete: true,
            })
            .await
            .expect("start playlist");
        backend
            .execute(BackendCommand::Next)
            .await
            .expect("next playlist track");
        assert!(
            backend
                .playback_session
                .as_ref()
                .expect("active transition")
                .transition
                .is_some()
        );
        backend.poll().await;
        assert_eq!(backend.playback_session.as_ref().expect("session").index, 2);
        assert!(
            backend
                .playback_session
                .as_ref()
                .expect("session")
                .transition
                .is_none()
        );
        backend
            .execute(BackendCommand::Previous)
            .await
            .expect("previous playlist track");
        assert_eq!(backend.playback_session.as_ref().expect("session").index, 1);
    }

    #[tokio::test]
    async fn near_end_tick_checks_and_starts_next_playlist_track_in_one_query() {
        let runner: Arc<dyn AutomationRunner> = Arc::new(SequenceRunner::new([
            (
                ScriptRequest::PlayPlaylistTrackOnce {
                    playlist_persistent_id: "P".to_owned(),
                    track: TrackSelector::PersistentId("T1".to_owned()),
                },
                r#"{"running":true,"state":"playing","position":0,"volume":50,"track":{"persistentId":"T1","name":"One","artist":"Artist","album":"Playlist","duration":60}}"#,
            ),
            (
                ScriptRequest::PollPlaylistTransition {
                    playlist_persistent_id: "P".to_owned(),
                    expected: TrackSelector::PersistentId("T1".to_owned()),
                    target: TrackSelector::PersistentId("T2".to_owned()),
                    max_wait_ms: 2_000,
                },
                r#"{"running":true,"state":"playing","position":0,"volume":50,"sessionAdvanced":true,"track":{"persistentId":"T2","name":"Two","artist":"Artist","album":"Playlist","duration":60}}"#,
            ),
        ]));
        let mut backend = MacOsMusicBackend::with_runner(runner, true);
        backend
            .execute(BackendCommand::PlayPlaylistTrack {
                playlist_id: PlaylistId::new("musicapp:playlist:persistent:P"),
                ordered_track_ids: vec![
                    TrackId::new("musicapp:persistent:T1"),
                    TrackId::new("musicapp:persistent:T2"),
                ],
                selected_index: 0,
                complete: true,
            })
            .await
            .expect("start playlist");
        backend.snapshot.playback.position = Duration::from_secs(59);
        let update = backend
            .tick(Duration::ZERO)
            .await
            .expect("near-end tick")
            .expect("transition update");

        let BackendUpdate::Playback { playback, .. } = update else {
            panic!("playback transition")
        };
        assert_eq!(
            playback.current_track.expect("next track").id,
            TrackId::new("musicapp:persistent:T2")
        );
        assert!(matches!(
            playback.context,
            PlaybackContext::Playlist {
                current_index: 1,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn natural_playlist_transition_uses_the_reconciled_shuffle_future() {
        let runner = Arc::new(TransitionRecordingRunner::new());
        let runner_for_backend: Arc<dyn AutomationRunner> = runner.clone();
        let mut backend = MacOsMusicBackend::with_runner(runner_for_backend, true);
        let playlist_id = PlaylistId::new("musicapp:playlist:persistent:P");
        backend
            .execute(BackendCommand::PlayPlaylistTrack {
                playlist_id,
                ordered_track_ids: ["A", "B", "C", "D", "E"]
                    .into_iter()
                    .map(|id| TrackId::new(format!("musicapp:persistent:{id}")))
                    .collect(),
                selected_index: 1,
                complete: true,
            })
            .await
            .expect("start playlist at B");

        backend.snapshot.playback.shuffle = true;
        backend.reconcile_session_shuffle();
        let expected = {
            let session = backend.playback_session.as_ref().expect("shuffled session");
            session.tracks[session.index + 1].selector.clone()
        };

        backend.snapshot.playback.position = Duration::from_secs(59);
        backend
            .tick(Duration::ZERO)
            .await
            .expect("near-end transition")
            .expect("transition update");

        let requests = runner.requests.lock().expect("request lock");
        let transition = requests
            .iter()
            .find_map(|request| match request {
                ScriptRequest::PollPlaylistTransition { target, .. } => Some(target),
                _ => None,
            })
            .expect("natural transition request");
        assert_eq!(transition, &expected);
        assert_eq!(
            backend
                .snapshot
                .playback
                .current_track
                .as_ref()
                .expect("shuffled successor")
                .id,
            TrackId::new(match expected {
                TrackSelector::PersistentId(id) => format!("musicapp:persistent:{id}"),
                TrackSelector::DatabaseId(id) => format!("musicapp:database:{id}"),
            })
        );
    }

    #[tokio::test]
    async fn confirmed_shuffle_toggle_commits_future_for_manual_next_and_polls() {
        let runner = Arc::new(TransitionRecordingRunner::new());
        let runner_for_backend: Arc<dyn AutomationRunner> = runner.clone();
        let mut backend = MacOsMusicBackend::with_runner(runner_for_backend, true);
        backend
            .execute(BackendCommand::PlayPlaylistTrack {
                playlist_id: PlaylistId::new("musicapp:playlist:persistent:P"),
                ordered_track_ids: ["A", "B", "C", "D", "E", "F"]
                    .into_iter()
                    .map(|id| TrackId::new(format!("musicapp:persistent:{id}")))
                    .collect(),
                selected_index: 1,
                complete: true,
            })
            .await
            .expect("start B");
        let current_before = backend.snapshot.playback.current_track.clone();

        backend
            .execute(BackendCommand::ToggleShuffle)
            .await
            .expect("toggle shuffle");
        let (future_after_toggle, next_index) = {
            let session = backend.playback_session.as_ref().expect("active session");
            assert!(session.shuffle_enabled);
            (
                session.tracks[session.index + 1..]
                    .iter()
                    .map(|track| track.id.clone())
                    .collect::<Vec<_>>(),
                session
                    .next_index(backend.snapshot.playback.repeat)
                    .expect("shuffled successor"),
            )
        };
        assert_ne!(
            future_after_toggle,
            ["C", "D", "E", "F"]
                .into_iter()
                .map(|id| TrackId::new(format!("musicapp:persistent:{id}")))
                .collect::<Vec<_>>()
        );
        assert_eq!(backend.snapshot.playback.current_track, current_before);

        backend.poll().await;
        assert_eq!(
            backend
                .playback_session
                .as_ref()
                .expect("session survives poll")
                .tracks
                .iter()
                .skip(2)
                .map(|track| track.id.clone())
                .collect::<Vec<_>>(),
            future_after_toggle
        );

        backend
            .execute(BackendCommand::Next)
            .await
            .expect("manual session next");
        let requests = runner.requests.lock().expect("request lock");
        assert!(
            !requests
                .iter()
                .any(|request| matches!(request, ScriptRequest::Next)),
            "an active synthesized session must not issue native nextTrack"
        );
        assert!(requests.iter().any(|request| {
            matches!(request, ScriptRequest::PlayPlaylistTrackOnce { track, .. }
                if *track == backend.playback_session.as_ref().expect("session").tracks[next_index].selector)
        }));
    }

    #[tokio::test]
    async fn disabling_shuffle_keeps_consumed_history_and_restores_canonical_future() {
        let runner = Arc::new(TransitionRecordingRunner::new());
        let runner_for_backend: Arc<dyn AutomationRunner> = runner;
        let mut backend = MacOsMusicBackend::with_runner(runner_for_backend, true);
        backend
            .execute(BackendCommand::PlayPlaylistTrack {
                playlist_id: PlaylistId::new("musicapp:playlist:persistent:P"),
                ordered_track_ids: ["A", "B", "C", "D", "E", "F"]
                    .into_iter()
                    .map(|id| TrackId::new(format!("musicapp:persistent:{id}")))
                    .collect(),
                selected_index: 1,
                complete: true,
            })
            .await
            .expect("start B");
        backend
            .execute(BackendCommand::ToggleShuffle)
            .await
            .expect("enable shuffle");
        backend
            .execute(BackendCommand::Next)
            .await
            .expect("consume one shuffled track");
        let consumed = backend.playback_session.as_ref().expect("session").tracks[..=2]
            .iter()
            .map(|track| track.id.clone())
            .collect::<Vec<_>>();

        backend
            .execute(BackendCommand::ToggleShuffle)
            .await
            .expect("disable shuffle");
        let session = backend
            .playback_session
            .as_ref()
            .expect("session remains active");
        assert!(!session.shuffle_enabled);
        assert_eq!(
            session.tracks[..=session.index]
                .iter()
                .map(|track| track.id.clone())
                .collect::<Vec<_>>(),
            consumed
        );
        assert!(
            session.tracks[session.index + 1..]
                .windows(2)
                .all(|pair| pair[0].source_index < pair[1].source_index),
            "only the remaining future returns to canonical source order"
        );
        assert!(
            session.tracks[session.index + 1..]
                .iter()
                .all(|track| !consumed.contains(&track.id))
        );
    }

    #[tokio::test]
    async fn external_music_app_track_change_cancels_playlist_context() {
        let runner: Arc<dyn AutomationRunner> = Arc::new(SequenceRunner::new([
            (
                ScriptRequest::PlayPlaylistTrackOnce {
                    playlist_persistent_id: "P".to_owned(),
                    track: TrackSelector::PersistentId("T1".to_owned()),
                },
                r#"{"running":true,"state":"playing","position":0,"volume":50,"track":{"persistentId":"T1","name":"One","artist":"Artist","album":"Playlist","duration":60}}"#,
            ),
            (
                ScriptRequest::Poll,
                r#"{"running":true,"state":"playing","position":5,"volume":50,"track":{"persistentId":"OTHER","name":"External","artist":"Someone","album":"Elsewhere","duration":100}}"#,
            ),
        ]));
        let mut backend = MacOsMusicBackend::with_runner(runner, true);
        backend
            .execute(BackendCommand::PlayPlaylistTrack {
                playlist_id: PlaylistId::new("musicapp:playlist:persistent:P"),
                ordered_track_ids: vec![TrackId::new("musicapp:persistent:T1")],
                selected_index: 0,
                complete: true,
            })
            .await
            .expect("start playlist");
        backend.poll().await;

        assert_eq!(
            backend.snapshot.playback.context,
            PlaybackContext::NoContext
        );
        assert_eq!(
            backend
                .snapshot
                .playback
                .current_track
                .expect("external track")
                .id,
            TrackId::new("musicapp:persistent:OTHER")
        );
    }

    #[tokio::test]
    async fn music_app_track_change_within_session_reconciles_playlist_position() {
        let runner: Arc<dyn AutomationRunner> = Arc::new(SequenceRunner::new([
            (
                ScriptRequest::PlayPlaylistTrackOnce {
                    playlist_persistent_id: "P".to_owned(),
                    track: TrackSelector::PersistentId("T2".to_owned()),
                },
                r#"{"running":true,"state":"playing","position":0,"volume":50,"track":{"persistentId":"T2","name":"Two","artist":"Artist","album":"Playlist","duration":60}}"#,
            ),
            (
                ScriptRequest::Poll,
                r#"{"running":true,"state":"playing","position":12,"volume":50,"track":{"persistentId":"T4","name":"Four","artist":"Artist","album":"Playlist","duration":60}}"#,
            ),
        ]));
        let mut backend = MacOsMusicBackend::with_runner(runner, true);
        backend
            .execute(BackendCommand::PlayPlaylistTrack {
                playlist_id: PlaylistId::new("musicapp:playlist:persistent:P"),
                ordered_track_ids: ["T1", "T2", "T3", "T4"]
                    .into_iter()
                    .map(|id| TrackId::new(format!("musicapp:persistent:{id}")))
                    .collect(),
                selected_index: 1,
                complete: true,
            })
            .await
            .expect("start middle playlist track");
        backend.poll().await;

        assert!(matches!(
            backend.snapshot.playback.context,
            PlaybackContext::Playlist {
                current_index: 3,
                ..
            }
        ));
    }

    #[test]
    fn session_position_reconciliation_keeps_shuffled_order_intact() {
        let runner: Arc<dyn AutomationRunner> = Arc::new(SequenceRunner::new([]));
        let mut backend = MacOsMusicBackend::with_runner(runner, true);
        let playlist_id = PlaylistId::new("musicapp:playlist:persistent:P");
        backend.snapshot.playback.shuffle = true;
        backend
            .create_playlist_session(
                playlist_id,
                ["A", "B", "C", "D", "E"]
                    .into_iter()
                    .map(|id| TrackId::new(format!("musicapp:persistent:{id}")))
                    .collect(),
                0,
                true,
            )
            .expect("create shuffled session");
        let ordered_before = backend
            .playback_session
            .as_ref()
            .expect("session")
            .tracks
            .iter()
            .map(|track| track.id.clone())
            .collect::<Vec<_>>();
        let target = ordered_before[3].clone();
        let raw = parse_output(&format!(
            r#"{{"running":true,"state":"playing","position":10,"volume":50,"shuffle":true,"track":{{"persistentId":"{}","name":"Target","artist":"Artist","album":"Playlist","duration":60}}}}"#,
            target.as_str().trim_start_matches("musicapp:persistent:")
        ))
        .expect("Music.app state");
        backend.apply_raw_playback(&raw);

        let session = backend.playback_session.as_ref().expect("session");
        assert_eq!(
            session
                .tracks
                .iter()
                .map(|track| track.id.clone())
                .collect::<Vec<_>>(),
            ordered_before
        );
        assert_eq!(session.index, 3);
        assert!(session.shuffle_enabled);
    }

    #[tokio::test]
    async fn partial_playlist_session_waits_for_and_uses_later_loaded_track() {
        let runner: Arc<dyn AutomationRunner> = Arc::new(SequenceRunner::new([
            (
                ScriptRequest::PlayPlaylistTrackOnce {
                    playlist_persistent_id: "P".to_owned(),
                    track: TrackSelector::PersistentId("T1".to_owned()),
                },
                r#"{"running":true,"state":"playing","position":0,"volume":50,"track":{"persistentId":"T1","name":"Cloud One","artist":"Artist","album":"Playlist","duration":60,"cloudStatus":"subscription"}}"#,
            ),
            (
                ScriptRequest::Poll,
                r#"{"running":true,"state":"stopped","position":60,"volume":50,"track":{"persistentId":"T1","name":"Cloud One","artist":"Artist","album":"Playlist","duration":60,"cloudStatus":"subscription"}}"#,
            ),
            (
                ScriptRequest::PlayPlaylistTrackOnce {
                    playlist_persistent_id: "P".to_owned(),
                    track: TrackSelector::PersistentId("T2".to_owned()),
                },
                r#"{"running":true,"state":"playing","position":0,"volume":50,"track":{"persistentId":"T2","name":"Cloud Two","artist":"Artist","album":"Playlist","duration":60,"cloudStatus":"subscription"}}"#,
            ),
        ]));
        let playlist_id = PlaylistId::new("musicapp:playlist:persistent:P");
        let mut backend = MacOsMusicBackend::with_runner(runner, true);
        backend.snapshot.playlists = vec![crate::domain::Playlist::unloaded(
            playlist_id.to_string(),
            "Nested Cloud Playlist",
            None,
            crate::domain::PlaylistKind::Subscription,
            Some(PlaylistId::new("musicapp:playlist:persistent:FOLDER")),
        )];
        backend
            .execute(BackendCommand::PlayPlaylistTrack {
                playlist_id: playlist_id.clone(),
                ordered_track_ids: vec![TrackId::new("musicapp:persistent:T1")],
                selected_index: 0,
                complete: false,
            })
            .await
            .expect("start partially loaded playlist");
        backend.poll().await;
        assert!(
            backend
                .playback_session
                .as_ref()
                .expect("waiting session")
                .waiting_for_more
        );

        let playlist = &mut backend.snapshot.playlists[0];
        playlist.tracks = vec![
            Track::new(
                "musicapp:persistent:T1",
                "Cloud One",
                "Artist",
                "Playlist",
                Duration::from_secs(60),
            ),
            Track::new(
                "musicapp:persistent:T2",
                "Cloud Two",
                "Artist",
                "Playlist",
                Duration::from_secs(60),
            ),
        ];
        playlist.track_count = 2;
        playlist.contents_state = PlaylistLoadState::Loaded { total: 2 };
        backend.extend_playlist_session(&playlist_id);
        backend
            .resume_waiting_session()
            .await
            .expect("resume when the next batch becomes available");

        assert_eq!(
            backend
                .snapshot
                .playback
                .current_track
                .as_ref()
                .expect("second cloud track")
                .id,
            TrackId::new("musicapp:persistent:T2")
        );
        assert!(matches!(
            backend.snapshot.playback.context,
            PlaybackContext::Playlist {
                current_index: 1,
                complete: true,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn foreground_playlist_batches_preempt_background_library_scan() {
        let runner: Arc<dyn AutomationRunner> = Arc::new(SequenceRunner::new([
            (
                ScriptRequest::FullState,
                r#"{"running":true,"state":"paused","position":0,"volume":50}"#,
            ),
            (
                ScriptRequest::DiscoverPlaylists,
                r#"{"running":true,"state":"paused","position":0,"volume":50,"playlists":[{"persistentId":"P","name":"Foreground","kind":"userPlaylist","smart":false}]}"#,
            ),
            (
                ScriptRequest::PlaylistBatch {
                    playlist_persistent_id: "P".to_owned(),
                    start: 0,
                    limit: PLAYLIST_INITIAL_BATCH_SIZE,
                    total: None,
                },
                r#"{"running":true,"state":"paused","position":0,"volume":50,"playlistBatch":{"playlistPersistentId":"P","start":0,"total":2,"tracks":[{"persistentId":"T1","name":"One","artist":"Artist","album":"Playlist","duration":60}]}}"#,
            ),
            (
                ScriptRequest::PlaylistBatch {
                    playlist_persistent_id: "P".to_owned(),
                    start: 1,
                    limit: PLAYLIST_CONTINUATION_BATCH_SIZE,
                    total: Some(2),
                },
                r#"{"running":true,"state":"paused","position":0,"volume":50,"playlistBatch":{"playlistPersistentId":"P","start":1,"total":2,"tracks":[{"persistentId":"T2","name":"Two","artist":"Artist","album":"Playlist","duration":60}]}}"#,
            ),
        ]));
        let mut backend = MacOsMusicBackend::with_runner(runner, true);
        backend.snapshot().await.expect("initial state");
        backend
            .tick(Duration::ZERO)
            .await
            .expect("playlist discovery")
            .expect("playlist event");
        let playlist_id = backend.snapshot.playlists[0].id.clone();
        let first = backend
            .execute(BackendCommand::LoadPlaylist(playlist_id.clone()))
            .await
            .expect("foreground first batch");
        assert!(matches!(
            first,
            BackendUpdate::PlaylistBatch {
                loaded: 1,
                complete: false,
                ..
            }
        ));
        let second = backend
            .tick(Duration::ZERO)
            .await
            .expect("foreground continuation")
            .expect("playlist continuation event");
        assert!(matches!(
            second,
            BackendUpdate::PlaylistBatch {
                loaded: 2,
                complete: true,
                ..
            }
        ));
        assert_eq!(
            backend.snapshot.library_status,
            CollectionLoadState::Loading {
                loaded: 0,
                total: 0
            }
        );
        assert_eq!(
            backend.snapshot.playlists[0]
                .tracks
                .iter()
                .map(|track| track.id.clone())
                .collect::<Vec<_>>(),
            vec![
                TrackId::new("musicapp:persistent:T1"),
                TrackId::new("musicapp:persistent:T2")
            ]
        );
    }

    #[test]
    fn selected_playlist_track_initializes_its_real_session_position() {
        let runner: Arc<dyn AutomationRunner> = Arc::new(SequenceRunner::new([]));
        let playlist_id = PlaylistId::new("musicapp:playlist:persistent:SHUFFLE");
        let expected_ids = ["T1", "T2", "T3", "T4"]
            .into_iter()
            .map(|id| TrackId::new(format!("musicapp:persistent:{id}")))
            .collect::<Vec<_>>();
        let mut backend = MacOsMusicBackend::with_runner(runner, true);
        backend.snapshot.playback.shuffle = true;
        backend
            .create_playlist_session(playlist_id.clone(), expected_ids.clone(), 2, true)
            .expect("shuffle session");

        let PlaybackContext::Playlist {
            playlist_id: actual_playlist_id,
            ordered_track_ids,
            current_index,
            complete,
            ..
        } = backend.snapshot.playback.context.clone()
        else {
            panic!("playlist context")
        };
        assert_eq!(actual_playlist_id, playlist_id);
        assert_eq!(current_index, 2);
        assert!(complete);
        assert_eq!(ordered_track_ids[2], expected_ids[2]);
        let mut actual_set = ordered_track_ids;
        actual_set.sort();
        let mut expected_set = expected_ids;
        expected_set.sort();
        assert_eq!(actual_set, expected_set);
    }

    #[test]
    fn playlist_session_initializes_first_and_sixth_selected_tracks() {
        let ids = (1..=6)
            .map(|index| TrackId::new(format!("musicapp:persistent:T{index}")))
            .collect::<Vec<_>>();
        for (selected_index, expected_index) in [(0, 0), (5, 5)] {
            let runner: Arc<dyn AutomationRunner> = Arc::new(SequenceRunner::new([]));
            let mut backend = MacOsMusicBackend::with_runner(runner, true);
            backend
                .create_playlist_session(
                    PlaylistId::new("musicapp:playlist:persistent:P"),
                    ids.clone(),
                    selected_index,
                    true,
                )
                .expect("create playlist session");
            assert_eq!(
                backend.playback_session.as_ref().expect("session").index,
                expected_index
            );
        }
    }

    #[test]
    fn shuffled_playlist_session_keeps_history_current_and_future_for_selected_sixth_track() {
        let runner: Arc<dyn AutomationRunner> = Arc::new(SequenceRunner::new([]));
        let mut backend = MacOsMusicBackend::with_runner(runner, true);
        backend.snapshot.playback.shuffle = true;
        backend
            .create_playlist_session(
                PlaylistId::new("musicapp:playlist:persistent:P"),
                (1..=10)
                    .map(|index| TrackId::new(format!("musicapp:persistent:T{index}")))
                    .collect(),
                5,
                true,
            )
            .expect("create shuffled playlist session");
        let session = backend.playback_session.as_ref().expect("session");
        assert_eq!(session.index, 5);
        assert_eq!(
            session.tracks[session.index].id,
            TrackId::new("musicapp:persistent:T6")
        );
        assert_eq!(
            session.tracks[..session.index]
                .iter()
                .map(|track| track.source_index)
                .collect::<Vec<_>>(),
            vec![0, 1, 2, 3, 4]
        );
        assert!(
            session.tracks[session.index + 1..]
                .iter()
                .all(|track| track.source_index > 5)
        );
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

    #[test]
    fn artwork_descriptor_is_bounded_decoded_and_typed() {
        assert_eq!(
            artwork_result(Some(RawArtwork {
                raw_data: Some("FFD8FFD9".to_owned()),
                missing: false,
                too_large: false,
                encoded_bytes: Some(4),
                resolver: Some("current_track".to_owned()),
                attempts: vec!["current_track:matched".to_owned()],
                transient: false,
                reason: None,
            })),
            ArtworkResult::Ready(Artwork {
                media_type: ArtworkMediaType::Jpeg,
                bytes: vec![0xff, 0xd8, 0xff, 0xd9],
            })
        );
        assert!(matches!(
            artwork_result(Some(RawArtwork {
                raw_data: None,
                missing: false,
                too_large: true,
                encoded_bytes: Some(MAX_ARTWORK_BYTES + 1),
                resolver: Some("current_track".to_owned()),
                attempts: vec!["current_track:matched".to_owned()],
                transient: false,
                reason: None,
            })),
            ArtworkResult::TooLarge { .. }
        ));
        assert!(matches!(
            artwork_result(Some(RawArtwork {
                raw_data: None,
                missing: false,
                too_large: false,
                encoded_bytes: None,
                resolver: None,
                attempts: vec!["current_track:identity_mismatch".to_owned()],
                transient: true,
                reason: Some("fresh Music.app track has not materialized".to_owned()),
            })),
            ArtworkResult::Transient(message) if message.contains("has not materialized")
        ));
    }

    #[tokio::test]
    async fn artwork_failure_preserves_last_authoritative_playback_snapshot() {
        let track_id = TrackId::new("musicapp:persistent:ART");
        let runner: Arc<dyn AutomationRunner> = Arc::new(SequenceRunner::new([(
            ScriptRequest::LoadTrackArtwork {
                track: TrackSelector::PersistentId("ART".to_owned()),
                max_bytes: MAX_ARTWORK_BYTES,
            },
            r#"{"running":true,"error":{"number":-1728,"message":"Artwork track disappeared"}}"#,
        )]));
        let mut backend = MacOsMusicBackend::with_runner(runner, true);
        backend.snapshot.playback = PlaybackSnapshot {
            status: PlaybackStatus::Playing,
            current_track: Some(Track::new(
                track_id.to_string(),
                "Still Playing",
                "Artist",
                "Album",
                Duration::from_secs(60),
            )),
            ..PlaybackSnapshot::default()
        };

        let update = backend
            .execute(BackendCommand::LoadTrackArtwork {
                key: ArtworkKey::Track(track_id.clone()),
                track_id,
            })
            .await
            .expect("artwork failure remains a typed update");

        let BackendUpdate::Artwork {
            playback, result, ..
        } = update
        else {
            panic!("artwork update")
        };
        assert_eq!(playback.status, PlaybackStatus::Playing);
        assert_eq!(
            playback.current_track.expect("cached playback track").title,
            "Still Playing"
        );
        assert!(matches!(result, ArtworkResult::Invalid(_)));
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
    #[ignore = "requires a running local Music.app, real artwork, and Automation consent"]
    async fn live_loads_current_track_artwork_lazily() {
        let mut backend = MacOsMusicBackend::new();
        let snapshot = backend.snapshot().await.expect("initial Music.app state");
        let track_id = if let Some(track) = snapshot.playback.current_track {
            track.id
        } else {
            backend
                .tick(Duration::ZERO)
                .await
                .expect("playlist discovery")
                .expect("playlist discovery update");
            let update = backend
                .tick(Duration::ZERO)
                .await
                .expect("library batch")
                .expect("library batch update");
            let BackendUpdate::LibraryBatch { tracks, .. } = update else {
                panic!("library batch for artwork fallback")
            };
            tracks
                .into_iter()
                .find(|track| track_selector(&track.id).is_ok())
                .expect("stable library track for artwork test")
                .id
        };
        let update = backend
            .execute(BackendCommand::LoadTrackArtwork {
                key: ArtworkKey::Track(track_id.clone()),
                track_id,
            })
            .await
            .expect("artwork query");
        let BackendUpdate::Artwork { result, .. } = update else {
            panic!("artwork update")
        };
        match result {
            ArtworkResult::Ready(artwork) => {
                assert!(!artwork.bytes.is_empty());
                assert!(artwork.bytes.len() <= MAX_ARTWORK_BYTES);
                if artwork.media_type == ArtworkMediaType::Jpeg {
                    let started = std::time::Instant::now();
                    let renderable = crate::ui::artwork::prepare_kitty_renderable(&artwork)
                        .expect("real JPEG artwork converts for Kitty");
                    eprintln!(
                        "Kitty artwork conversion: source={} bytes, result={} bytes, elapsed={:.2} ms",
                        artwork.bytes.len(),
                        renderable.bytes.len(),
                        started.elapsed().as_secs_f64() * 1_000.0,
                    );
                    assert_eq!(renderable.media_type, ArtworkMediaType::Png);
                }
            }
            ArtworkResult::Missing => {}
            other => panic!("unexpected artwork result: {other:?}"),
        }
    }

    #[tokio::test]
    #[ignore = "changes Music.app playback briefly and requires Automation consent"]
    async fn live_plays_two_album_tracks_in_exact_derived_order_then_restores_playback() {
        let mut backend = MacOsMusicBackend::new();
        backend.snapshot().await.expect("initial Music.app state");
        let original_status = backend.snapshot.playback.status;
        let original_position = backend.snapshot.playback.position.as_secs() as i64;
        let original_track = backend
            .snapshot
            .playback
            .current_track
            .as_ref()
            .map(|track| track.id.clone());

        backend
            .tick(Duration::ZERO)
            .await
            .expect("playlist discovery")
            .expect("playlist update");
        let update = backend
            .tick(Duration::ZERO)
            .await
            .expect("library batch")
            .expect("library update");
        let BackendUpdate::LibraryBatch { tracks, .. } = update else {
            panic!("library batch update")
        };
        let album = derive_library(&tracks)
            .albums
            .into_iter()
            .find(|album| {
                album.tracks.len() >= 2
                    && album
                        .tracks
                        .iter()
                        .all(|track| persistent_track_selector(&track.id).is_some())
            })
            .expect("an album with at least two stable local tracks in the first batch");
        let expected = album
            .tracks
            .iter()
            .take(2)
            .map(|track| track.id.clone())
            .collect::<Vec<_>>();

        backend
            .execute(BackendCommand::PlayAlbum {
                album_id: album.id,
                track_ids: album.tracks.iter().map(|track| track.id.clone()).collect(),
            })
            .await
            .expect("play derived album");
        tokio::time::sleep(Duration::from_millis(750)).await;
        let BackendUpdate::Playback {
            playback: first, ..
        } = backend.poll().await
        else {
            panic!("first album playback update")
        };

        backend
            .execute(BackendCommand::Next)
            .await
            .expect("play next album track");
        tokio::time::sleep(Duration::from_millis(750)).await;
        let BackendUpdate::Playback {
            playback: second, ..
        } = backend.poll().await
        else {
            panic!("second album playback update")
        };

        if let Some(original_track) = original_track {
            backend
                .execute(BackendCommand::PlayTrack(original_track))
                .await
                .expect("restore original track");
            if original_position > 0 {
                backend
                    .execute(BackendCommand::SeekBy(original_position))
                    .await
                    .expect("restore original position");
            }
            if original_status != PlaybackStatus::Playing {
                backend
                    .execute(BackendCommand::Pause)
                    .await
                    .expect("restore paused playback");
            }
        }

        assert_eq!(
            first.current_track.expect("first album track").id,
            expected[0]
        );
        assert_eq!(
            second.current_track.expect("second album track").id,
            expected[1]
        );
    }

    #[tokio::test]
    #[ignore = "requires real nested Music.app playlists and Automation consent"]
    async fn live_preserves_real_playlist_folder_relationships() {
        let mut backend = MacOsMusicBackend::new();
        backend.snapshot().await.expect("initial Music.app state");
        let update = backend
            .tick(Duration::ZERO)
            .await
            .expect("playlist query")
            .expect("playlist update");
        let BackendUpdate::Playlists { playlists, .. } = update else {
            panic!("playlist update")
        };
        let hierarchy = crate::domain::PlaylistHierarchy::from_playlists(&playlists);
        let folder_ids = playlists
            .iter()
            .filter(|playlist| playlist.kind == crate::domain::PlaylistKind::Folder)
            .map(|playlist| playlist.id.clone())
            .collect::<std::collections::BTreeSet<_>>();
        assert!(!folder_ids.is_empty(), "expected at least one real folder");
        assert!(
            playlists.iter().any(|playlist| playlist
                .parent_id
                .as_ref()
                .is_some_and(|parent| folder_ids.contains(parent))),
            "expected at least one nested real playlist"
        );
        assert!(!hierarchy.roots.is_empty());
    }

    #[test]
    #[ignore = "profiles the real local Music.app library and requires Automation consent"]
    fn live_profiles_library_batch_sizes() {
        let runner = SystemAutomationRunner;
        for limit in [100, 200, 400, 500] {
            let started = Instant::now();
            let output = runner
                .run(ScriptRequest::ProfileLibraryBatch { start: 0, limit })
                .expect("profile batch");
            let transport = started.elapsed();
            let parse_started = Instant::now();
            let raw = parse_output(&output).expect("profile response");
            let parse = parse_started.elapsed();
            let profile = raw.profile.expect("JXA profile metrics");
            let count = raw.library_batch.expect("library batch").tracks.len();
            println!(
                "batch={limit} tracks={count} total_ms={:.2} collection_ms={:.2} serialization_ms={:.2} parse_ms={:.2} output_bytes={}",
                transport.as_secs_f64() * 1_000.0,
                profile.collection_ms,
                profile.serialization_ms,
                parse.as_secs_f64() * 1_000.0,
                output.len()
            );
            assert_eq!(count, limit);
        }
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
                assert!(tracks.len() <= PLAYLIST_INITIAL_BATCH_SIZE);
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
    #[ignore = "profiles real Music.app playlist loading and requires Automation consent"]
    async fn live_profiles_small_medium_and_large_playlist_loading() {
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
        let mut measured = [None, None, None];
        for playlist in playlists.into_iter().filter(|playlist| {
            playlist.kind != crate::domain::PlaylistKind::Folder
                && persistent_playlist_selector(&playlist.id).is_some()
        }) {
            let started = Instant::now();
            let first = backend
                .execute(BackendCommand::LoadPlaylist(playlist.id.clone()))
                .await
                .expect("first playlist batch");
            let first_track_latency = started.elapsed();
            let (total, mut complete) = match first {
                BackendUpdate::PlaylistBatch {
                    total, complete, ..
                } => (total, complete),
                BackendUpdate::PlaylistLoadFailed { .. } => continue,
                other => panic!("unexpected first playlist update: {other:?}"),
            };
            while !complete {
                let update = backend
                    .tick(Duration::ZERO)
                    .await
                    .expect("playlist continuation")
                    .expect("playlist continuation update");
                match update {
                    BackendUpdate::PlaylistBatch {
                        complete: next_complete,
                        ..
                    } => complete = next_complete,
                    BackendUpdate::PlaylistLoadFailed { .. } => break,
                    _ => {}
                }
            }
            let complete_latency = started.elapsed();
            let category = if (1..=20).contains(&total) {
                Some(0)
            } else if (21..400).contains(&total) {
                Some(1)
            } else if total >= 400 {
                Some(2)
            } else {
                None
            };
            if let Some(category) = category
                && measured[category].is_none()
            {
                println!(
                    "playlist_category={} total={} first_visible_ms={:.2} complete_ms={:.2}",
                    ["small", "medium", "large"][category],
                    total,
                    first_track_latency.as_secs_f64() * 1_000.0,
                    complete_latency.as_secs_f64() * 1_000.0,
                );
                measured[category] = Some((total, first_track_latency, complete_latency));
            }
            if measured.iter().all(Option::is_some) {
                break;
            }
        }
        assert!(
            measured[0].is_some(),
            "no small real playlist was available"
        );
        assert!(
            measured[1].is_some(),
            "no medium real playlist was available"
        );
        assert!(
            measured[2].is_some(),
            "no large real playlist was available"
        );
    }

    #[tokio::test]
    #[ignore = "changes Music.app playback and requires a real four-track playlist"]
    async fn live_playlist_session_advances_three_natural_ends_then_restores_playback() {
        let mut backend = MacOsMusicBackend::new();
        backend.snapshot().await.expect("initial Music.app state");
        let original_status = backend.snapshot.playback.status;
        let original_position = backend.snapshot.playback.position.as_secs() as i64;
        let original_track = backend
            .snapshot
            .playback
            .current_track
            .as_ref()
            .map(|track| track.id.clone());
        let original_repeat = backend.snapshot.playback.repeat;
        let original_shuffle = backend.snapshot.playback.shuffle;
        let update = backend
            .tick(Duration::ZERO)
            .await
            .expect("playlist discovery")
            .expect("playlist update");
        let BackendUpdate::Playlists { playlists, .. } = update else {
            panic!("playlist discovery failed")
        };

        let mut selected = None;
        for playlist in playlists.into_iter().filter(|playlist| {
            playlist.kind != crate::domain::PlaylistKind::Folder
                && persistent_playlist_selector(&playlist.id).is_some()
        }) {
            let first = backend
                .execute(BackendCommand::LoadPlaylist(playlist.id.clone()))
                .await
                .expect("playlist batch");
            let mut complete = matches!(first, BackendUpdate::PlaylistBatch { complete: true, .. });
            while !complete {
                let update = backend
                    .tick(Duration::ZERO)
                    .await
                    .expect("playlist continuation")
                    .expect("playlist continuation update");
                match update {
                    BackendUpdate::PlaylistBatch {
                        complete: next_complete,
                        ..
                    } => complete = next_complete,
                    BackendUpdate::PlaylistLoadFailed { .. } => break,
                    _ => {}
                }
            }
            let Some(loaded) = backend
                .snapshot
                .playlists
                .iter()
                .find(|candidate| candidate.id == playlist.id)
            else {
                continue;
            };
            if loaded.tracks.len() >= 4
                && loaded.tracks.iter().take(4).all(|track| {
                    track_selector(&track.id).is_ok() && track.duration > Duration::from_secs(4)
                })
            {
                selected = Some((
                    loaded.id.clone(),
                    loaded
                        .tracks
                        .iter()
                        .map(|track| track.id.clone())
                        .collect::<Vec<_>>(),
                    loaded
                        .tracks
                        .iter()
                        .take(3)
                        .map(|track| track.duration)
                        .collect::<Vec<_>>(),
                    loaded.tracks.iter().any(|track| {
                        track
                            .metadata
                            .cloud_status
                            .as_deref()
                            .is_some_and(|status| !status.eq_ignore_ascii_case("local"))
                    }),
                ));
                break;
            }
        }
        let (playlist_id, track_ids, durations, contains_cloud) =
            selected.expect("real playlist with four exact playable tracks");

        if original_shuffle {
            backend
                .execute(BackendCommand::ToggleShuffle)
                .await
                .expect("temporarily disable shuffle");
        }
        while backend.snapshot.playback.repeat != RepeatMode::Off {
            backend
                .execute(BackendCommand::CycleRepeat)
                .await
                .expect("temporarily disable repeat");
        }
        backend
            .execute(BackendCommand::PlayPlaylistTrack {
                playlist_id,
                ordered_track_ids: track_ids.clone(),
                selected_index: 0,
                complete: true,
            })
            .await
            .expect("start real playlist session");

        let mut transition_delays = Vec::new();
        let mut post_audio_delays = Vec::new();
        let mut transition_error = None;
        'transitions: for (index, duration) in durations.into_iter().enumerate() {
            let position = backend.snapshot.playback.position.as_secs() as i64;
            let target = duration.as_secs().saturating_sub(1) as i64;
            let target_seconds = u64::try_from(target).unwrap_or_default();
            if target > position {
                backend
                    .execute(BackendCommand::SeekBy(target - position))
                    .await
                    .expect("seek near natural end");
            }
            let seek_started = Instant::now();
            loop {
                tokio::time::sleep(Duration::from_millis(100)).await;
                backend.poll().await;
                if backend.snapshot.playback.position.as_secs() >= target_seconds.saturating_sub(1)
                {
                    break;
                }
                assert!(
                    seek_started.elapsed() < Duration::from_secs(5),
                    "Music.app did not apply the near-end seek"
                );
            }
            let transition_started = Instant::now();
            let remaining_at_start =
                backend
                    .snapshot
                    .playback
                    .current_track
                    .as_ref()
                    .map_or(Duration::ZERO, |track| {
                        track
                            .duration
                            .saturating_sub(backend.snapshot.playback.position)
                    });
            loop {
                tokio::time::sleep(Duration::from_millis(250)).await;
                backend.poll_playlist_transition().await;
                if backend
                    .snapshot
                    .playback
                    .current_track
                    .as_ref()
                    .is_some_and(|track| track.id == track_ids[index + 1])
                {
                    let transition_delay = transition_started.elapsed();
                    transition_delays.push(transition_delay);
                    post_audio_delays.push(transition_delay.saturating_sub(remaining_at_start));
                    break;
                }
                if transition_started.elapsed() >= Duration::from_secs(10) {
                    transition_error = Some(format!(
                        "playlist transition {} -> {} timed out",
                        index + 1,
                        index + 2
                    ));
                    break 'transitions;
                }
            }
        }
        println!(
            "playlist_transition_ms={:?} post_audio_transition_ms={:?} contains_cloud={contains_cloud}",
            transition_delays
                .iter()
                .map(|delay| delay.as_secs_f64() * 1_000.0)
                .collect::<Vec<_>>(),
            post_audio_delays
                .iter()
                .map(|delay| delay.as_secs_f64() * 1_000.0)
                .collect::<Vec<_>>()
        );

        if let Some(original_track) = original_track {
            backend
                .execute(BackendCommand::PlayTrack(original_track))
                .await
                .expect("restore original track");
            if original_position > 0 {
                backend
                    .execute(BackendCommand::SeekBy(original_position))
                    .await
                    .expect("restore original position");
            }
            match original_status {
                PlaybackStatus::Playing => {}
                PlaybackStatus::Paused => {
                    backend
                        .execute(BackendCommand::Pause)
                        .await
                        .expect("restore paused playback");
                }
                PlaybackStatus::Stopped => {
                    backend.run_playback_command(ScriptRequest::Stop).await;
                }
            }
        } else {
            backend.run_playback_command(ScriptRequest::Stop).await;
        }
        while backend.snapshot.playback.repeat != original_repeat {
            backend
                .execute(BackendCommand::CycleRepeat)
                .await
                .expect("restore repeat");
        }
        if backend.snapshot.playback.shuffle != original_shuffle {
            backend
                .execute(BackendCommand::ToggleShuffle)
                .await
                .expect("restore shuffle");
        }

        assert!(transition_error.is_none(), "{transition_error:?}");
        assert_eq!(transition_delays.len(), 3);
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
                ordered_track_ids: vec![track_id.clone()],
                selected_index: 0,
                complete: true,
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
