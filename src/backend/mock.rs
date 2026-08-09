use std::time::Duration;

use async_trait::async_trait;

use crate::domain::{
    Album, Artist, ArtworkResult, BackendAvailability, BackendSnapshot, CollectionLoadState,
    PlaybackContext, PlaybackSnapshot, PlaybackStatus, Playlist, PlaylistId, QueueItem,
    RecentlyPlayedEntry, RepeatMode, Station, Track, TrackId,
};

use super::{
    BackendCommand, BackendError, BackendUpdate, MusicBackend, capabilities::Capabilities,
};

#[derive(Clone, Debug)]
pub struct MockMusicBackend {
    library: Vec<Track>,
    artists: Vec<Artist>,
    albums: Vec<Album>,
    stations: Vec<Station>,
    playlists: Vec<Playlist>,
    queue: Vec<QueueItem>,
    current_index: usize,
    next_queue_id: u64,
    playback: PlaybackSnapshot,
}

impl MockMusicBackend {
    #[must_use]
    pub fn new() -> Self {
        let mut library = vec![
            Track::new(
                "mock-001",
                "Midnight Terminal",
                "The Asyncs",
                "Event Loop",
                Duration::from_secs(238),
            ),
            Track::new(
                "mock-002",
                "Borrowed Time",
                "Ferris & Friends",
                "Safe Transitions",
                Duration::from_secs(194),
            ),
            Track::new(
                "mock-003",
                "No Blocking Calls",
                "The Asyncs",
                "Event Loop",
                Duration::from_secs(263),
            ),
            Track::new(
                "mock-004",
                "Capability Blues",
                "Ferris & Friends",
                "Safe Transitions",
                Duration::from_secs(221),
            ),
            Track::new(
                "mock-005",
                "Alternate Screen",
                "Raw Mode",
                "Terminal Safety",
                Duration::from_secs(207),
            ),
            Track::new(
                "mock-006",
                "Stale Request",
                "Raw Mode",
                "Terminal Safety",
                Duration::from_secs(182),
            ),
        ];
        library[0].metadata.last_played_date = Some("2026-08-03T18:30:00.000Z".to_owned());
        library[0].metadata.play_count = Some(12);
        library[1].metadata.last_played_date = Some("2026-08-02T09:15:00.000Z".to_owned());
        library[1].metadata.play_count = Some(4);
        let queue = library
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, track)| QueueItem::new(format!("mock-queue-{index:03}"), track))
            .collect::<Vec<_>>();
        let albums = vec![
            Album::new(
                "mock-album-event-loop",
                "Event Loop",
                "The Asyncs",
                2026,
                "2026-07-30",
                vec![library[0].clone(), library[2].clone()],
            ),
            Album::new(
                "mock-album-safe-transitions",
                "Safe Transitions",
                "Ferris & Friends",
                2026,
                "2026-07-24",
                vec![library[1].clone(), library[3].clone()],
            ),
            Album::new(
                "mock-album-terminal-safety",
                "Terminal Safety",
                "Raw Mode",
                2025,
                "2026-07-12",
                vec![library[4].clone(), library[5].clone()],
            ),
        ];
        let artists = vec![
            Artist::new(
                "mock-artist-asyncs",
                "The Asyncs",
                vec![albums[0].id.clone()],
                vec![library[0].id.clone(), library[2].id.clone()],
            ),
            Artist::new(
                "mock-artist-ferris",
                "Ferris & Friends",
                vec![albums[1].id.clone()],
                vec![library[1].id.clone(), library[3].id.clone()],
            ),
            Artist::new(
                "mock-artist-raw-mode",
                "Raw Mode",
                vec![albums[2].id.clone()],
                vec![library[4].id.clone(), library[5].id.clone()],
            ),
        ];
        let stations = vec![
            Station::new(
                "mock-station-music-one",
                "Apple Music 1 (Mock)",
                "Demo global pop and artist interviews.",
            ),
            Station::new(
                "mock-station-chill",
                "Chill Station (Mock)",
                "Deterministic low-key selections for focus.",
            ),
            Station::new(
                "mock-station-alternative",
                "Alternative Station (Mock)",
                "A demo mix of alternative catalog tracks.",
            ),
        ];
        let folder_id = PlaylistId::new("mock-playlist-folder-work");
        let mut folder = Playlist::new(
            folder_id.to_string(),
            "Work",
            Some("A mock folder for hierarchy and expansion tests.".to_owned()),
            Vec::new(),
        );
        folder.kind = crate::domain::PlaylistKind::Folder;
        let mut nested_duplicate = Playlist::new(
            "mock-playlist-nested-terminal-focus",
            "Terminal Focus",
            Some("A duplicate name nested under the Work folder.".to_owned()),
            vec![library[0].clone(), library[4].clone()],
        );
        nested_duplicate.parent_id = Some(folder_id);
        let playlists = vec![
            Playlist::new(
                "mock-playlist-terminal-focus",
                "Terminal Focus",
                Some("A focused set for building reliable terminal applications.".to_owned()),
                vec![library[0].clone(), library[2].clone(), library[4].clone()],
            ),
            Playlist::new(
                "mock-playlist-safe-transitions",
                "Safe Transitions",
                Some("Deterministic tracks for testing state changes.".to_owned()),
                vec![library[1].clone(), library[3].clone(), library[5].clone()],
            ),
            Playlist::new(
                "mock-playlist-all-tracks",
                "Mock Library",
                None,
                library.clone(),
            ),
            folder,
            nested_duplicate,
        ];
        let playback = PlaybackSnapshot {
            current_entry_id: queue.first().map(|item| item.id.clone()),
            current_track: queue.first().map(|item| item.track.clone()),
            ..PlaybackSnapshot::default()
        };

        Self {
            library,
            artists,
            albums,
            stations,
            playlists,
            queue,
            current_index: 0,
            next_queue_id: 6,
            playback,
        }
    }

    fn snapshot_value(&self) -> BackendSnapshot {
        let recently_played = self
            .library
            .iter()
            .filter_map(|track| {
                track
                    .metadata
                    .last_played_date
                    .clone()
                    .map(|played_at| RecentlyPlayedEntry {
                        track_id: track.id.clone(),
                        title: track.title.clone(),
                        artist: track.artist.clone(),
                        play_count: track.metadata.play_count,
                        played_at,
                    })
            })
            .collect();
        BackendSnapshot {
            availability: BackendAvailability::Available,
            playback: self.playback.clone(),
            queue: self.queue.clone(),
            library: self.library.clone(),
            artists: self.artists.clone(),
            albums: self.albums.clone(),
            recently_added: self.albums.clone(),
            recently_played,
            stations: self.stations.clone(),
            playlists: self.playlists.clone(),
            library_status: CollectionLoadState::Loaded {
                total: self.library.len(),
            },
            playlist_status: CollectionLoadState::Loaded {
                total: self.playlists.len(),
            },
        }
    }

    fn require_track(&self) -> Result<(), BackendError> {
        if self.queue.is_empty() {
            Err(BackendError::EmptyQueue)
        } else {
            Ok(())
        }
    }

    fn select_current(&mut self) {
        self.playback.current_entry_id = self
            .queue
            .get(self.current_index)
            .map(|item| item.id.clone());
        self.playback.current_track = self
            .queue
            .get(self.current_index)
            .map(|item| item.track.clone());
        match &mut self.playback.context {
            PlaybackContext::Playlist { current_index, .. }
            | PlaybackContext::Album { current_index, .. } => {
                *current_index = self.current_index;
            }
            PlaybackContext::NoContext => {}
        }
    }

    fn next_track(&mut self, automatic: bool) -> Result<(), BackendError> {
        self.require_track()?;

        if automatic && self.playback.repeat == RepeatMode::One {
            self.playback.position = Duration::ZERO;
            return Ok(());
        }

        if self.current_index + 1 < self.queue.len() {
            self.current_index += 1;
        } else if self.playback.repeat == RepeatMode::All {
            self.current_index = 0;
        } else {
            self.playback.status = PlaybackStatus::Stopped;
            self.playback.context = PlaybackContext::NoContext;
        }
        self.playback.position = Duration::ZERO;
        self.select_current();
        Ok(())
    }

    fn previous_track(&mut self) -> Result<(), BackendError> {
        self.require_track()?;
        self.current_index = self.current_index.saturating_sub(1);
        self.playback.position = Duration::ZERO;
        self.select_current();
        Ok(())
    }

    fn play_track(&mut self, track_id: &TrackId) -> Result<(), BackendError> {
        self.playback.context = PlaybackContext::NoContext;
        let index = self
            .queue
            .iter()
            .position(|item| item.track.id == *track_id)
            .ok_or_else(|| BackendError::TrackNotFound(track_id.clone()))?;
        self.current_index = index;
        self.playback.position = Duration::ZERO;
        self.playback.status = PlaybackStatus::Playing;
        self.select_current();
        Ok(())
    }

    fn play_playlist(&mut self, playlist_id: &PlaylistId) -> Result<(), BackendError> {
        let playlist = self
            .playlists
            .iter()
            .find(|playlist| playlist.id == *playlist_id)
            .cloned()
            .ok_or_else(|| BackendError::PlaylistNotFound(playlist_id.clone()))?;
        let ordered_track_ids = playlist
            .tracks
            .iter()
            .map(|track| track.id.clone())
            .collect::<Vec<_>>();
        self.queue = playlist
            .tracks
            .into_iter()
            .enumerate()
            .map(|(index, track)| QueueItem::new(format!("mock-playlist-{index:03}"), track))
            .collect();
        self.current_index = 0;
        self.playback.position = Duration::ZERO;
        self.playback.status = PlaybackStatus::Playing;
        self.playback.context = PlaybackContext::Playlist {
            playlist_id: playlist_id.clone(),
            ordered_track_ids,
            current_index: 0,
            complete: true,
        };
        self.select_current();
        Ok(())
    }

    fn play_playlist_track(
        &mut self,
        playlist_id: &PlaylistId,
        ordered_track_ids: &[TrackId],
        selected_index: usize,
        complete: bool,
    ) -> Result<(), BackendError> {
        let playlist = self
            .playlists
            .iter()
            .find(|playlist| playlist.id == *playlist_id)
            .ok_or_else(|| BackendError::PlaylistNotFound(playlist_id.clone()))?;
        let tracks = ordered_track_ids
            .iter()
            .map(|track_id| {
                playlist
                    .tracks
                    .iter()
                    .find(|track| track.id == *track_id)
                    .cloned()
                    .ok_or_else(|| BackendError::TrackNotFound(track_id.clone()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        if selected_index >= tracks.len() {
            return Err(BackendError::PlaylistNotFound(playlist_id.clone()));
        }
        self.queue = tracks
            .into_iter()
            .enumerate()
            .map(|(index, track)| QueueItem::new(format!("mock-playlist-{index:03}"), track))
            .collect();
        self.current_index = selected_index;
        self.playback.position = Duration::ZERO;
        self.playback.status = PlaybackStatus::Playing;
        self.playback.context = PlaybackContext::Playlist {
            playlist_id: playlist_id.clone(),
            ordered_track_ids: ordered_track_ids.to_vec(),
            current_index: selected_index,
            complete,
        };
        self.select_current();
        Ok(())
    }

    fn play_album(
        &mut self,
        album_id: &crate::domain::AlbumId,
        track_ids: &[TrackId],
    ) -> Result<(), BackendError> {
        if !self.albums.iter().any(|album| album.id == *album_id) {
            return Err(BackendError::AlbumNotFound(album_id.clone()));
        }
        let tracks = track_ids
            .iter()
            .map(|track_id| {
                self.library
                    .iter()
                    .find(|track| track.id == *track_id)
                    .cloned()
                    .ok_or_else(|| BackendError::TrackNotFound(track_id.clone()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        if tracks.is_empty() {
            return Err(BackendError::AlbumNotFound(album_id.clone()));
        }
        self.queue = tracks
            .into_iter()
            .enumerate()
            .map(|(index, track)| QueueItem::new(format!("mock-album-{index:03}"), track))
            .collect();
        self.current_index = 0;
        self.playback.position = Duration::ZERO;
        self.playback.status = PlaybackStatus::Playing;
        self.playback.context = PlaybackContext::Album {
            album_id: album_id.clone(),
            ordered_track_ids: track_ids.to_vec(),
            current_index: 0,
        };
        self.select_current();
        Ok(())
    }

    fn seek_by(&mut self, seconds: i64) -> Result<(), BackendError> {
        self.require_track()?;
        let duration = self
            .playback
            .current_track
            .as_ref()
            .map_or(0, |track| track.duration.as_secs());
        let current = self.playback.position.as_secs();
        let updated = if seconds.is_negative() {
            current.saturating_sub(seconds.unsigned_abs())
        } else {
            current.saturating_add(seconds as u64)
        }
        .min(duration);
        self.playback.position = Duration::from_secs(updated);
        Ok(())
    }

    fn toggle_favorite(&mut self) -> Result<(), BackendError> {
        self.require_track()?;
        let id = self.queue[self.current_index].track.id.clone();
        let favorite = !self.queue[self.current_index].track.is_favorite;
        self.queue[self.current_index].track.is_favorite = favorite;
        if let Some(track) = self.library.iter_mut().find(|track| track.id == id) {
            track.is_favorite = favorite;
        }
        self.select_current();
        Ok(())
    }

    fn remove_queue_item(&mut self, index: usize) -> Result<(), BackendError> {
        if index >= self.queue.len() {
            return Err(BackendError::QueueIndex {
                index,
                length: self.queue.len(),
            });
        }

        self.queue.remove(index);
        if self.queue.is_empty() {
            self.current_index = 0;
            self.playback.status = PlaybackStatus::Stopped;
            self.playback.position = Duration::ZERO;
        } else if index < self.current_index {
            self.current_index -= 1;
        } else if self.current_index >= self.queue.len() {
            self.current_index = self.queue.len() - 1;
            self.playback.position = Duration::ZERO;
        } else if index == self.current_index {
            self.playback.position = Duration::ZERO;
        }
        self.select_current();
        Ok(())
    }

    fn move_queue_item(&mut self, from: usize, to: usize) -> Result<(), BackendError> {
        let length = self.queue.len();
        if from >= length {
            return Err(BackendError::QueueIndex {
                index: from,
                length,
            });
        }
        if to >= length {
            return Err(BackendError::QueueIndex { index: to, length });
        }

        let current_id = self
            .queue
            .get(self.current_index)
            .map(|item| item.id.clone());
        let item = self.queue.remove(from);
        self.queue.insert(to, item);
        if let Some(current_id) = current_id {
            self.current_index = self
                .queue
                .iter()
                .position(|item| item.id == current_id)
                .unwrap_or_default();
        }
        self.select_current();
        Ok(())
    }
}

impl Default for MockMusicBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MusicBackend for MockMusicBackend {
    fn name(&self) -> &'static str {
        "Mock Playback (no audio)"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::mock()
    }

    async fn snapshot(&mut self) -> Result<BackendSnapshot, BackendError> {
        Ok(self.snapshot_value())
    }

    async fn execute(&mut self, command: BackendCommand) -> Result<BackendUpdate, BackendError> {
        match command {
            BackendCommand::RefreshLibrary => {
                return Ok(BackendUpdate::Notice {
                    availability: BackendAvailability::Available,
                    playback: self.playback.clone(),
                    message: "Mock library is already ready".to_owned(),
                });
            }
            BackendCommand::OpenPlayer => {
                return Err(BackendError::Unsupported(
                    crate::backend::capabilities::Capability::Launch,
                ));
            }
            BackendCommand::Play => {
                self.require_track()?;
                self.playback.status = PlaybackStatus::Playing;
            }
            BackendCommand::Pause => {
                self.require_track()?;
                if self.playback.status == PlaybackStatus::Playing {
                    self.playback.status = PlaybackStatus::Paused;
                }
            }
            BackendCommand::Stop => {
                self.playback.status = PlaybackStatus::Stopped;
                self.playback.position = Duration::ZERO;
                self.playback.context = PlaybackContext::NoContext;
                return Ok(BackendUpdate::Stopped {
                    availability: BackendAvailability::Available,
                    playback: self.playback.clone(),
                });
            }
            BackendCommand::PlayPause => {
                self.require_track()?;
                self.playback.status = match self.playback.status {
                    PlaybackStatus::Playing => PlaybackStatus::Paused,
                    PlaybackStatus::Paused | PlaybackStatus::Stopped => PlaybackStatus::Playing,
                };
            }
            BackendCommand::PlayTrack(track_id) => self.play_track(&track_id)?,
            BackendCommand::PlayPlaylistTrack {
                playlist_id,
                ordered_track_ids,
                selected_index,
                complete,
            } => self.play_playlist_track(
                &playlist_id,
                &ordered_track_ids,
                selected_index,
                complete,
            )?,
            BackendCommand::PlayPlaylist(playlist_id) => self.play_playlist(&playlist_id)?,
            BackendCommand::PlayAlbum {
                album_id,
                track_ids,
            } => self.play_album(&album_id, &track_ids)?,
            BackendCommand::LoadTrackArtwork { key, .. } => {
                return Ok(BackendUpdate::Artwork {
                    availability: BackendAvailability::Available,
                    playback: self.playback.clone(),
                    key,
                    result: ArtworkResult::Missing,
                });
            }
            BackendCommand::LoadPlaylist(_) => {}
            BackendCommand::RemovePlaylistTrack { .. } => {
                return Err(BackendError::Unsupported(
                    crate::backend::capabilities::Capability::PlaylistTrackRemove,
                ));
            }
            BackendCommand::Next => self.next_track(false)?,
            BackendCommand::Previous => self.previous_track()?,
            BackendCommand::SeekBy(seconds) => self.seek_by(seconds)?,
            BackendCommand::SetVolume(volume) => self.playback.volume = volume.min(100),
            BackendCommand::ToggleMute => self.playback.muted = !self.playback.muted,
            BackendCommand::ToggleShuffle => {
                self.playback.shuffle = !self.playback.shuffle;
                self.playback.context = PlaybackContext::NoContext;
            }
            BackendCommand::CycleRepeat => self.playback.repeat = self.playback.repeat.next(),
            BackendCommand::ToggleFavoriteCurrent => self.toggle_favorite()?,
            BackendCommand::Enqueue(track) => {
                let queue_id = format!("mock-queue-{:03}", self.next_queue_id);
                self.next_queue_id = self.next_queue_id.saturating_add(1);
                self.queue.push(QueueItem::new(queue_id, *track));
                if self.playback.current_track.is_none() {
                    self.current_index = 0;
                    self.select_current();
                }
            }
            BackendCommand::RemoveQueueItem(index) => self.remove_queue_item(index)?,
            BackendCommand::MoveQueueItem { from, to } => self.move_queue_item(from, to)?,
        }
        Ok(BackendUpdate::Snapshot(self.snapshot_value()))
    }

    async fn tick(&mut self, elapsed: Duration) -> Result<Option<BackendUpdate>, BackendError> {
        if self.playback.status != PlaybackStatus::Playing {
            return Ok(None);
        }

        self.playback.position = self.playback.position.saturating_add(elapsed);
        let duration = self
            .playback
            .current_track
            .as_ref()
            .map_or(Duration::ZERO, |track| track.duration);
        if self.playback.position >= duration {
            self.next_track(true)?;
        }
        Ok(Some(BackendUpdate::Snapshot(self.snapshot_value())))
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::{
        backend::{BackendCommand, BackendUpdate, MusicBackend, capabilities::Capability},
        domain::{
            AlbumId, BackendSnapshot, PlaybackContext, PlaybackStatus, PlaylistId, RepeatMode,
            Track, TrackId,
        },
    };

    use super::MockMusicBackend;

    fn snapshot(update: BackendUpdate) -> BackendSnapshot {
        let BackendUpdate::Snapshot(snapshot) = update else {
            panic!("mock backend must emit full snapshots")
        };
        snapshot
    }

    #[tokio::test]
    async fn playback_commands_change_real_mock_state() {
        let mut backend = MockMusicBackend::new();
        assert!(backend.capabilities().supports(Capability::Playback));

        let playing = snapshot(
            backend
                .execute(BackendCommand::Play)
                .await
                .expect("mock play succeeds"),
        );
        assert_eq!(playing.playback.status, PlaybackStatus::Playing);

        let advanced = snapshot(
            backend
                .tick(Duration::from_secs(2))
                .await
                .expect("mock tick succeeds")
                .expect("playing backend emits a snapshot"),
        );
        assert_eq!(advanced.playback.position, Duration::from_secs(2));

        let paused = snapshot(
            backend
                .execute(BackendCommand::Pause)
                .await
                .expect("mock pause succeeds"),
        );
        assert_eq!(paused.playback.status, PlaybackStatus::Paused);
        assert!(
            backend
                .tick(Duration::from_secs(2))
                .await
                .expect("paused tick succeeds")
                .is_none()
        );

        let toggled = snapshot(
            backend
                .execute(BackendCommand::PlayPause)
                .await
                .expect("mock toggle succeeds"),
        );
        assert_eq!(toggled.playback.status, PlaybackStatus::Playing);

        let sought = snapshot(
            backend
                .execute(BackendCommand::SeekBy(30))
                .await
                .expect("mock seek succeeds"),
        );
        assert_eq!(sought.playback.position, Duration::from_secs(32));

        let next = snapshot(
            backend
                .execute(BackendCommand::Next)
                .await
                .expect("mock next succeeds"),
        );
        assert_eq!(
            next.playback
                .current_track
                .as_ref()
                .expect("current track")
                .id
                .to_string(),
            "mock-002"
        );

        let previous = snapshot(
            backend
                .execute(BackendCommand::Previous)
                .await
                .expect("mock previous succeeds"),
        );
        assert_eq!(
            previous
                .playback
                .current_track
                .as_ref()
                .expect("current track")
                .id
                .to_string(),
            "mock-001"
        );

        let favorited = snapshot(
            backend
                .execute(BackendCommand::ToggleFavoriteCurrent)
                .await
                .expect("mock favorite succeeds"),
        );
        assert!(
            favorited
                .playback
                .current_track
                .expect("current track")
                .is_favorite
        );

        let repeated = snapshot(
            backend
                .execute(BackendCommand::CycleRepeat)
                .await
                .expect("mock repeat succeeds"),
        );
        assert_eq!(repeated.playback.repeat, RepeatMode::All);
    }

    #[tokio::test]
    async fn queue_operations_mutate_the_queue() {
        let mut backend = MockMusicBackend::new();
        let original_length = backend.snapshot().await.expect("snapshot").queue.len();
        let extra = Track::new(
            "mock-extra",
            "Extra",
            "Fixture",
            "Tests",
            Duration::from_secs(60),
        );

        let appended = snapshot(
            backend
                .execute(BackendCommand::Enqueue(Box::new(extra)))
                .await
                .expect("enqueue"),
        );
        assert_eq!(appended.queue.len(), original_length + 1);

        let moved = snapshot(
            backend
                .execute(BackendCommand::MoveQueueItem {
                    from: original_length,
                    to: 0,
                })
                .await
                .expect("reorder"),
        );
        assert_eq!(moved.queue[0].track.id.to_string(), "mock-extra");

        let removed = snapshot(
            backend
                .execute(BackendCommand::RemoveQueueItem(0))
                .await
                .expect("remove"),
        );
        assert_eq!(removed.queue.len(), original_length);
    }

    #[tokio::test]
    async fn snapshot_exposes_navigable_mock_playlists() {
        let mut backend = MockMusicBackend::new();
        let snapshot = backend.snapshot().await.expect("snapshot");

        assert_eq!(snapshot.library.len(), 6);
        assert_eq!(snapshot.artists.len(), 3);
        assert_eq!(snapshot.albums.len(), 3);
        assert_eq!(snapshot.stations.len(), 3);
        assert_eq!(snapshot.playlists.len(), 5);
        assert_eq!(snapshot.artists[0].name, "The Asyncs");
        assert_eq!(snapshot.albums[0].title, "Event Loop");
        assert_eq!(snapshot.stations[0].name, "Apple Music 1 (Mock)");
        assert_eq!(snapshot.playlists[0].name, "Terminal Focus");
        assert!(snapshot.playlists[0].description.is_some());
        assert_eq!(snapshot.playlists[0].tracks.len(), 3);
    }

    #[tokio::test]
    async fn play_track_selects_a_mock_library_song() {
        let mut backend = MockMusicBackend::new();

        let snapshot = snapshot(
            backend
                .execute(BackendCommand::PlayTrack(TrackId::new("mock-004")))
                .await
                .expect("play selected track"),
        );

        assert_eq!(snapshot.playback.status, PlaybackStatus::Playing);
        assert_eq!(
            snapshot.playback.current_track.expect("current track").id,
            TrackId::new("mock-004")
        );
        assert_eq!(snapshot.playback.position, Duration::ZERO);
    }

    #[tokio::test]
    async fn play_album_replaces_queue_in_supplied_track_order() {
        let mut backend = MockMusicBackend::new();

        let snapshot = snapshot(
            backend
                .execute(BackendCommand::PlayAlbum {
                    album_id: AlbumId::new("mock-album-event-loop"),
                    track_ids: vec![TrackId::new("mock-003"), TrackId::new("mock-001")],
                })
                .await
                .expect("play mock album"),
        );

        assert_eq!(snapshot.playback.status, PlaybackStatus::Playing);
        assert_eq!(
            snapshot
                .queue
                .iter()
                .map(|entry| entry.track.id.clone())
                .collect::<Vec<_>>(),
            vec![TrackId::new("mock-003"), TrackId::new("mock-001")]
        );
        assert_eq!(
            snapshot
                .playback
                .current_track
                .expect("first album track")
                .id,
            TrackId::new("mock-003")
        );
    }

    #[tokio::test]
    async fn selected_mock_playlist_track_keeps_ordered_context_and_advances() {
        let mut backend = MockMusicBackend::new();
        let ordered_track_ids = vec![
            TrackId::new("mock-001"),
            TrackId::new("mock-003"),
            TrackId::new("mock-005"),
        ];
        let started = snapshot(
            backend
                .execute(BackendCommand::PlayPlaylistTrack {
                    playlist_id: PlaylistId::new("mock-playlist-terminal-focus"),
                    ordered_track_ids: ordered_track_ids.clone(),
                    selected_index: 1,
                    complete: true,
                })
                .await
                .expect("start mock playlist context"),
        );
        assert!(matches!(
            started.playback.context,
            PlaybackContext::Playlist {
                current_index: 1,
                ..
            }
        ));
        assert_eq!(
            started.playback.current_track.expect("selected track").id,
            TrackId::new("mock-003")
        );

        let advanced = snapshot(
            backend
                .tick(Duration::from_secs(264))
                .await
                .expect("mock tick")
                .expect("mock transition"),
        );
        assert_eq!(
            advanced.playback.current_track.expect("next track").id,
            TrackId::new("mock-005")
        );
        assert_eq!(
            advanced.playback.context,
            PlaybackContext::Playlist {
                playlist_id: PlaylistId::new("mock-playlist-terminal-focus"),
                ordered_track_ids,
                current_index: 2,
                complete: true,
            }
        );
    }

    #[tokio::test]
    async fn stop_clears_mock_playback_context() {
        let mut backend = MockMusicBackend::new();
        backend.execute(BackendCommand::Play).await.expect("play");
        let BackendUpdate::Stopped { playback, .. } =
            backend.execute(BackendCommand::Stop).await.expect("stop")
        else {
            panic!("stop must acknowledge shutdown");
        };
        assert_eq!(playback.status, PlaybackStatus::Stopped);
        assert_eq!(playback.context, PlaybackContext::NoContext);
    }
}
