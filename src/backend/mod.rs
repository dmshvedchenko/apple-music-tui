pub mod capabilities;
pub mod macos;
pub mod mock;

use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use thiserror::Error;
use tokio::sync::mpsc;

use crate::domain::{
    Album, AlbumId, Artist, ArtworkKey, ArtworkResult, BackendAvailability, BackendSnapshot,
    PlaybackSnapshot, Playlist, PlaylistId, RecentlyPlayedEntry, Track, TrackId,
};

use self::capabilities::{Capabilities, Capability};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BackendCommand {
    /// Starts a new bounded authoritative Music.app library scan when no scan is active.
    RefreshLibrary,
    OpenPlayer,
    Play,
    Pause,
    Stop,
    PlayPause,
    PlayTrack(TrackId),
    PlayPlaylistTrack {
        playlist_id: PlaylistId,
        ordered_track_ids: Vec<TrackId>,
        selected_index: usize,
        complete: bool,
    },
    PlayPlaylist(PlaylistId),
    PlayAlbum {
        album_id: AlbumId,
        track_ids: Vec<TrackId>,
    },
    LoadTrackArtwork {
        key: ArtworkKey,
        track_id: TrackId,
    },
    LoadPlaylist(PlaylistId),
    RemovePlaylistTrack {
        playlist_id: PlaylistId,
        index: usize,
        expected_track_id: TrackId,
    },
    Next,
    Previous,
    SeekBy(i64),
    SetVolume(u8),
    ToggleMute,
    ToggleShuffle,
    CycleRepeat,
    ToggleFavoriteCurrent,
    Enqueue(Box<Track>),
    RemoveQueueItem(usize),
    MoveQueueItem {
        from: usize,
        to: usize,
    },
}

/// Scheduling class for work submitted to the single Music.app automation worker.
///
/// The worker remains deliberately serialized: Music.app Apple Events are not safe to
/// mutate concurrently.  This classification only decides what runs *after* the
/// currently executing Apple Event returns.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum CommandPriority {
    Low,
    Normal,
    High,
    Interactive,
}

impl CommandPriority {
    const fn is_interactive(self) -> bool {
        matches!(self, Self::Interactive)
    }
}

const fn command_priority(command: &BackendCommand) -> CommandPriority {
    match command {
        BackendCommand::Play
        | BackendCommand::Pause
        | BackendCommand::Stop
        | BackendCommand::PlayPause
        | BackendCommand::PlayTrack(_)
        | BackendCommand::PlayPlaylistTrack { .. }
        | BackendCommand::PlayPlaylist(_)
        | BackendCommand::PlayAlbum { .. }
        | BackendCommand::Next
        | BackendCommand::Previous
        | BackendCommand::SeekBy(_)
        | BackendCommand::SetVolume(_)
        | BackendCommand::ToggleMute
        | BackendCommand::ToggleShuffle
        | BackendCommand::CycleRepeat => CommandPriority::Interactive,
        BackendCommand::LoadPlaylist(_) | BackendCommand::RemovePlaylistTrack { .. } => {
            CommandPriority::High
        }
        BackendCommand::LoadTrackArtwork { .. } => CommandPriority::Normal,
        BackendCommand::RefreshLibrary
        | BackendCommand::OpenPlayer
        | BackendCommand::ToggleFavoriteCurrent
        | BackendCommand::Enqueue(_)
        | BackendCommand::RemoveQueueItem(_)
        | BackendCommand::MoveQueueItem { .. } => CommandPriority::Low,
    }
}

/// Returns whether the command is a latency-sensitive direct user operation.
#[must_use]
pub const fn is_interactive_command(command: &BackendCommand) -> bool {
    command_priority(command).is_interactive()
}

#[derive(Debug)]
struct QueuedCommand {
    command: BackendCommand,
    queued_at: Instant,
}

/// Small fair priority scheduler for serialized backend work.
///
/// Eight foreground operations may pass a low-priority operation before it is
/// serviced.  This prevents refresh work from starving during sustained input,
/// while never letting it delay the first waiting interactive command.
#[derive(Default)]
struct CommandScheduler {
    interactive: VecDeque<QueuedCommand>,
    high: VecDeque<QueuedCommand>,
    normal: VecDeque<QueuedCommand>,
    low: VecDeque<QueuedCommand>,
    foreground_since_low: usize,
}

impl CommandScheduler {
    fn enqueue(&mut self, command: BackendCommand) {
        if self.contains_equivalent_coalescible(&command) {
            tracing::debug!(?command, "coalesced redundant backend command");
            return;
        }
        let queued = QueuedCommand {
            command,
            queued_at: Instant::now(),
        };
        match command_priority(&queued.command) {
            CommandPriority::Interactive => self.interactive.push_back(queued),
            CommandPriority::High => self.high.push_back(queued),
            CommandPriority::Normal => self.normal.push_back(queued),
            CommandPriority::Low => self.low.push_back(queued),
        }
    }

    fn contains_equivalent_coalescible(&self, command: &BackendCommand) -> bool {
        self.interactive
            .iter()
            .chain(&self.high)
            .chain(&self.normal)
            .chain(&self.low)
            .any(|queued| match (&queued.command, command) {
                (BackendCommand::RefreshLibrary, BackendCommand::RefreshLibrary) => true,
                (BackendCommand::LoadPlaylist(left), BackendCommand::LoadPlaylist(right)) => {
                    left == right
                }
                (
                    BackendCommand::LoadTrackArtwork { key: left, .. },
                    BackendCommand::LoadTrackArtwork { key: right, .. },
                ) => left == right,
                _ => false,
            })
    }

    fn pop_next(&mut self) -> Option<QueuedCommand> {
        if !self.low.is_empty() && self.foreground_since_low >= 8 {
            self.foreground_since_low = 0;
            return self.low.pop_front();
        }

        let next = self
            .interactive
            .pop_front()
            .or_else(|| self.high.pop_front())
            .or_else(|| self.normal.pop_front());
        if next.is_some() {
            self.foreground_since_low += 1;
            return next;
        }
        self.foreground_since_low = 0;
        self.low.pop_front()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BackendUpdate {
    LibraryRefreshStarted {
        availability: BackendAvailability,
        playback: PlaybackSnapshot,
    },
    LibraryRefreshFailed {
        availability: BackendAvailability,
        playback: PlaybackSnapshot,
        message: String,
    },
    Snapshot(BackendSnapshot),
    Playback {
        availability: BackendAvailability,
        playback: PlaybackSnapshot,
    },
    Playlists {
        availability: BackendAvailability,
        playback: PlaybackSnapshot,
        playlists: Vec<Playlist>,
    },
    LibraryBatch {
        availability: BackendAvailability,
        playback: PlaybackSnapshot,
        tracks: Vec<Track>,
        authoritative_tracks: Option<Vec<Track>>,
        loaded: usize,
        total: usize,
        complete: bool,
        artists: Vec<Artist>,
        albums: Vec<Album>,
        recently_added: Vec<Album>,
        recently_played: Vec<RecentlyPlayedEntry>,
    },
    PlaylistBatch {
        availability: BackendAvailability,
        playback: PlaybackSnapshot,
        playlist_id: PlaylistId,
        tracks: Vec<Track>,
        loaded: usize,
        total: usize,
        complete: bool,
    },
    PlaylistLoadFailed {
        availability: BackendAvailability,
        playback: PlaybackSnapshot,
        playlist_id: PlaylistId,
        message: String,
    },
    PlaylistTrackRemoved {
        availability: BackendAvailability,
        playback: PlaybackSnapshot,
        playlist_id: PlaylistId,
        index: usize,
        expected_track_id: TrackId,
    },
    Stopped {
        availability: BackendAvailability,
        playback: PlaybackSnapshot,
    },
    PlaybackContextFailed {
        availability: BackendAvailability,
        playback: PlaybackSnapshot,
        message: String,
    },
    Notice {
        availability: BackendAvailability,
        playback: PlaybackSnapshot,
        message: String,
    },
    Artwork {
        availability: BackendAvailability,
        playback: PlaybackSnapshot,
        key: ArtworkKey,
        result: ArtworkResult,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BackendEvent {
    Ready {
        name: &'static str,
        capabilities: Capabilities,
        snapshot: BackendSnapshot,
    },
    Update(BackendUpdate),
    Error(String),
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum BackendError {
    #[error("backend does not support {0:?}")]
    Unsupported(Capability),

    #[error("the playback queue is empty")]
    EmptyQueue,

    #[error("track '{0}' is not available in the playback queue")]
    TrackNotFound(TrackId),

    #[error("playlist '{0}' is not available")]
    PlaylistNotFound(PlaylistId),

    #[error("album '{0}' is not available")]
    AlbumNotFound(AlbumId),

    #[error("queue index {index} is out of bounds for length {length}")]
    QueueIndex { index: usize, length: usize },

    #[error("{0}")]
    OperationFailed(String),
}

#[async_trait]
pub trait MusicBackend: Send + 'static {
    fn name(&self) -> &'static str;

    fn capabilities(&self) -> Capabilities;

    fn poll_interval(&self) -> Duration {
        Duration::from_millis(500)
    }

    async fn snapshot(&mut self) -> Result<BackendSnapshot, BackendError>;

    async fn execute(&mut self, command: BackendCommand) -> Result<BackendUpdate, BackendError>;

    async fn tick(&mut self, elapsed: Duration) -> Result<Option<BackendUpdate>, BackendError>;
}

#[must_use]
pub fn spawn_worker<B: MusicBackend>(
    mut backend: B,
    mut commands: mpsc::Receiver<BackendCommand>,
    events: mpsc::Sender<BackendEvent>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        tracing::debug!(backend = backend.name(), "backend worker started");
        match backend.snapshot().await {
            Ok(snapshot) => {
                if events
                    .send(BackendEvent::Ready {
                        name: backend.name(),
                        capabilities: backend.capabilities(),
                        snapshot,
                    })
                    .await
                    .is_err()
                {
                    return;
                }
                tracing::trace!(
                    backend = backend.name(),
                    "initial backend state event emitted"
                );
            }
            Err(error) => {
                tracing::debug!(backend = backend.name(), %error, "initial backend query failed");
                let _ = events.send(BackendEvent::Error(error.to_string())).await;
                return;
            }
        }

        let tick_rate = backend
            .poll_interval()
            .clamp(Duration::from_millis(250), Duration::from_millis(1_000));
        let mut ticker = tokio::time::interval(tick_rate);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        tracing::debug!(
            backend = backend.name(),
            interval_ms = tick_rate.as_millis(),
            "backend polling started"
        );
        let mut scheduler = CommandScheduler::default();

        loop {
            // Drain commands received while the prior request was running before selecting
            // another request. This is what lets direct input pass already-queued background
            // work without attempting to interrupt an active Apple Event.
            while let Ok(command) = commands.try_recv() {
                scheduler.enqueue(command);
            }

            if let Some(queued) = scheduler.pop_next() {
                let priority = command_priority(&queued.command);
                let queue_wait = queued.queued_at.elapsed();
                let command_started = Instant::now();
                tracing::debug!(
                    ?priority,
                    command = ?queued.command,
                    queue_wait_ms = queue_wait.as_secs_f64() * 1_000.0,
                    "backend command dequeued"
                );
                let event = match backend.execute(queued.command).await {
                    Ok(update) => BackendEvent::Update(update),
                    Err(error) => BackendEvent::Error(error.to_string()),
                };
                tracing::debug!(
                    ?priority,
                    queue_wait_ms = queue_wait.as_secs_f64() * 1_000.0,
                    command_ms = command_started.elapsed().as_secs_f64() * 1_000.0,
                    total_ms = (queue_wait + command_started.elapsed()).as_secs_f64() * 1_000.0,
                    "backend command timing"
                );
                if events.send(event).await.is_err() {
                    break;
                }
                continue;
            }

            tokio::select! {
                biased;
                command = commands.recv() => {
                    let Some(command) = command else {
                        break;
                    };
                    scheduler.enqueue(command);
                }
                _ = ticker.tick() => {
                    tracing::trace!(backend = backend.name(), "backend poll tick");
                    match backend.tick(tick_rate).await {
                        Ok(Some(update)) => {
                            if events.send(BackendEvent::Update(update)).await.is_err() {
                                break;
                            }
                            tracing::trace!(backend = backend.name(), "backend state event emitted");
                        }
                        Ok(None) => {}
                        Err(error) => {
                            if events.send(BackendEvent::Error(error.to_string())).await.is_err() {
                                break;
                            }
                        }
                    }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{Arc, Mutex},
        time::Duration,
    };

    use super::*;
    use crate::domain::{PlaybackSnapshot, PlaybackStatus};

    struct UpdatingBackend {
        emitted_update: bool,
    }

    #[derive(Clone)]
    struct RecordingBackend {
        executed: Arc<Mutex<Vec<BackendCommand>>>,
    }

    #[async_trait]
    impl MusicBackend for RecordingBackend {
        fn name(&self) -> &'static str {
            "scheduler-test"
        }

        fn capabilities(&self) -> Capabilities {
            Capabilities::default()
        }

        async fn snapshot(&mut self) -> Result<BackendSnapshot, BackendError> {
            Ok(BackendSnapshot::default())
        }

        async fn execute(
            &mut self,
            command: BackendCommand,
        ) -> Result<BackendUpdate, BackendError> {
            self.executed.lock().expect("execution log").push(command);
            Ok(BackendUpdate::Snapshot(BackendSnapshot::default()))
        }

        async fn tick(
            &mut self,
            _elapsed: Duration,
        ) -> Result<Option<BackendUpdate>, BackendError> {
            Ok(None)
        }
    }

    #[async_trait]
    impl MusicBackend for UpdatingBackend {
        fn name(&self) -> &'static str {
            "updating-test"
        }

        fn capabilities(&self) -> Capabilities {
            Capabilities::default()
        }

        fn poll_interval(&self) -> Duration {
            Duration::from_millis(250)
        }

        async fn snapshot(&mut self) -> Result<BackendSnapshot, BackendError> {
            Ok(BackendSnapshot {
                playback: PlaybackSnapshot {
                    status: PlaybackStatus::Paused,
                    ..PlaybackSnapshot::default()
                },
                ..BackendSnapshot::default()
            })
        }

        async fn execute(
            &mut self,
            _command: BackendCommand,
        ) -> Result<BackendUpdate, BackendError> {
            Ok(BackendUpdate::Snapshot(self.snapshot().await?))
        }

        async fn tick(
            &mut self,
            _elapsed: Duration,
        ) -> Result<Option<BackendUpdate>, BackendError> {
            if self.emitted_update {
                return Ok(None);
            }
            self.emitted_update = true;
            Ok(Some(BackendUpdate::Snapshot(BackendSnapshot {
                playback: PlaybackSnapshot {
                    status: PlaybackStatus::Playing,
                    position: Duration::from_millis(1_500),
                    ..PlaybackSnapshot::default()
                },
                ..BackendSnapshot::default()
            })))
        }
    }

    #[tokio::test]
    async fn initial_and_repeated_backend_states_reach_the_event_channel() {
        let (command_sender, command_receiver) = mpsc::channel(4);
        let (event_sender, mut event_receiver) = mpsc::channel(4);
        let worker = spawn_worker(
            UpdatingBackend {
                emitted_update: false,
            },
            command_receiver,
            event_sender,
        );

        let ready = event_receiver.recv().await.expect("initial event");
        let BackendEvent::Ready { snapshot, .. } = ready else {
            panic!("expected ready event");
        };
        assert_eq!(snapshot.playback.status, PlaybackStatus::Paused);

        let update = tokio::time::timeout(Duration::from_secs(1), event_receiver.recv())
            .await
            .expect("poll update timeout")
            .expect("poll update event");
        let BackendEvent::Update(BackendUpdate::Snapshot(snapshot)) = update else {
            panic!("expected snapshot event");
        };
        assert_eq!(snapshot.playback.status, PlaybackStatus::Playing);
        assert_eq!(snapshot.playback.position, Duration::from_millis(1_500));

        drop(command_sender);
        worker.await.expect("worker shutdown");
    }

    #[tokio::test]
    async fn interactive_playback_jumps_ahead_of_queued_background_work() {
        let executed = Arc::new(Mutex::new(Vec::new()));
        let (command_sender, command_receiver) = mpsc::channel(8);
        command_sender
            .try_send(BackendCommand::RefreshLibrary)
            .expect("queue refresh");
        command_sender
            .try_send(BackendCommand::LoadPlaylist(
                crate::domain::PlaylistId::new("P1"),
            ))
            .expect("queue playlist load");
        command_sender
            .try_send(BackendCommand::PlayPause)
            .expect("queue interactive command");
        drop(command_sender);
        let (event_sender, mut event_receiver) = mpsc::channel(8);
        let worker = spawn_worker(
            RecordingBackend {
                executed: Arc::clone(&executed),
            },
            command_receiver,
            event_sender,
        );

        let _ = event_receiver.recv().await.expect("ready event");
        for _ in 0..3 {
            let _ = event_receiver.recv().await.expect("command event");
        }
        worker.await.expect("worker shutdown");

        assert_eq!(
            *executed.lock().expect("execution log"),
            vec![
                BackendCommand::PlayPause,
                BackendCommand::LoadPlaylist(crate::domain::PlaylistId::new("P1")),
                BackendCommand::RefreshLibrary,
            ]
        );
    }

    #[tokio::test]
    async fn duplicate_playlist_loads_and_artwork_requests_are_coalesced() {
        let executed = Arc::new(Mutex::new(Vec::new()));
        let (command_sender, command_receiver) = mpsc::channel(8);
        let playlist = crate::domain::PlaylistId::new("P1");
        command_sender
            .try_send(BackendCommand::LoadPlaylist(playlist.clone()))
            .expect("first playlist load");
        command_sender
            .try_send(BackendCommand::LoadPlaylist(playlist))
            .expect("duplicate playlist load");
        drop(command_sender);
        let (event_sender, mut event_receiver) = mpsc::channel(8);
        let worker = spawn_worker(
            RecordingBackend {
                executed: Arc::clone(&executed),
            },
            command_receiver,
            event_sender,
        );

        let _ = event_receiver.recv().await.expect("ready event");
        let _ = event_receiver.recv().await.expect("playlist event");
        worker.await.expect("worker shutdown");

        assert!(matches!(
            executed.lock().expect("execution log").as_slice(),
            [BackendCommand::LoadPlaylist(id)] if id == &crate::domain::PlaylistId::new("P1")
        ));
    }

    #[test]
    fn ordered_interactive_commands_stay_ordered_and_background_work_runs() {
        let mut scheduler = CommandScheduler::default();
        scheduler.enqueue(BackendCommand::RefreshLibrary);
        scheduler.enqueue(BackendCommand::Next);
        scheduler.enqueue(BackendCommand::Previous);

        assert!(matches!(
            scheduler.pop_next().map(|queued| queued.command),
            Some(BackendCommand::Next)
        ));
        assert!(matches!(
            scheduler.pop_next().map(|queued| queued.command),
            Some(BackendCommand::Previous)
        ));
        assert!(matches!(
            scheduler.pop_next().map(|queued| queued.command),
            Some(BackendCommand::RefreshLibrary)
        ));
    }

    #[test]
    fn low_priority_work_is_fair_after_eight_foreground_commands() {
        let mut scheduler = CommandScheduler::default();
        scheduler.enqueue(BackendCommand::RefreshLibrary);
        for _ in 0..9 {
            scheduler.enqueue(BackendCommand::PlayPause);
        }

        for _ in 0..8 {
            assert!(matches!(
                scheduler.pop_next().map(|queued| queued.command),
                Some(BackendCommand::PlayPause)
            ));
        }
        assert!(matches!(
            scheduler.pop_next().map(|queued| queued.command),
            Some(BackendCommand::RefreshLibrary)
        ));
    }
}
