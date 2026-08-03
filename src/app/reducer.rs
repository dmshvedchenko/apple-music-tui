use crate::backend::{BackendCommand, BackendEvent, BackendUpdate, capabilities::Capability};

use super::{
    action::{Action, Command},
    state::{
        AppState, BackendStatus, Focus, LocalSearchIndexEntry, LocalSearchResult, NavigationEntry,
        QUEUE_PANE_MIN_WIDTH, Route, Screen, ViewStatus,
    },
};

pub fn reduce(state: &mut AppState, action: Action) -> Vec<Command> {
    if state.help_open
        && !matches!(
            action,
            Action::Quit
                | Action::ToggleHelp
                | Action::Back
                | Action::Resize { .. }
                | Action::Backend(_)
        )
    {
        return Vec::new();
    }

    match action {
        Action::Quit => state.should_quit = true,
        Action::OpenPlayer => {
            return backend_command(state, Capability::Launch, BackendCommand::OpenPlayer);
        }
        Action::MoveUp => move_selection(state, false),
        Action::MoveDown => move_selection(state, true),
        Action::FocusLeft => {
            if matches!(
                &state.navigation.active,
                Route::ArtistDetail { .. }
                    | Route::AlbumDetail { .. }
                    | Route::PlaylistDetail { .. }
            ) {
                navigate_back(state);
            } else {
                state.focus = match state.focus {
                    Focus::Sidebar | Focus::Content => Focus::Sidebar,
                    Focus::Queue => Focus::Content,
                };
            }
        }
        Action::FocusRight => {
            state.focus = match state.focus {
                Focus::Sidebar => Focus::Content,
                Focus::Content
                    if state.terminal_size.0 >= QUEUE_PANE_MIN_WIDTH
                        && state.capabilities.supports(Capability::QueueRead) =>
                {
                    Focus::Queue
                }
                Focus::Content => Focus::Content,
                Focus::Queue => Focus::Queue,
            };
        }
        Action::OpenSelected => return open_selected(state),
        Action::PlaySelectedPlaylist => return play_selected_playlist(state),
        Action::StartSearch => {
            navigate_to_section(state, Screen::Search);
            state.search_input_active = true;
        }
        Action::SearchInput(character) => {
            state.search_query.push(character);
            refresh_search(state);
        }
        Action::SearchBackspace => {
            state.search_query.pop();
            refresh_search(state);
        }
        Action::SubmitSearch => state.search_input_active = false,
        Action::GoTo(screen) => navigate_to_section(state, screen),
        Action::Play => {
            return backend_command(state, Capability::Playback, BackendCommand::Play);
        }
        Action::Pause => {
            return backend_command(state, Capability::Playback, BackendCommand::Pause);
        }
        Action::PlayPause => {
            return backend_command(state, Capability::Playback, BackendCommand::PlayPause);
        }
        Action::NextTrack => {
            return backend_command(state, Capability::Playback, BackendCommand::Next);
        }
        Action::PreviousTrack => {
            return backend_command(state, Capability::Playback, BackendCommand::Previous);
        }
        Action::SeekBackward => {
            return backend_command(state, Capability::Seek, BackendCommand::SeekBy(-5));
        }
        Action::SeekForward => {
            return backend_command(state, Capability::Seek, BackendCommand::SeekBy(5));
        }
        Action::VolumeDown => {
            let volume = state.playback.volume.saturating_sub(5);
            return backend_command(state, Capability::Volume, BackendCommand::SetVolume(volume));
        }
        Action::VolumeUp => {
            let volume = state.playback.volume.saturating_add(5).min(100);
            return backend_command(state, Capability::Volume, BackendCommand::SetVolume(volume));
        }
        Action::ToggleMute => {
            return backend_command(state, Capability::Mute, BackendCommand::ToggleMute);
        }
        Action::ToggleShuffle => {
            return backend_command(state, Capability::Shuffle, BackendCommand::ToggleShuffle);
        }
        Action::CycleRepeat => {
            return backend_command(state, Capability::Repeat, BackendCommand::CycleRepeat);
        }
        Action::ToggleFavorite => {
            return backend_command(
                state,
                Capability::Favorite,
                BackendCommand::ToggleFavoriteCurrent,
            );
        }
        Action::ToggleHelp => state.help_open = !state.help_open,
        Action::Back => {
            if state.help_open {
                state.help_open = false;
            } else if state.search_input_active {
                state.search_input_active = false;
            } else {
                navigate_back(state);
            }
        }
        Action::Resize { width, height } => {
            state.terminal_size = (width, height);
            if (width < QUEUE_PANE_MIN_WIDTH || !state.capabilities.supports(Capability::QueueRead))
                && state.focus == Focus::Queue
            {
                state.focus = Focus::Content;
            }
        }
        Action::Backend(event) => apply_backend_event(state, *event),
    }

    Vec::new()
}

fn open_selected(state: &mut AppState) -> Vec<Command> {
    if state.focus == Focus::Sidebar {
        if let Some(screen) = Screen::ALL.get(state.sidebar_selection).copied() {
            navigate_to_section(state, screen);
        }
        return Vec::new();
    }

    if state.focus != Focus::Content {
        return Vec::new();
    }

    if matches!(
        state.navigation.active,
        Route::Section(Screen::Playlists | Screen::MadeForYou)
    ) {
        let selected = state
            .playlists
            .get(state.content_selection)
            .map(|playlist| {
                (
                    playlist.id.clone(),
                    !playlist.tracks_loaded
                        && state.capabilities.supports(Capability::PlaylistRead),
                )
            });
        if let Some((playlist_id, needs_load)) = selected {
            return open_playlist_detail(state, playlist_id, needs_load);
        }
    }

    if matches!(state.navigation.active, Route::Section(Screen::Search)) {
        match state.search_results.get(state.content_selection).cloned() {
            Some(LocalSearchResult::Artist(artist_id)) => {
                open_detail(state, Route::ArtistDetail { artist_id });
                return Vec::new();
            }
            Some(LocalSearchResult::Album(album_id)) => {
                open_detail(state, Route::AlbumDetail { album_id });
                return Vec::new();
            }
            Some(LocalSearchResult::Playlist(playlist_id)) => {
                let needs_load = state
                    .playlists
                    .iter()
                    .find(|playlist| playlist.id == playlist_id)
                    .is_some_and(|playlist| {
                        !playlist.tracks_loaded
                            && state.capabilities.supports(Capability::PlaylistRead)
                    });
                return open_playlist_detail(state, playlist_id, needs_load);
            }
            Some(LocalSearchResult::Track(_)) | None => {}
        }
    }

    let destination = match &state.navigation.active {
        Route::Section(Screen::Artists) => {
            state
                .artists
                .get(state.content_selection)
                .map(|artist| Route::ArtistDetail {
                    artist_id: artist.id.clone(),
                })
        }
        Route::Section(Screen::Albums) => {
            state
                .albums
                .get(state.content_selection)
                .map(|album| Route::AlbumDetail {
                    album_id: album.id.clone(),
                })
        }
        Route::Section(Screen::RecentlyAdded) => state
            .recently_added
            .get(state.content_selection)
            .map(|album| Route::AlbumDetail {
                album_id: album.id.clone(),
            }),
        _ => None,
    };
    if let Some(destination) = destination {
        open_detail(state, destination);
        return Vec::new();
    }

    if let Some(command) = selected_track_command(state) {
        return backend_command(state, Capability::SelectionPlayback, command);
    }

    Vec::new()
}

fn open_playlist_detail(
    state: &mut AppState,
    playlist_id: crate::domain::PlaylistId,
    needs_load: bool,
) -> Vec<Command> {
    open_detail(
        state,
        Route::PlaylistDetail {
            playlist_id: playlist_id.clone(),
        },
    );
    if needs_load {
        vec![Command::Backend(BackendCommand::LoadPlaylist(playlist_id))]
    } else {
        Vec::new()
    }
}

fn play_selected_playlist(state: &mut AppState) -> Vec<Command> {
    let playlist_id = match &state.navigation.active {
        Route::Section(Screen::Playlists | Screen::MadeForYou) => state
            .playlists
            .get(state.content_selection)
            .map(|playlist| playlist.id.clone()),
        Route::PlaylistDetail { playlist_id } => Some(playlist_id.clone()),
        _ => None,
    };
    let Some(playlist_id) = playlist_id else {
        state.notification = Some("Select a playlist before starting playlist playback".to_owned());
        return Vec::new();
    };
    backend_command(
        state,
        Capability::SelectionPlayback,
        BackendCommand::PlayPlaylist(playlist_id),
    )
}

fn open_detail(state: &mut AppState, destination: Route) {
    state.navigation.history.push(NavigationEntry {
        route: state.navigation.active.clone(),
        content_selection: state.content_selection,
    });
    state.navigation.active = destination;
    state.content_selection = 0;
}

fn navigate_to_section(state: &mut AppState, screen: Screen) {
    state.navigation.active = Route::Section(screen);
    state.navigation.history.clear();
    state.sidebar_selection = Screen::ALL
        .iter()
        .position(|candidate| *candidate == screen)
        .unwrap_or_default();
    state.focus = Focus::Content;
    state.content_selection = 0;
    state.search_input_active = screen == Screen::Search;
    if screen == Screen::Search {
        refresh_search(state);
    }
}

fn navigate_back(state: &mut AppState) {
    if let Some(entry) = state.navigation.history.pop() {
        state.navigation.active = entry.route;
        state.content_selection = entry.content_selection;
        state.focus = Focus::Content;
    } else if state.focus == Focus::Queue {
        state.focus = Focus::Content;
    } else {
        state.focus = Focus::Sidebar;
    }
}

fn move_selection(state: &mut AppState, down: bool) {
    let (selection, length) = match state.focus {
        Focus::Sidebar => (&mut state.sidebar_selection, Screen::ALL.len()),
        Focus::Content => {
            let length = content_length(state);
            (&mut state.content_selection, length)
        }
        Focus::Queue => (&mut state.queue_selection, state.queue.len()),
    };

    if length == 0 {
        *selection = 0;
    } else if down {
        *selection = (*selection + 1).min(length - 1);
    } else {
        *selection = selection.saturating_sub(1);
    }
}

fn content_length(state: &AppState) -> usize {
    match &state.navigation.active {
        Route::Section(Screen::ListenNow | Screen::Browse) => 3,
        Route::Section(Screen::Radio) => state.stations.len(),
        Route::Section(Screen::RecentlyAdded) => state.recently_added.len(),
        Route::Section(Screen::Albums) => state.albums.len(),
        Route::Section(Screen::Artists) => state.artists.len(),
        Route::Section(Screen::Songs) => state.library.len(),
        Route::Section(Screen::Search) => state.search_results.len(),
        Route::Section(Screen::MadeForYou | Screen::Playlists) => state.playlists.len(),
        Route::ArtistDetail { artist_id } => state
            .artists
            .iter()
            .find(|artist| artist.id == *artist_id)
            .map_or(0, |artist| artist.top_track_ids.len()),
        Route::AlbumDetail { album_id } => state
            .albums
            .iter()
            .find(|album| album.id == *album_id)
            .map_or(0, |album| album.tracks.len()),
        Route::PlaylistDetail { playlist_id } => state
            .playlists
            .iter()
            .find(|playlist| playlist.id == *playlist_id)
            .map_or(0, |playlist| playlist.tracks.len()),
    }
}

fn selected_track_command(state: &AppState) -> Option<BackendCommand> {
    match &state.navigation.active {
        Route::Section(Screen::Songs) => state
            .library
            .get(state.content_selection)
            .map(|track| BackendCommand::PlayTrack(track.id.clone())),
        Route::Section(Screen::Search) => state
            .search_results
            .get(state.content_selection)
            .and_then(|result| match result {
                LocalSearchResult::Track(track_id) => {
                    Some(BackendCommand::PlayTrack(track_id.clone()))
                }
                LocalSearchResult::Artist(_)
                | LocalSearchResult::Album(_)
                | LocalSearchResult::Playlist(_) => None,
            }),
        Route::ArtistDetail { artist_id } => state
            .artists
            .iter()
            .find(|artist| artist.id == *artist_id)
            .and_then(|artist| artist.top_track_ids.get(state.content_selection))
            .cloned()
            .map(BackendCommand::PlayTrack),
        Route::AlbumDetail { album_id } => state
            .albums
            .iter()
            .find(|album| album.id == *album_id)
            .and_then(|album| album.tracks.get(state.content_selection))
            .map(|track| BackendCommand::PlayTrack(track.id.clone())),
        Route::PlaylistDetail { playlist_id } => state
            .playlists
            .iter()
            .find(|playlist| playlist.id == *playlist_id)
            .and_then(|playlist| playlist.tracks.get(state.content_selection))
            .map(|track| BackendCommand::PlayPlaylistTrack {
                playlist_id: playlist_id.clone(),
                track_id: track.id.clone(),
            }),
        Route::Section(_) => None,
    }
}

fn backend_command(
    state: &mut AppState,
    capability: Capability,
    command: BackendCommand,
) -> Vec<Command> {
    if state.capabilities.supports(capability) {
        vec![Command::Backend(command)]
    } else {
        state.notification = Some(format!("Active backend does not support {capability:?}"));
        Vec::new()
    }
}

fn apply_backend_event(state: &mut AppState, event: BackendEvent) {
    match event {
        BackendEvent::Ready {
            name,
            capabilities,
            snapshot,
        } => {
            state.backend_status = BackendStatus::Ready {
                name: name.to_owned(),
            };
            state.capabilities = capabilities;
            state.backend_availability = snapshot.availability;
            state.playback = snapshot.playback;
            state.queue = snapshot.queue;
            state.library = snapshot.library;
            state.artists = snapshot.artists;
            state.albums = snapshot.albums;
            state.recently_added = snapshot.recently_added;
            state.stations = snapshot.stations;
            state.playlists = snapshot.playlists;
            state.library_status = snapshot.library_status;
            state.playlist_status = snapshot.playlist_status;
            rebuild_search_index(state);
            refresh_search(state);
            tracing::debug!(
                status = ?state.playback.status,
                has_track = state.playback.current_track.is_some(),
                "application initial playback state updated"
            );
            state.view_status = ViewStatus::Loaded;
        }
        BackendEvent::Update(BackendUpdate::Snapshot(snapshot)) => {
            let previous_status = state.playback.status;
            let previous_track = state
                .playback
                .current_track
                .as_ref()
                .map(|track| track.id.clone());
            state.backend_availability = snapshot.availability;
            state.playback = snapshot.playback;
            state.queue = snapshot.queue;
            state.library = snapshot.library;
            state.artists = snapshot.artists;
            state.albums = snapshot.albums;
            state.recently_added = snapshot.recently_added;
            state.stations = snapshot.stations;
            state.playlists = snapshot.playlists;
            state.library_status = snapshot.library_status;
            state.playlist_status = snapshot.playlist_status;
            rebuild_search_index(state);
            refresh_search(state);
            clamp_selections(state);
            let current_track = state
                .playback
                .current_track
                .as_ref()
                .map(|track| track.id.clone());
            if previous_status != state.playback.status || previous_track != current_track {
                tracing::debug!(
                    previous_status = ?previous_status,
                    current_status = ?state.playback.status,
                    track_changed = previous_track != current_track,
                    "application playback state updated"
                );
            }
            tracing::trace!(
                position_ms = state.playback.position.as_millis(),
                "application playback position updated"
            );
        }
        BackendEvent::Update(BackendUpdate::Playback {
            availability,
            playback,
        }) => apply_playback_update(state, availability, playback),
        BackendEvent::Update(BackendUpdate::Playlists {
            availability,
            playback,
            playlists,
        }) => {
            apply_playback_update(state, availability, playback);
            state.playlists = playlists;
            state.view_status = ViewStatus::Loaded;
            state.playlist_status = crate::domain::CollectionLoadState::Loaded {
                total: state.playlists.len(),
            };
            rebuild_search_index(state);
            refresh_search(state);
            clamp_selections(state);
        }
        BackendEvent::Update(BackendUpdate::LibraryBatch {
            availability,
            playback,
            tracks,
            loaded,
            total,
            complete,
            artists,
            albums,
            recently_added,
        }) => {
            apply_playback_update(state, availability, playback);
            let starts_new_load = loaded == tracks.len();
            if starts_new_load {
                state.library.clear();
                state.artists.clear();
                state.albums.clear();
                state.recently_added.clear();
                state
                    .search_index
                    .retain(|entry| matches!(entry.result, LocalSearchResult::Playlist(_)));
            }
            if !complete {
                state
                    .search_index
                    .extend(tracks.iter().map(track_search_index_entry));
            }
            state.library.extend(tracks);
            state.library_status = if complete {
                crate::domain::CollectionLoadState::Loaded { total }
            } else {
                crate::domain::CollectionLoadState::Loading { loaded, total }
            };
            if complete {
                state.artists = artists;
                state.albums = albums;
                state.recently_added = recently_added;
            }
            if complete {
                rebuild_search_index(state);
            }
            refresh_search(state);
            state.view_status = if state.library.is_empty() && complete {
                ViewStatus::Empty
            } else {
                ViewStatus::Loaded
            };
            clamp_selections(state);
        }
        BackendEvent::Update(BackendUpdate::PlaylistBatch {
            availability,
            playback,
            playlist_id,
            tracks,
            loaded,
            total,
            complete,
        }) => {
            apply_playback_update(state, availability, playback);
            if let Some(playlist) = state
                .playlists
                .iter_mut()
                .find(|playlist| playlist.id == playlist_id)
            {
                if loaded == tracks.len() {
                    playlist.tracks.clear();
                }
                playlist.tracks.extend(tracks);
                playlist.track_count = total;
                playlist.tracks_loaded = complete;
            }
            clamp_selections(state);
        }
        BackendEvent::Error(message) => {
            if matches!(state.backend_status, BackendStatus::Initializing) {
                state.backend_status = BackendStatus::Error {
                    message: message.clone(),
                };
                state.view_status = ViewStatus::Error(message.clone());
            } else {
                state.backend_availability =
                    crate::domain::BackendAvailability::Error(message.clone());
            }
            state.notification = Some(message);
        }
    }
}

fn apply_playback_update(
    state: &mut AppState,
    availability: crate::domain::BackendAvailability,
    playback: crate::domain::PlaybackSnapshot,
) {
    state.backend_availability = availability;
    state.playback = playback;
}

fn refresh_search(state: &mut AppState) {
    let terms = normalized_search_text(&state.search_query)
        .split_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    state.search_results = if terms.is_empty() {
        Vec::new()
    } else {
        state
            .search_index
            .iter()
            .filter(|entry| {
                terms
                    .iter()
                    .all(|term| entry.normalized_text.contains(term))
            })
            .map(|entry| entry.result.clone())
            .collect()
    };
    state.content_selection = state
        .content_selection
        .min(state.search_results.len().saturating_sub(1));
}

fn rebuild_search_index(state: &mut AppState) {
    let mut index = Vec::with_capacity(
        state.library.len() + state.artists.len() + state.albums.len() + state.playlists.len(),
    );
    index.extend(
        state
            .playlists
            .iter()
            .map(|playlist| LocalSearchIndexEntry {
                result: LocalSearchResult::Playlist(playlist.id.clone()),
                normalized_text: normalized_search_text(&format!(
                    "{} {}",
                    playlist.name,
                    playlist.description.as_deref().unwrap_or_default()
                )),
            }),
    );
    index.extend(state.artists.iter().map(|artist| LocalSearchIndexEntry {
        result: LocalSearchResult::Artist(artist.id.clone()),
        normalized_text: normalized_search_text(&artist.name),
    }));
    index.extend(state.albums.iter().map(|album| LocalSearchIndexEntry {
        result: LocalSearchResult::Album(album.id.clone()),
        normalized_text: normalized_search_text(&format!("{} {}", album.title, album.artist)),
    }));
    index.extend(state.library.iter().map(track_search_index_entry));
    state.search_index = index;
}

fn track_search_index_entry(track: &crate::domain::Track) -> LocalSearchIndexEntry {
    LocalSearchIndexEntry {
        result: LocalSearchResult::Track(track.id.clone()),
        normalized_text: normalized_search_text(&format!(
            "{} {} {} {} {} {}",
            track.title,
            track.artist,
            track.album,
            track.metadata.album_artist.as_deref().unwrap_or_default(),
            track.metadata.composer.as_deref().unwrap_or_default(),
            track.metadata.genre.as_deref().unwrap_or_default()
        )),
    }
}

fn normalized_search_text(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn clamp_selections(state: &mut AppState) {
    let content_maximum = content_length(state).saturating_sub(1);
    let queue_maximum = state.queue.len().saturating_sub(1);
    state.content_selection = state.content_selection.min(content_maximum);
    state.queue_selection = state.queue_selection.min(queue_maximum);
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::{
        app::{
            action::{Action, Command},
            state::{AppState, BackendStatus, Focus, LocalSearchResult, Route, Screen, ViewStatus},
        },
        backend::{
            BackendCommand, BackendEvent, BackendUpdate, MusicBackend, capabilities::Capabilities,
            mock::MockMusicBackend,
        },
        domain::{
            Album, AlbumId, Artist, ArtistId, BackendSnapshot, PlaybackSnapshot, PlaybackStatus,
            Playlist, PlaylistId, Track, TrackId,
        },
    };

    use super::reduce;

    #[test]
    fn navigation_updates_sidebar_and_opens_screen() {
        let mut state = AppState::default();
        reduce(&mut state, Action::MoveDown);
        reduce(&mut state, Action::OpenSelected);

        assert_eq!(state.navigation.active, Route::Section(Screen::Browse));
        assert_eq!(state.focus, Focus::Content);
    }

    #[test]
    fn every_sidebar_item_activates_its_corresponding_screen() {
        for (index, expected_screen) in Screen::ALL.into_iter().enumerate() {
            let mut state = AppState {
                sidebar_selection: index,
                ..AppState::default()
            };

            reduce(&mut state, Action::OpenSelected);

            assert_eq!(
                state.navigation.active,
                Route::Section(expected_screen),
                "sidebar index {index} should open {}",
                expected_screen.label()
            );
            assert_eq!(state.sidebar_selection, index);
            assert_eq!(state.focus, Focus::Content);
            assert_eq!(state.content_selection, 0);
        }
    }

    #[tokio::test]
    async fn artist_album_and_playlist_details_preserve_ids_and_back_history() {
        let cases = [
            (
                Screen::Artists,
                Route::ArtistDetail {
                    artist_id: ArtistId::new("mock-artist-asyncs"),
                },
            ),
            (
                Screen::Albums,
                Route::AlbumDetail {
                    album_id: AlbumId::new("mock-album-event-loop"),
                },
            ),
            (
                Screen::Playlists,
                Route::PlaylistDetail {
                    playlist_id: PlaylistId::new("mock-playlist-terminal-focus"),
                },
            ),
        ];

        for (screen, expected_detail) in cases {
            let mut state = loaded_mock_state().await;
            reduce(&mut state, Action::GoTo(screen));

            reduce(&mut state, Action::OpenSelected);

            assert_eq!(state.navigation.active, expected_detail);
            assert_eq!(state.navigation.history.len(), 1);
            assert_eq!(state.content_selection, 0);

            reduce(&mut state, Action::MoveDown);
            assert_eq!(state.content_selection, 1);
            reduce(&mut state, Action::Back);

            assert_eq!(state.navigation.active, Route::Section(screen));
            assert_eq!(state.content_selection, 0);
            assert!(state.navigation.history.is_empty());
        }
    }

    #[tokio::test]
    async fn selected_song_emits_play_track_command() {
        let mut state = loaded_mock_state().await;
        reduce(&mut state, Action::GoTo(Screen::Songs));
        reduce(&mut state, Action::MoveDown);

        let commands = reduce(&mut state, Action::OpenSelected);

        assert_eq!(
            commands,
            vec![Command::Backend(BackendCommand::PlayTrack(TrackId::new(
                "mock-002"
            )))]
        );
    }

    #[test]
    fn selected_playlist_opens_as_a_detail_route_and_back_restores_the_list() {
        let mut state = AppState {
            playlists: vec![
                playlist("playlist-one", "One"),
                playlist("playlist-two", "Two"),
            ],
            ..AppState::default()
        };
        state.sidebar_selection = Screen::ALL
            .iter()
            .position(|screen| *screen == Screen::Playlists)
            .expect("Playlists is a sidebar screen");

        reduce(&mut state, Action::OpenSelected);
        assert_eq!(state.navigation.active, Route::Section(Screen::Playlists));

        reduce(&mut state, Action::MoveDown);
        reduce(&mut state, Action::OpenSelected);

        assert_eq!(
            state.navigation.active,
            Route::PlaylistDetail {
                playlist_id: PlaylistId::new("playlist-two")
            }
        );
        assert_eq!(state.navigation.history.len(), 1);
        assert_eq!(state.content_selection, 0);

        reduce(&mut state, Action::MoveDown);
        assert_eq!(state.content_selection, 1);
        reduce(&mut state, Action::Back);

        assert_eq!(state.navigation.active, Route::Section(Screen::Playlists));
        assert_eq!(state.content_selection, 1);
        assert!(state.navigation.history.is_empty());
    }

    #[test]
    fn opening_unloaded_local_playlist_requests_lazy_contents() {
        let playlist_id = PlaylistId::new("musicapp:playlist:persistent:ABC");
        let mut state = AppState {
            navigation: crate::app::state::NavigationState {
                active: Route::Section(Screen::Playlists),
                history: Vec::new(),
            },
            focus: Focus::Content,
            capabilities: Capabilities::macos(),
            playlists: vec![crate::domain::Playlist::unloaded(
                playlist_id.to_string(),
                "Local Playlist",
                None,
                crate::domain::PlaylistKind::User,
                None,
            )],
            ..AppState::default()
        };

        let commands = reduce(&mut state, Action::OpenSelected);

        assert_eq!(
            state.navigation.active,
            Route::PlaylistDetail {
                playlist_id: playlist_id.clone()
            }
        );
        assert_eq!(
            commands,
            vec![Command::Backend(BackendCommand::LoadPlaylist(playlist_id))]
        );
    }

    #[test]
    fn progressive_library_batches_append_then_install_derived_views() {
        let mut state = AppState::default();
        let first = Track::new("one", "One", "Artist", "Album", Duration::from_secs(60));
        let second = Track::new("two", "Two", "Artist", "Album", Duration::from_secs(70));
        reduce(
            &mut state,
            Action::Backend(Box::new(BackendEvent::Update(
                BackendUpdate::LibraryBatch {
                    availability: crate::domain::BackendAvailability::Available,
                    playback: PlaybackSnapshot::default(),
                    tracks: vec![first],
                    loaded: 1,
                    total: 2,
                    complete: false,
                    artists: Vec::new(),
                    albums: Vec::new(),
                    recently_added: Vec::new(),
                },
            ))),
        );
        assert_eq!(state.library.len(), 1);
        assert_eq!(state.search_index.len(), 1);
        assert_eq!(
            state.library_status,
            crate::domain::CollectionLoadState::Loading {
                loaded: 1,
                total: 2
            }
        );

        let album = crate::domain::Album::new(
            "album",
            "Album",
            "Artist",
            2026,
            "2026-01-01",
            vec![second.clone()],
        );
        reduce(
            &mut state,
            Action::Backend(Box::new(BackendEvent::Update(
                BackendUpdate::LibraryBatch {
                    availability: crate::domain::BackendAvailability::Available,
                    playback: PlaybackSnapshot::default(),
                    tracks: vec![second],
                    loaded: 2,
                    total: 2,
                    complete: true,
                    artists: Vec::new(),
                    albums: vec![album.clone()],
                    recently_added: vec![album],
                },
            ))),
        );
        assert_eq!(state.library.len(), 2);
        assert_eq!(state.albums.len(), 1);
        assert_eq!(state.recently_added.len(), 1);
        assert_eq!(state.search_index.len(), 3);
        assert_eq!(
            state.library_status,
            crate::domain::CollectionLoadState::Loaded { total: 2 }
        );
    }

    #[test]
    fn local_search_uses_one_normalized_index_for_all_local_collection_types() {
        let track = Track::new(
            "track-beacon",
            "Beacon Song",
            "Beacon Artist",
            "Beacon Album",
            Duration::from_secs(180),
        );
        let album = Album::new(
            "album-beacon",
            "Beacon Album",
            "Beacon Artist",
            2026,
            "2026-08-03",
            vec![track.clone()],
        );
        let artist = Artist::new(
            "artist-beacon",
            "Beacon Artist",
            vec![album.id.clone()],
            vec![track.id.clone()],
        );
        let playlist = crate::domain::Playlist::unloaded(
            "playlist-beacon",
            "Beacon Playlist",
            Some("A local Beacon collection".to_owned()),
            crate::domain::PlaylistKind::User,
            None,
        );
        let mut state = AppState::default();
        reduce(
            &mut state,
            Action::Backend(Box::new(BackendEvent::Ready {
                name: "Music.app",
                capabilities: Capabilities::macos(),
                snapshot: BackendSnapshot {
                    library: vec![track],
                    artists: vec![artist],
                    albums: vec![album],
                    playlists: vec![playlist],
                    ..BackendSnapshot::default()
                },
            })),
        );

        reduce(&mut state, Action::StartSearch);
        for character in "  BEACON  ".chars() {
            reduce(&mut state, Action::SearchInput(character));
        }

        assert_eq!(
            state.search_results,
            vec![
                LocalSearchResult::Playlist(PlaylistId::new("playlist-beacon")),
                LocalSearchResult::Artist(ArtistId::new("artist-beacon")),
                LocalSearchResult::Album(AlbumId::new("album-beacon")),
                LocalSearchResult::Track(TrackId::new("track-beacon")),
            ]
        );
        assert_eq!(state.search_index.len(), 4);
    }

    #[test]
    fn opening_playlist_search_result_preserves_identity_and_requests_lazy_contents() {
        let playlist_id = PlaylistId::new("musicapp:playlist:persistent:SEARCH");
        let mut state = AppState {
            navigation: crate::app::state::NavigationState {
                active: Route::Section(Screen::Search),
                history: Vec::new(),
            },
            focus: Focus::Content,
            capabilities: Capabilities::macos(),
            search_results: vec![LocalSearchResult::Playlist(playlist_id.clone())],
            playlists: vec![crate::domain::Playlist::unloaded(
                playlist_id.to_string(),
                "Search Result",
                None,
                crate::domain::PlaylistKind::User,
                None,
            )],
            ..AppState::default()
        };

        assert_eq!(
            reduce(&mut state, Action::OpenSelected),
            vec![Command::Backend(BackendCommand::LoadPlaylist(
                playlist_id.clone()
            ))]
        );
        assert_eq!(
            state.navigation.active,
            Route::PlaylistDetail {
                playlist_id: playlist_id.clone()
            }
        );
        assert_eq!(
            state.navigation.history,
            vec![crate::app::state::NavigationEntry {
                route: Route::Section(Screen::Search),
                content_selection: 0,
            }]
        );
    }

    #[test]
    fn play_selected_playlist_uses_selection_playback_capability() {
        let playlist_id = PlaylistId::new("musicapp:playlist:persistent:ABC");
        let mut state = AppState {
            navigation: crate::app::state::NavigationState {
                active: Route::Section(Screen::Playlists),
                history: Vec::new(),
            },
            focus: Focus::Content,
            capabilities: Capabilities::macos(),
            playlists: vec![crate::domain::Playlist::unloaded(
                playlist_id.to_string(),
                "Local Playlist",
                None,
                crate::domain::PlaylistKind::User,
                None,
            )],
            ..AppState::default()
        };

        assert_eq!(
            reduce(&mut state, Action::PlaySelectedPlaylist),
            vec![Command::Backend(BackendCommand::PlayPlaylist(playlist_id))]
        );
    }

    #[test]
    fn playlist_detail_plays_exact_track_with_playlist_context() {
        let playlist_id = PlaylistId::new("musicapp:playlist:persistent:ABC");
        let track_id = TrackId::new("musicapp:persistent:TRACK");
        let mut playlist = Playlist::new(
            playlist_id.to_string(),
            "Local Playlist",
            None,
            vec![Track::new(
                track_id.to_string(),
                "Track",
                "Artist",
                "Album",
                Duration::from_secs(60),
            )],
        );
        playlist.id = playlist_id.clone();
        let mut state = AppState {
            navigation: crate::app::state::NavigationState {
                active: Route::PlaylistDetail {
                    playlist_id: playlist_id.clone(),
                },
                history: Vec::new(),
            },
            focus: Focus::Content,
            capabilities: Capabilities::macos(),
            playlists: vec![playlist],
            ..AppState::default()
        };

        assert_eq!(
            reduce(&mut state, Action::OpenSelected),
            vec![Command::Backend(BackendCommand::PlayPlaylistTrack {
                playlist_id,
                track_id,
            })]
        );
    }

    #[test]
    fn focus_left_returns_from_playlist_detail() {
        let mut state = AppState {
            navigation: crate::app::state::NavigationState {
                active: Route::PlaylistDetail {
                    playlist_id: PlaylistId::new("playlist-one"),
                },
                history: vec![crate::app::state::NavigationEntry {
                    route: Route::Section(Screen::Playlists),
                    content_selection: 0,
                }],
            },
            focus: Focus::Content,
            ..AppState::default()
        };

        reduce(&mut state, Action::FocusLeft);

        assert_eq!(state.navigation.active, Route::Section(Screen::Playlists));
        assert_eq!(state.focus, Focus::Content);
    }

    #[test]
    fn quit_sets_state_without_emitting_commands() {
        let mut state = AppState::default();
        let commands = reduce(&mut state, Action::Quit);

        assert!(state.should_quit);
        assert!(commands.is_empty());
    }

    #[test]
    fn unsupported_actions_do_not_emit_commands() {
        let mut state = AppState::default();
        let commands = reduce(&mut state, Action::PlayPause);

        assert!(commands.is_empty());
        assert!(state.notification.is_some());
    }

    #[test]
    fn ready_backend_enables_semantic_playback_command() {
        let mut state = AppState::default();
        reduce(
            &mut state,
            Action::Backend(Box::new(BackendEvent::Ready {
                name: "Mock Music",
                capabilities: Capabilities::mock(),
                snapshot: BackendSnapshot::default(),
            })),
        );

        assert_eq!(
            reduce(&mut state, Action::Play),
            vec![Command::Backend(BackendCommand::Play)]
        );
        assert_eq!(
            reduce(&mut state, Action::Pause),
            vec![Command::Backend(BackendCommand::Pause)]
        );
        assert_eq!(
            reduce(&mut state, Action::PlayPause),
            vec![Command::Backend(BackendCommand::PlayPause)]
        );
    }

    #[test]
    fn queue_focus_is_only_reachable_when_the_queue_pane_is_visible() {
        let mut state = AppState {
            capabilities: Capabilities::mock(),
            ..AppState::default()
        };
        reduce(&mut state, Action::FocusRight);
        reduce(&mut state, Action::FocusRight);
        assert_eq!(state.focus, Focus::Content);

        reduce(
            &mut state,
            Action::Resize {
                width: 120,
                height: 30,
            },
        );
        reduce(&mut state, Action::FocusRight);
        assert_eq!(state.focus, Focus::Queue);

        reduce(
            &mut state,
            Action::Resize {
                width: 80,
                height: 24,
            },
        );
        assert_eq!(state.focus, Focus::Content);
    }

    #[test]
    fn startup_backend_error_becomes_a_view_error() {
        let mut state = AppState::default();
        reduce(
            &mut state,
            Action::Backend(Box::new(BackendEvent::Error("offline".to_owned()))),
        );

        assert_eq!(
            state.backend_status,
            BackendStatus::Error {
                message: "offline".to_owned()
            }
        );
        assert_eq!(state.view_status, ViewStatus::Error("offline".to_owned()));
    }

    #[test]
    fn backend_snapshot_replaces_stale_playback_metadata_and_position() {
        let old_track = Track::new(
            "old-track",
            "Old Title",
            "Old Artist",
            "Old Album",
            Duration::from_secs(180),
        );
        let new_track = Track::new(
            "new-track",
            "New Title",
            "New Artist",
            "New Album",
            Duration::from_secs(240),
        );
        let mut state = AppState {
            playback: PlaybackSnapshot {
                status: PlaybackStatus::Paused,
                current_track: Some(old_track),
                position: Duration::from_secs(10),
                ..PlaybackSnapshot::default()
            },
            ..AppState::default()
        };

        reduce(
            &mut state,
            Action::Backend(Box::new(BackendEvent::Update(BackendUpdate::Snapshot(
                BackendSnapshot {
                    playback: PlaybackSnapshot {
                        status: PlaybackStatus::Playing,
                        current_track: Some(new_track),
                        position: Duration::from_millis(12_500),
                        ..PlaybackSnapshot::default()
                    },
                    ..BackendSnapshot::default()
                },
            )))),
        );

        assert_eq!(state.playback.status, PlaybackStatus::Playing);
        assert_eq!(state.playback.position, Duration::from_millis(12_500));
        let track = state.playback.current_track.expect("updated track");
        assert_eq!(track.title, "New Title");
        assert_eq!(track.artist, "New Artist");
        assert_eq!(track.album, "New Album");
    }

    #[test]
    fn initial_backend_event_populates_real_playback_state() {
        let track = Track::new(
            "initial-track",
            "Initial Title",
            "Initial Artist",
            "Initial Album",
            Duration::from_secs(200),
        );
        let mut state = AppState::default();

        reduce(
            &mut state,
            Action::Backend(Box::new(BackendEvent::Ready {
                name: "Music.app",
                capabilities: Capabilities::macos(),
                snapshot: BackendSnapshot {
                    playback: PlaybackSnapshot {
                        status: PlaybackStatus::Playing,
                        current_track: Some(track),
                        position: Duration::from_millis(12_500),
                        ..PlaybackSnapshot::default()
                    },
                    ..BackendSnapshot::default()
                },
            })),
        );

        assert_eq!(state.playback.status, PlaybackStatus::Playing);
        assert_eq!(state.playback.position, Duration::from_millis(12_500));
        let track = state.playback.current_track.expect("initial track");
        assert_eq!(track.title, "Initial Title");
        assert_eq!(track.artist, "Initial Artist");
        assert_eq!(track.album, "Initial Album");
    }

    #[test]
    fn post_startup_backend_error_clears_connected_health() {
        let mut state = AppState {
            backend_status: BackendStatus::Ready {
                name: "Music.app".to_owned(),
            },
            ..AppState::default()
        };

        reduce(
            &mut state,
            Action::Backend(Box::new(BackendEvent::Error(
                "Backend worker stopped unexpectedly".to_owned(),
            ))),
        );

        assert_eq!(
            state.backend_availability,
            crate::domain::BackendAvailability::Error(
                "Backend worker stopped unexpectedly".to_owned()
            )
        );
    }

    fn playlist(id: &str, name: &str) -> Playlist {
        Playlist::new(
            id,
            name,
            Some(format!("{name} description")),
            vec![
                Track::new(
                    format!("{id}-track-one"),
                    "Track One",
                    "Artist",
                    "Album",
                    Duration::from_secs(180),
                ),
                Track::new(
                    format!("{id}-track-two"),
                    "Track Two",
                    "Artist",
                    "Album",
                    Duration::from_secs(210),
                ),
            ],
        )
    }

    async fn loaded_mock_state() -> AppState {
        let mut backend = MockMusicBackend::new();
        let snapshot = backend.snapshot().await.expect("mock snapshot");
        let mut state = AppState::default();
        reduce(
            &mut state,
            Action::Backend(Box::new(BackendEvent::Ready {
                name: backend.name(),
                capabilities: backend.capabilities(),
                snapshot,
            })),
        );
        state
    }
}
