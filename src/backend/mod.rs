pub mod capabilities;
pub mod macos;
pub mod mock;

use std::time::Duration;

use async_trait::async_trait;
use thiserror::Error;
use tokio::sync::mpsc;

use crate::domain::{
    Album, Artist, BackendAvailability, BackendSnapshot, PlaybackSnapshot, Playlist, PlaylistId,
    Track, TrackId,
};

use self::capabilities::{Capabilities, Capability};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BackendCommand {
    OpenPlayer,
    Play,
    Pause,
    PlayPause,
    PlayTrack(TrackId),
    PlayPlaylistTrack {
        playlist_id: PlaylistId,
        track_id: TrackId,
    },
    PlayPlaylist(PlaylistId),
    LoadPlaylist(PlaylistId),
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BackendUpdate {
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
        loaded: usize,
        total: usize,
        complete: bool,
        artists: Vec<Artist>,
        albums: Vec<Album>,
        recently_added: Vec<Album>,
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

    #[error("queue index {index} is out of bounds for length {length}")]
    QueueIndex { index: usize, length: usize },
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
        ticker.tick().await;
        tracing::debug!(
            backend = backend.name(),
            interval_ms = tick_rate.as_millis(),
            "backend polling started"
        );

        loop {
            tokio::select! {
                command = commands.recv() => {
                    let Some(command) = command else {
                        break;
                    };
                    let event = match backend.execute(command).await {
                        Ok(update) => BackendEvent::Update(update),
                        Err(error) => BackendEvent::Error(error.to_string()),
                    };
                    if events.send(event).await.is_err() {
                        break;
                    }
                    tracing::trace!(backend = backend.name(), "backend command event emitted");
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
    use super::*;
    use crate::domain::{PlaybackSnapshot, PlaybackStatus};

    struct UpdatingBackend {
        emitted_update: bool,
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
}
