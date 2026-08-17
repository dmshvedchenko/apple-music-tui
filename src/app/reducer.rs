use std::cmp::Ordering;
use std::collections::HashMap;

use crate::backend::{BackendCommand, BackendEvent, BackendUpdate, capabilities::Capability};
use crate::domain::{
    ArtworkKey, ArtworkResult, PlaylistHierarchy, PlaylistId, PlaylistKind, PlaylistLoadState,
    VisiblePlaylistEntry,
};

use super::{
    action::{Action, Command},
    state::{
        ActionMenuState, AppState, ArtworkCacheEntry, BackendStatus, CollectionKind,
        CollectionSort, CollectionViewState, ContextAction, ContextTarget, FilterEditorState,
        Focus, LocalSearchIndexEntry, LocalSearchResult, NavigationEntry,
        PlaylistTrackRemovalConfirmation, QUEUE_PANE_MIN_WIDTH, RenderableArtworkCacheEntry, Route,
        Screen, SortMenuState, ViewStatus,
    },
};

pub fn reduce(state: &mut AppState, action: Action) -> Vec<Command> {
    if state.playlist_track_removal_confirmation.is_some() {
        match action {
            Action::Quit | Action::Back => {
                state.playlist_track_removal_confirmation = None;
                return Vec::new();
            }
            Action::OpenSelected
            | Action::ConfirmPlaylistTrackRemoval
            | Action::RequestPlaylistTrackRemoval => {
                let Some(confirmation) = state.playlist_track_removal_confirmation.take() else {
                    return Vec::new();
                };
                state.playlist_track_removal_in_flight = true;
                return backend_command(
                    state,
                    Capability::PlaylistTrackRemove,
                    BackendCommand::RemovePlaylistTrack {
                        playlist_id: confirmation.playlist_id,
                        index: confirmation.index,
                        expected_track_id: confirmation.track_id,
                    },
                );
            }
            Action::Resize { width, height } => {
                state.terminal_size = (width, height);
                return Vec::new();
            }
            Action::Backend(_)
            | Action::ArtworkConversionCompleted { .. }
            | Action::RetryArtwork { .. } => {}
            _ => return Vec::new(),
        }
    }
    if state.sort_menu.is_some() {
        match action {
            Action::Quit | Action::Back | Action::OpenCollectionSort => {
                state.sort_menu = None;
                return Vec::new();
            }
            Action::MoveUp | Action::PageUp => {
                move_sort_menu_selection(state, false);
                return Vec::new();
            }
            Action::MoveDown | Action::PageDown => {
                move_sort_menu_selection(state, true);
                return Vec::new();
            }
            Action::OpenSelected => {
                choose_sort_field(state);
                return Vec::new();
            }
            Action::ToggleCollectionSortDirection | Action::PlayPause => {
                toggle_collection_sort_direction(state);
                return Vec::new();
            }
            Action::Resize { width, height } => {
                state.terminal_size = (width, height);
                return Vec::new();
            }
            Action::Backend(_)
            | Action::ArtworkConversionCompleted { .. }
            | Action::RetryArtwork { .. } => {}
            _ => return Vec::new(),
        }
    }
    if state.filter_editor.is_some() {
        match action {
            Action::Quit => {
                state.should_quit = true;
                state.stop_playback_on_exit = true;
                return Vec::new();
            }
            Action::Back => {
                cancel_collection_filter(state);
                return Vec::new();
            }
            Action::CollectionFilterInput(character) => {
                update_collection_filter_draft(state, |draft| draft.push(character));
                return Vec::new();
            }
            Action::CollectionFilterBackspace => {
                update_collection_filter_draft(state, |draft| {
                    draft.pop();
                });
                return Vec::new();
            }
            Action::ClearCollectionFilter => {
                update_collection_filter_draft(state, String::clear);
                return Vec::new();
            }
            Action::SubmitCollectionFilter => {
                commit_collection_filter(state);
                return Vec::new();
            }
            Action::Resize { width, height } => {
                state.terminal_size = (width, height);
                return Vec::new();
            }
            Action::Backend(_)
            | Action::ArtworkConversionCompleted { .. }
            | Action::RetryArtwork { .. } => {}
            _ => return Vec::new(),
        }
    }
    if state.action_menu.is_some() {
        match &action {
            Action::Quit | Action::Back | Action::OpenActions => {
                state.action_menu = None;
                return Vec::new();
            }
            Action::MoveUp | Action::PageUp => {
                move_action_menu_selection(state, false);
                return Vec::new();
            }
            Action::MoveDown | Action::PageDown => {
                move_action_menu_selection(state, true);
                return Vec::new();
            }
            Action::OpenSelected => return execute_selected_action_menu(state),
            Action::Resize { width, height } => {
                state.terminal_size = (*width, *height);
                return Vec::new();
            }
            Action::Backend(_)
            | Action::ArtworkConversionCompleted { .. }
            | Action::RetryArtwork { .. } => {}
            _ => return Vec::new(),
        }
    }
    if state.help_open {
        match action {
            Action::Quit | Action::ToggleHelp | Action::Back => {
                state.help_open = false;
                state.help_scroll = 0;
                return Vec::new();
            }
            Action::MoveUp | Action::PageUp => {
                state.help_scroll = state.help_scroll.saturating_sub(1);
                return Vec::new();
            }
            Action::MoveDown | Action::PageDown => {
                state.help_scroll = state.help_scroll.saturating_add(1);
                return Vec::new();
            }
            Action::JumpToStart => {
                state.help_scroll = 0;
                return Vec::new();
            }
            Action::JumpToEnd => return Vec::new(),
            Action::Resize { width, height } => {
                state.terminal_size = (width, height);
                return Vec::new();
            }
            Action::Backend(_)
            | Action::ArtworkConversionCompleted { .. }
            | Action::RetryArtwork { .. } => {}
            _ => return Vec::new(),
        }
    }

    match action {
        Action::Quit if matches!(state.navigation.active, Route::NowPlaying) => {
            navigate_back(state);
        }
        Action::Quit => {
            state.should_quit = true;
            state.stop_playback_on_exit = true;
        }
        Action::InputClosed => state.should_quit = true,
        Action::RequestPlaylistTrackRemoval => return request_playlist_track_removal(state),
        Action::ConfirmPlaylistTrackRemoval => {}
        Action::OpenNowPlaying => {
            if !matches!(state.navigation.active, Route::NowPlaying) {
                open_detail(state, Route::NowPlaying);
            }
            if let Some(track) = state.playback.current_track.as_ref() {
                let track_id = track.id.clone();
                let key = state.artwork_key_for_track(&track_id);
                return request_track_artwork(state, key, track_id);
            }
        }
        Action::RefreshLibrary => {
            return vec![Command::Backend(BackendCommand::RefreshLibrary)];
        }
        Action::OpenPlayer => {
            return backend_command(state, Capability::Launch, BackendCommand::OpenPlayer);
        }
        Action::MoveUp => move_selection(state, false),
        Action::MoveDown => move_selection(state, true),
        Action::JumpToStart => jump_selection(state, false),
        Action::JumpToEnd => jump_selection(state, true),
        Action::JumpToPlayingTrack => jump_to_playing_playlist_track(state),
        Action::PageUp => page_selection(state, false),
        Action::PageDown => page_selection(state, true),
        Action::FocusLeft => {
            if matches!(
                &state.navigation.active,
                Route::ArtistDetail { .. }
                    | Route::AlbumDetail { .. }
                    | Route::PlaylistDetail { .. }
                    | Route::NowPlaying
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
        Action::PlaySelectedCollection => return play_selected_collection(state),
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
        Action::ToggleHelp => {
            state.help_open = true;
            state.help_scroll = 0;
        }
        Action::OpenActions => open_action_menu(state),
        Action::OpenCollectionSort => open_collection_sort(state),
        Action::ToggleCollectionSortDirection => toggle_current_collection_sort_direction(state),
        Action::StartCollectionFilter => start_collection_filter(state),
        Action::ClearCollectionFilter => clear_current_collection_filter(state),
        Action::CollectionFilterInput(_)
        | Action::CollectionFilterBackspace
        | Action::SubmitCollectionFilter => {}
        Action::Back => {
            if state.search_input_active {
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
        Action::Backend(event) => {
            let previous_track = state
                .playback
                .current_track
                .as_ref()
                .map(|track| track.id.clone());
            let commands = apply_backend_event(state, *event);
            let current_track = state
                .playback
                .current_track
                .as_ref()
                .map(|track| track.id.clone());
            if previous_track != current_track
                && let Some(track_id) = current_track
            {
                let key = state.artwork_key_for_track(&track_id);
                return request_track_artwork(state, key, track_id);
            }
            return commands;
        }
        Action::ArtworkConversionCompleted {
            key,
            source_fingerprint,
            result,
        } => apply_artwork_conversion(state, key, source_fingerprint, result),
        Action::RetryArtwork { key, track_id } => {
            if artwork_key_is_current(state, &key)
                && matches!(
                    state.artwork_cache.get(&key),
                    Some(ArtworkCacheEntry::Loading)
                )
            {
                tracing::debug!(key = ?key, track_id = %track_id, retry_attempt = 1, "retrying transient Music.app artwork query");
                return vec![Command::Backend(BackendCommand::LoadTrackArtwork {
                    key,
                    track_id,
                })];
            }
        }
    }

    Vec::new()
}

fn active_collection(state: &AppState) -> Option<CollectionKind> {
    match state.navigation.active {
        Route::Section(Screen::Songs) => Some(CollectionKind::Songs),
        Route::Section(Screen::Albums) => Some(CollectionKind::Albums),
        Route::Section(Screen::Artists) => Some(CollectionKind::Artists),
        _ => None,
    }
}

fn collection_view(state: &AppState, collection: CollectionKind) -> &CollectionViewState {
    match collection {
        CollectionKind::Songs => &state.library_views.songs,
        CollectionKind::Albums => &state.library_views.albums,
        CollectionKind::Artists => &state.library_views.artists,
    }
}

fn collection_view_mut(
    state: &mut AppState,
    collection: CollectionKind,
) -> &mut CollectionViewState {
    match collection {
        CollectionKind::Songs => &mut state.library_views.songs,
        CollectionKind::Albums => &mut state.library_views.albums,
        CollectionKind::Artists => &mut state.library_views.artists,
    }
}

fn sort_fields(collection: CollectionKind) -> &'static [CollectionSort] {
    match collection {
        CollectionKind::Songs => &[
            CollectionSort::SongTitle,
            CollectionSort::SongArtist,
            CollectionSort::SongAlbum,
            CollectionSort::SongDateAdded,
            CollectionSort::SongYear,
            CollectionSort::SongPlayCount,
        ],
        CollectionKind::Albums => &[
            CollectionSort::AlbumTitle,
            CollectionSort::AlbumArtist,
            CollectionSort::AlbumYear,
            CollectionSort::AlbumRecentlyAdded,
        ],
        CollectionKind::Artists => &[
            CollectionSort::ArtistName,
            CollectionSort::ArtistAlbumCount,
            CollectionSort::ArtistTrackCount,
        ],
    }
}

fn open_collection_sort(state: &mut AppState) {
    let Some(collection) = active_collection(state) else {
        state.notification = Some("Sorting is available in Songs, Albums, and Artists".to_owned());
        return;
    };
    let selection = sort_fields(collection)
        .iter()
        .position(|field| *field == collection_view(state, collection).sort)
        .unwrap_or_default();
    state.sort_menu = Some(SortMenuState {
        collection,
        selection,
    });
}

fn move_sort_menu_selection(state: &mut AppState, down: bool) {
    let Some(menu) = state.sort_menu.as_mut() else {
        return;
    };
    let length = sort_fields(menu.collection).len();
    if down {
        menu.selection = (menu.selection + 1).min(length.saturating_sub(1));
    } else {
        menu.selection = menu.selection.saturating_sub(1);
    }
}

fn choose_sort_field(state: &mut AppState) {
    let Some(menu) = state.sort_menu.take() else {
        return;
    };
    let Some(field) = sort_fields(menu.collection).get(menu.selection).copied() else {
        return;
    };
    let view = collection_view_mut(state, menu.collection);
    if view.sort == field {
        view.descending = !view.descending;
    } else {
        view.sort = field;
        view.descending = false;
    }
    rebuild_collection_view(state, menu.collection);
}

fn toggle_collection_sort_direction(state: &mut AppState) {
    let Some(menu) = state.sort_menu.as_ref() else {
        return;
    };
    let collection = menu.collection;
    collection_view_mut(state, collection).descending =
        !collection_view(state, collection).descending;
    rebuild_collection_view(state, collection);
}

fn toggle_current_collection_sort_direction(state: &mut AppState) {
    let Some(collection) = active_collection(state) else {
        return;
    };
    let descending = collection_view(state, collection).descending;
    collection_view_mut(state, collection).descending = !descending;
    rebuild_collection_view(state, collection);
}

fn start_collection_filter(state: &mut AppState) {
    let Some(collection) = active_collection(state) else {
        state.notification =
            Some("Filtering is available in Songs, Albums, and Artists".to_owned());
        return;
    };
    let current = collection_view(state, collection).filter.clone();
    state.filter_editor = Some(FilterEditorState {
        collection,
        original: current.clone(),
        draft: current,
    });
}

fn update_collection_filter_draft(state: &mut AppState, update: impl FnOnce(&mut String)) {
    let Some(editor) = state.filter_editor.as_mut() else {
        return;
    };
    update(&mut editor.draft);
    let collection = editor.collection;
    rebuild_collection_view(state, collection);
}

fn commit_collection_filter(state: &mut AppState) {
    let Some(editor) = state.filter_editor.take() else {
        return;
    };
    collection_view_mut(state, editor.collection).filter = editor.draft;
    rebuild_collection_view(state, editor.collection);
}

fn cancel_collection_filter(state: &mut AppState) {
    let Some(editor) = state.filter_editor.take() else {
        return;
    };
    collection_view_mut(state, editor.collection).filter = editor.original;
    rebuild_collection_view(state, editor.collection);
}

fn clear_current_collection_filter(state: &mut AppState) {
    let Some(collection) = active_collection(state) else {
        return;
    };
    collection_view_mut(state, collection).filter.clear();
    rebuild_collection_view(state, collection);
}

fn collection_filter_text(state: &AppState, collection: CollectionKind) -> &str {
    state
        .filter_editor
        .as_ref()
        .filter(|editor| editor.collection == collection)
        .map_or_else(
            || collection_view(state, collection).filter.as_str(),
            |editor| editor.draft.as_str(),
        )
}

fn rebuild_library_views(state: &mut AppState) {
    for collection in [
        CollectionKind::Songs,
        CollectionKind::Albums,
        CollectionKind::Artists,
    ] {
        rebuild_collection_view(state, collection);
    }
}

fn rebuild_collection_view(state: &mut AppState, collection: CollectionKind) {
    let rebuild_started = std::time::Instant::now();
    let selected_id = selected_collection_identity(state, collection);
    let old_selection = state.content_selection;
    let filter = normalized_search_text(collection_filter_text(state, collection));
    let terms = filter.split_whitespace().collect::<Vec<_>>();
    let (indices, keys) = match collection {
        CollectionKind::Songs => state
            .library
            .iter()
            .enumerate()
            .filter_map(|(index, track)| {
                let key = normalized_search_text(&format!(
                    "{} {} {} {} {}",
                    track.title,
                    track.artist,
                    track.album,
                    track.metadata.album_artist.as_deref().unwrap_or_default(),
                    track.metadata.genre.as_deref().unwrap_or_default()
                ));
                terms
                    .iter()
                    .all(|term| key.contains(term))
                    .then_some((index, key))
            })
            .unzip(),
        CollectionKind::Albums => state
            .albums
            .iter()
            .enumerate()
            .filter_map(|(index, album)| {
                let key = normalized_search_text(&format!("{} {}", album.title, album.artist));
                terms
                    .iter()
                    .all(|term| key.contains(term))
                    .then_some((index, key))
            })
            .unzip(),
        CollectionKind::Artists => state
            .artists
            .iter()
            .enumerate()
            .filter_map(|(index, artist)| {
                let key = normalized_search_text(&artist.name);
                terms
                    .iter()
                    .all(|term| key.contains(term))
                    .then_some((index, key))
            })
            .unzip(),
    };
    let descending = collection_view(state, collection).descending;
    let sort = collection_view(state, collection).sort;
    let mut indices: Vec<usize> = indices;
    indices.sort_by(|left, right| {
        compare_collection_indices(state, collection, sort, *left, *right, descending)
    });
    let source_len = match collection {
        CollectionKind::Songs => state.library.len(),
        CollectionKind::Albums => state.albums.len(),
        CollectionKind::Artists => state.artists.len(),
    };
    let view = collection_view_mut(state, collection);
    view.indices = indices;
    view.normalized_filter_keys = keys;
    view.source_len = Some(source_len);
    view.rebuild_count += 1;
    if active_collection(state) == Some(collection) && state.focus == Focus::Content {
        state.content_selection = selected_id
            .and_then(|identity| {
                collection_view(state, collection)
                    .indices
                    .iter()
                    .position(|index| collection_identity_at(state, collection, *index) == identity)
            })
            .unwrap_or_else(|| {
                old_selection.min(
                    collection_view(state, collection)
                        .indices
                        .len()
                        .saturating_sub(1),
                )
            });
    }
    tracing::debug!(
        collection = collection.label(),
        entries = collection_view(state, collection).indices.len(),
        rebuild_ms = rebuild_started.elapsed().as_secs_f64() * 1_000.0,
        "local collection view rebuild timing"
    );
}

fn collection_identity_at(state: &AppState, collection: CollectionKind, index: usize) -> String {
    match collection {
        CollectionKind::Songs => state.library[index].id.to_string(),
        CollectionKind::Albums => state.albums[index].id.to_string(),
        CollectionKind::Artists => state.artists[index].id.to_string(),
    }
}

fn selected_collection_identity(state: &AppState, collection: CollectionKind) -> Option<String> {
    if active_collection(state) != Some(collection) {
        return None;
    }
    selected_collection_source_index(state, collection)
        .map(|index| collection_identity_at(state, collection, index))
}

fn selected_collection_source_index(state: &AppState, collection: CollectionKind) -> Option<usize> {
    let view = collection_view(state, collection);
    if view.source_len.is_some() {
        view.indices.get(state.content_selection).copied()
    } else {
        Some(state.content_selection)
    }
}

fn compare_collection_indices(
    state: &AppState,
    collection: CollectionKind,
    sort: CollectionSort,
    left: usize,
    right: usize,
    descending: bool,
) -> Ordering {
    let primary = match collection {
        CollectionKind::Songs => {
            let l = &state.library[left];
            let r = &state.library[right];
            match sort {
                CollectionSort::SongTitle => {
                    normalized_search_text(&l.title).cmp(&normalized_search_text(&r.title))
                }
                CollectionSort::SongArtist => {
                    normalized_search_text(&l.artist).cmp(&normalized_search_text(&r.artist))
                }
                CollectionSort::SongAlbum => {
                    normalized_search_text(&l.album).cmp(&normalized_search_text(&r.album))
                }
                CollectionSort::SongDateAdded => l.metadata.date_added.cmp(&r.metadata.date_added),
                CollectionSort::SongYear => l.metadata.year.cmp(&r.metadata.year),
                CollectionSort::SongPlayCount => l.metadata.play_count.cmp(&r.metadata.play_count),
                _ => Ordering::Equal,
            }
        }
        CollectionKind::Albums => {
            let l = &state.albums[left];
            let r = &state.albums[right];
            match sort {
                CollectionSort::AlbumTitle => {
                    normalized_search_text(&l.title).cmp(&normalized_search_text(&r.title))
                }
                CollectionSort::AlbumArtist => {
                    normalized_search_text(&l.artist).cmp(&normalized_search_text(&r.artist))
                }
                CollectionSort::AlbumYear => l.year.cmp(&r.year),
                CollectionSort::AlbumRecentlyAdded => l.added_date.cmp(&r.added_date),
                _ => Ordering::Equal,
            }
        }
        CollectionKind::Artists => {
            let l = &state.artists[left];
            let r = &state.artists[right];
            match sort {
                CollectionSort::ArtistName => {
                    normalized_search_text(&l.name).cmp(&normalized_search_text(&r.name))
                }
                CollectionSort::ArtistAlbumCount => l.album_ids.len().cmp(&r.album_ids.len()),
                CollectionSort::ArtistTrackCount => {
                    l.top_track_ids.len().cmp(&r.top_track_ids.len())
                }
                _ => Ordering::Equal,
            }
        }
    };
    let primary = if descending {
        primary.reverse()
    } else {
        primary
    };
    primary.then_with(|| {
        collection_identity_at(state, collection, left)
            .cmp(&collection_identity_at(state, collection, right))
    })
}

fn open_action_menu(state: &mut AppState) {
    let Some(target) = selected_context_target(state) else {
        state.notification = Some("No supported actions for this selection".to_owned());
        return;
    };
    let actions = actions_for_target(state, &target);
    if actions.is_empty() {
        state.notification = Some("No supported actions for this selection".to_owned());
        return;
    }
    state.action_menu = Some(ActionMenuState {
        target,
        actions,
        selection: 0,
    });
}

/// Select the canonical playlist occurrence published by the active playback session.
///
/// `current_source_index` is distinct from the session's playback-order index: under
/// shuffle it still identifies the exact row in Music.app playlist order, including a
/// duplicate track occurrence. This is deliberately an explicit navigation action; backend
/// updates never call it, so browsing selection remains under user control.
fn jump_to_playing_playlist_track(state: &mut AppState) {
    let Route::PlaylistDetail { playlist_id } = &state.navigation.active else {
        return;
    };
    let crate::domain::PlaybackContext::Playlist {
        playlist_id: context_playlist_id,
        current_source_index,
        ..
    } = &state.playback.context
    else {
        return;
    };
    if context_playlist_id != playlist_id {
        return;
    }
    let Some(playlist) = playlist_by_id(state, playlist_id) else {
        return;
    };
    // A partially loaded playlist simply has no row to reveal yet. Do not turn this
    // navigation action into a new Music.app request.
    if *current_source_index < playlist.tracks.len() {
        state.content_selection = *current_source_index;
    }
}

fn request_playlist_track_removal(state: &mut AppState) -> Vec<Command> {
    let Route::PlaylistDetail { playlist_id } = state.navigation.active.clone() else {
        return Vec::new();
    };
    let Some(playlist) = playlist_by_id(state, &playlist_id) else {
        return Vec::new();
    };
    let Some(track) = playlist.tracks.get(state.content_selection) else {
        return Vec::new();
    };
    request_playlist_track_removal_for(
        state,
        playlist_id,
        state.content_selection,
        track.id.clone(),
    )
}

fn request_playlist_track_removal_for(
    state: &mut AppState,
    playlist_id: PlaylistId,
    index: usize,
    track_id: crate::domain::TrackId,
) -> Vec<Command> {
    let Some(playlist) = playlist_by_id(state, &playlist_id) else {
        return stale_action_target(state);
    };
    if playlist.kind != PlaylistKind::User
        || !state.capabilities.supports(Capability::PlaylistTrackRemove)
        || playlist
            .tracks
            .get(index)
            .is_none_or(|track| track.id != track_id)
    {
        return Vec::new();
    }
    let track = &playlist.tracks[index];
    state.playlist_track_removal_confirmation = Some(PlaylistTrackRemovalConfirmation {
        playlist_id,
        index,
        track_id,
        track_title: track.title.clone(),
        playlist_name: playlist.name.clone(),
    });
    Vec::new()
}

fn selected_context_target(state: &AppState) -> Option<ContextTarget> {
    match &state.navigation.active {
        Route::Section(Screen::Songs) => {
            selected_collection_source_index(state, CollectionKind::Songs)
                .and_then(|index| state.library.get(index))
                .map(|track| ContextTarget::Track(track.id.clone()))
        }
        Route::Section(Screen::RecentlyPlayed) => state
            .recently_played
            .get(state.content_selection)
            .map(|entry| ContextTarget::Track(entry.track_id.clone())),
        Route::Section(Screen::Albums) => {
            selected_collection_source_index(state, CollectionKind::Albums)
                .and_then(|index| state.albums.get(index))
                .map(|album| ContextTarget::Album(album.id.clone()))
        }
        Route::Section(Screen::RecentlyAdded) => state
            .recently_added
            .get(state.content_selection)
            .map(|album| ContextTarget::Album(album.id.clone())),
        Route::Section(Screen::Artists) => {
            selected_collection_source_index(state, CollectionKind::Artists)
                .and_then(|index| state.artists.get(index))
                .map(|artist| ContextTarget::Artist(artist.id.clone()))
        }
        Route::Section(Screen::Playlists) => selected_playlist_id(state).and_then(|id| {
            playlist_by_id(state, &id).map(|playlist| {
                if playlist.kind == PlaylistKind::Folder {
                    ContextTarget::Folder(id)
                } else {
                    ContextTarget::Playlist(id)
                }
            })
        }),
        Route::Section(Screen::MadeForYou) => {
            state
                .playlists
                .get(state.content_selection)
                .map(|playlist| {
                    if playlist.kind == PlaylistKind::Folder {
                        ContextTarget::Folder(playlist.id.clone())
                    } else {
                        ContextTarget::Playlist(playlist.id.clone())
                    }
                })
        }
        Route::Section(Screen::Search) => state
            .search_results
            .get(state.content_selection)
            .and_then(|result| match result {
                LocalSearchResult::Track(id) => Some(ContextTarget::Track(id.clone())),
                LocalSearchResult::Album(id) => Some(ContextTarget::Album(id.clone())),
                LocalSearchResult::Artist(id) => Some(ContextTarget::Artist(id.clone())),
                LocalSearchResult::Playlist(id) => playlist_by_id(state, id).map(|playlist| {
                    if playlist.kind == PlaylistKind::Folder {
                        ContextTarget::Folder(id.clone())
                    } else {
                        ContextTarget::Playlist(id.clone())
                    }
                }),
            }),
        Route::PlaylistDetail { playlist_id } => state
            .playlists
            .iter()
            .find(|playlist| playlist.id == *playlist_id)
            .and_then(|playlist| playlist.tracks.get(state.content_selection))
            .map(|track| ContextTarget::PlaylistTrack {
                playlist_id: playlist_id.clone(),
                track_id: track.id.clone(),
                index: state.content_selection,
            }),
        Route::AlbumDetail { album_id } => state
            .albums
            .iter()
            .find(|album| album.id == *album_id)
            .and_then(|album| album.tracks.get(state.content_selection))
            .map(|track| ContextTarget::Track(track.id.clone())),
        Route::ArtistDetail { artist_id } => state
            .artists
            .iter()
            .find(|artist| artist.id == *artist_id)
            .and_then(|artist| artist.top_track_ids.get(state.content_selection))
            .cloned()
            .map(ContextTarget::Track),
        _ => None,
    }
}

fn actions_for_target(state: &AppState, target: &ContextTarget) -> Vec<ContextAction> {
    match target {
        ContextTarget::Track(track_id) | ContextTarget::PlaylistTrack { track_id, .. } => {
            let mut actions = Vec::new();
            if state.capabilities.supports(Capability::SelectionPlayback) {
                actions.push(ContextAction::PlayTrack);
            }
            if state
                .albums
                .iter()
                .any(|album| album.tracks.iter().any(|track| track.id == *track_id))
            {
                actions.push(ContextAction::OpenAlbum);
            }
            if state
                .artists
                .iter()
                .any(|artist| artist.top_track_ids.contains(track_id))
            {
                actions.push(ContextAction::OpenArtist);
            }
            if let ContextTarget::PlaylistTrack { playlist_id, .. } = target
                && playlist_by_id(state, playlist_id)
                    .is_some_and(|playlist| playlist.kind == PlaylistKind::User)
                && state.capabilities.supports(Capability::PlaylistTrackRemove)
            {
                actions.push(ContextAction::RemoveFromPlaylist);
            }
            actions
        }
        ContextTarget::Album(_) => {
            let mut actions = vec![ContextAction::OpenAlbum];
            if state.capabilities.supports(Capability::AlbumPlayback) {
                actions.push(ContextAction::PlayAlbum);
            }
            actions
        }
        ContextTarget::Artist(_) => vec![ContextAction::OpenArtist],
        ContextTarget::Playlist(_) => {
            let mut actions = vec![ContextAction::OpenPlaylist];
            if state.capabilities.supports(Capability::SelectionPlayback) {
                actions.push(ContextAction::PlayPlaylist);
            }
            actions
        }
        ContextTarget::Folder(id) => vec![if state.expanded_playlist_folders.contains(id) {
            ContextAction::CollapseFolder
        } else {
            ContextAction::ExpandFolder
        }],
    }
}

fn move_action_menu_selection(state: &mut AppState, down: bool) {
    let Some(menu) = state.action_menu.as_mut() else {
        return;
    };
    if menu.actions.is_empty() {
        menu.selection = 0;
    } else if down {
        menu.selection = (menu.selection + 1).min(menu.actions.len() - 1);
    } else {
        menu.selection = menu.selection.saturating_sub(1);
    }
}

fn execute_selected_action_menu(state: &mut AppState) -> Vec<Command> {
    let Some(menu) = state.action_menu.take() else {
        return Vec::new();
    };
    let Some(action) = menu.actions.get(menu.selection).copied() else {
        return Vec::new();
    };
    execute_context_action(state, menu.target, action)
}

fn execute_context_action(
    state: &mut AppState,
    target: ContextTarget,
    action: ContextAction,
) -> Vec<Command> {
    match (target, action) {
        (ContextTarget::Track(track_id), ContextAction::PlayTrack) => {
            if state.library.iter().any(|track| track.id == track_id) {
                backend_command(
                    state,
                    Capability::SelectionPlayback,
                    BackendCommand::PlayTrack(track_id),
                )
            } else {
                stale_action_target(state)
            }
        }
        (
            ContextTarget::PlaylistTrack {
                playlist_id,
                track_id,
                index,
            },
            ContextAction::PlayTrack,
        ) => {
            let Some(playlist) = playlist_by_id(state, &playlist_id) else {
                return stale_action_target(state);
            };
            if playlist
                .tracks
                .get(index)
                .is_none_or(|track| track.id != track_id)
            {
                return stale_action_target(state);
            }
            backend_command(
                state,
                Capability::SelectionPlayback,
                BackendCommand::PlayPlaylistTrack {
                    playlist_id,
                    ordered_track_ids: playlist
                        .tracks
                        .iter()
                        .map(|track| track.id.clone())
                        .collect(),
                    selected_index: index,
                    complete: playlist.contents_state.is_complete(),
                },
            )
        }
        (
            ContextTarget::PlaylistTrack {
                playlist_id,
                track_id,
                index,
            },
            ContextAction::RemoveFromPlaylist,
        ) => request_playlist_track_removal_for(state, playlist_id, index, track_id),
        (ContextTarget::Track(track_id), ContextAction::OpenAlbum)
        | (ContextTarget::PlaylistTrack { track_id, .. }, ContextAction::OpenAlbum) => {
            let Some(album) = state
                .albums
                .iter()
                .find(|album| album.tracks.iter().any(|track| track.id == track_id))
            else {
                return stale_action_target(state);
            };
            let album_id = album.id.clone();
            open_detail(
                state,
                Route::AlbumDetail {
                    album_id: album_id.clone(),
                },
            );
            request_album_artwork(state, album_id)
        }
        (ContextTarget::Track(track_id), ContextAction::OpenArtist)
        | (ContextTarget::PlaylistTrack { track_id, .. }, ContextAction::OpenArtist) => {
            let Some(artist) = state
                .artists
                .iter()
                .find(|artist| artist.top_track_ids.contains(&track_id))
            else {
                return stale_action_target(state);
            };
            open_detail(
                state,
                Route::ArtistDetail {
                    artist_id: artist.id.clone(),
                },
            );
            Vec::new()
        }
        (ContextTarget::Album(album_id), ContextAction::OpenAlbum) => {
            if state.albums.iter().any(|album| album.id == album_id)
                || state
                    .recently_added
                    .iter()
                    .any(|album| album.id == album_id)
            {
                open_detail(
                    state,
                    Route::AlbumDetail {
                        album_id: album_id.clone(),
                    },
                );
                request_album_artwork(state, album_id)
            } else {
                stale_action_target(state)
            }
        }
        (ContextTarget::Album(album_id), ContextAction::PlayAlbum) => {
            let Some(album) = state
                .albums
                .iter()
                .find(|album| album.id == album_id)
                .or_else(|| {
                    state
                        .recently_added
                        .iter()
                        .find(|album| album.id == album_id)
                })
            else {
                return stale_action_target(state);
            };
            backend_command(
                state,
                Capability::AlbumPlayback,
                BackendCommand::PlayAlbum {
                    album_id,
                    track_ids: album.tracks.iter().map(|track| track.id.clone()).collect(),
                },
            )
        }
        (ContextTarget::Artist(artist_id), ContextAction::OpenArtist) => {
            if state.artists.iter().any(|artist| artist.id == artist_id) {
                open_detail(state, Route::ArtistDetail { artist_id });
                Vec::new()
            } else {
                stale_action_target(state)
            }
        }
        (ContextTarget::Playlist(playlist_id), ContextAction::OpenPlaylist) => {
            let Some(playlist) = playlist_by_id(state, &playlist_id) else {
                return stale_action_target(state);
            };
            let needs_load = playlist.contents_state.should_request_load()
                && state.capabilities.supports(Capability::PlaylistRead);
            open_playlist_detail(state, playlist_id, needs_load)
        }
        (ContextTarget::Playlist(playlist_id), ContextAction::PlayPlaylist) => {
            if playlist_by_id(state, &playlist_id).is_some() {
                backend_command(
                    state,
                    Capability::SelectionPlayback,
                    BackendCommand::PlayPlaylist(playlist_id),
                )
            } else {
                stale_action_target(state)
            }
        }
        (
            ContextTarget::Folder(playlist_id),
            ContextAction::ExpandFolder | ContextAction::CollapseFolder,
        ) => {
            if playlist_by_id(state, &playlist_id)
                .is_some_and(|playlist| playlist.kind == PlaylistKind::Folder)
            {
                toggle_playlist_folder(state, playlist_id);
                Vec::new()
            } else {
                stale_action_target(state)
            }
        }
        _ => stale_action_target(state),
    }
}

fn stale_action_target(state: &mut AppState) -> Vec<Command> {
    state.notification = Some("Selected item is no longer available".to_owned());
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

    if matches!(state.navigation.active, Route::Section(Screen::Playlists)) {
        let Some(playlist_id) = selected_playlist_id(state) else {
            return Vec::new();
        };
        if playlist_by_id(state, &playlist_id)
            .is_some_and(|playlist| playlist.kind == PlaylistKind::Folder)
        {
            toggle_playlist_folder(state, playlist_id);
            return Vec::new();
        }
        let needs_load = playlist_by_id(state, &playlist_id).is_some_and(|playlist| {
            playlist.contents_state.should_request_load()
                && state.capabilities.supports(Capability::PlaylistRead)
        });
        return open_playlist_detail(state, playlist_id, needs_load);
    }

    if matches!(state.navigation.active, Route::Section(Screen::MadeForYou)) {
        let selected = state
            .playlists
            .get(state.content_selection)
            .map(|playlist| {
                (
                    playlist.id.clone(),
                    playlist.contents_state.should_request_load()
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
                open_detail(
                    state,
                    Route::AlbumDetail {
                        album_id: album_id.clone(),
                    },
                );
                return request_album_artwork(state, album_id);
            }
            Some(LocalSearchResult::Playlist(playlist_id)) => {
                let needs_load = state
                    .playlists
                    .iter()
                    .find(|playlist| playlist.id == playlist_id)
                    .is_some_and(|playlist| {
                        playlist.contents_state.should_request_load()
                            && state.capabilities.supports(Capability::PlaylistRead)
                    });
                return open_playlist_detail(state, playlist_id, needs_load);
            }
            Some(LocalSearchResult::Track(_)) | None => {}
        }
    }

    let destination = match &state.navigation.active {
        Route::Section(Screen::Artists) => {
            selected_collection_source_index(state, CollectionKind::Artists)
                .and_then(|index| state.artists.get(index))
                .map(|artist| Route::ArtistDetail {
                    artist_id: artist.id.clone(),
                })
        }
        Route::Section(Screen::Albums) => {
            selected_collection_source_index(state, CollectionKind::Albums)
                .and_then(|index| state.albums.get(index))
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
        let album_id = match &destination {
            Route::AlbumDetail { album_id } => Some(album_id.clone()),
            _ => None,
        };
        open_detail(state, destination);
        return album_id.map_or_else(Vec::new, |album_id| request_album_artwork(state, album_id));
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
        if let Some(playlist) = state
            .playlists
            .iter_mut()
            .find(|playlist| playlist.id == playlist_id)
        {
            playlist.contents_state = PlaylistLoadState::Loading {
                loaded: playlist.tracks.len(),
                total: (playlist.track_count > 0).then_some(playlist.track_count),
            };
        }
        vec![Command::Backend(BackendCommand::LoadPlaylist(playlist_id))]
    } else {
        Vec::new()
    }
}

fn request_album_artwork(state: &mut AppState, album_id: crate::domain::AlbumId) -> Vec<Command> {
    let key = ArtworkKey::Album(album_id.clone());
    let cache_is_transient = matches!(
        state.artwork_cache.get(&key),
        Some(ArtworkCacheEntry::Transient(_))
    );
    if state.artwork_cache.contains_key(&key) && !cache_is_transient
        || !state.capabilities.supports(Capability::ArtworkRead)
    {
        tracing::debug!(
            album_id = %album_id,
            cached = state.artwork_cache.contains_key(&key),
            artwork_capability = state.capabilities.supports(Capability::ArtworkRead),
            "artwork request not started"
        );
        return Vec::new();
    }
    let track_id = state
        .albums
        .iter()
        .find(|album| album.id == album_id)
        .and_then(|album| {
            album.tracks.iter().find(|track| {
                track.id.as_str().starts_with("musicapp:persistent:")
                    || track.id.as_str().starts_with("musicapp:database:")
            })
        })
        .map(|track| track.id.clone());
    let Some(track_id) = track_id else {
        insert_artwork_cache(
            state,
            key,
            ArtworkCacheEntry::Unavailable("No stable local artwork identity".to_owned()),
        );
        return Vec::new();
    };
    request_track_artwork(state, key, track_id)
}

fn request_track_artwork(
    state: &mut AppState,
    key: ArtworkKey,
    track_id: crate::domain::TrackId,
) -> Vec<Command> {
    if matches!(
        state.artwork_cache.get(&key),
        Some(ArtworkCacheEntry::Transient(_))
    ) {
        state.artwork_cache.remove(&key);
        state
            .artwork_cache_order
            .retain(|cached_key| cached_key != &key);
    }
    if state.artwork_cache.contains_key(&key)
        || !state.capabilities.supports(Capability::ArtworkRead)
    {
        return Vec::new();
    }
    tracing::debug!(key = ?key, track_id = %track_id, "artwork request started");
    state
        .artwork_request_tracks
        .insert(key.clone(), track_id.clone());
    state.artwork_retry_attempts.insert(key.clone(), 0);
    insert_artwork_cache(state, key.clone(), ArtworkCacheEntry::Loading);
    vec![Command::Backend(BackendCommand::LoadTrackArtwork {
        key,
        track_id,
    })]
}

fn play_selected_collection(state: &mut AppState) -> Vec<Command> {
    let album = match &state.navigation.active {
        Route::Section(Screen::Albums) => {
            selected_collection_source_index(state, CollectionKind::Albums)
                .and_then(|index| state.albums.get(index))
        }
        Route::Section(Screen::RecentlyAdded) => state.recently_added.get(state.content_selection),
        Route::AlbumDetail { album_id } => state.albums.iter().find(|album| album.id == *album_id),
        _ => None,
    };
    if let Some(album) = album {
        if album.tracks.is_empty() {
            state.notification = Some("Selected album has no playable local tracks".to_owned());
            return Vec::new();
        }
        return backend_command(
            state,
            Capability::AlbumPlayback,
            BackendCommand::PlayAlbum {
                album_id: album.id.clone(),
                track_ids: album.tracks.iter().map(|track| track.id.clone()).collect(),
            },
        );
    }

    let playlist_id = match &state.navigation.active {
        Route::Section(Screen::Playlists) => selected_playlist_id(state),
        Route::Section(Screen::MadeForYou) => state
            .playlists
            .get(state.content_selection)
            .map(|playlist| playlist.id.clone()),
        Route::PlaylistDetail { playlist_id } => Some(playlist_id.clone()),
        _ => None,
    };
    let Some(playlist_id) = playlist_id else {
        state.notification =
            Some("Select an album or playlist before starting collection playback".to_owned());
        return Vec::new();
    };
    if playlist_by_id(state, &playlist_id)
        .is_some_and(|playlist| playlist.kind == PlaylistKind::Folder)
    {
        state.notification = Some("Folders contain playlists and cannot be played".to_owned());
        return Vec::new();
    }
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

fn jump_selection(state: &mut AppState, end: bool) {
    let (selection, length) = selection_and_length(state);
    *selection = if end { length.saturating_sub(1) } else { 0 };
}

fn page_selection(state: &mut AppState, down: bool) {
    let page = usize::from(state.terminal_size.1.saturating_sub(12)).max(1);
    let (selection, length) = selection_and_length(state);
    *selection = if down {
        (*selection + page).min(length.saturating_sub(1))
    } else {
        selection.saturating_sub(page)
    };
}

fn selection_and_length(state: &mut AppState) -> (&mut usize, usize) {
    match state.focus {
        Focus::Sidebar => (&mut state.sidebar_selection, Screen::ALL.len()),
        Focus::Content => {
            let length = content_length(state);
            (&mut state.content_selection, length)
        }
        Focus::Queue => (&mut state.queue_selection, state.queue.len()),
    }
}

fn content_length(state: &AppState) -> usize {
    match &state.navigation.active {
        Route::NowPlaying => 0,
        Route::Section(Screen::ListenNow | Screen::Browse) => 3,
        Route::Section(Screen::Radio) => state.stations.len(),
        Route::Section(Screen::RecentlyAdded) => state.recently_added.len(),
        Route::Section(Screen::RecentlyPlayed) => state.recently_played.len(),
        Route::Section(Screen::Albums) => collection_view(state, CollectionKind::Albums)
            .source_len
            .map_or(state.albums.len(), |_| {
                collection_view(state, CollectionKind::Albums).indices.len()
            }),
        Route::Section(Screen::Artists) => collection_view(state, CollectionKind::Artists)
            .source_len
            .map_or(state.artists.len(), |_| {
                collection_view(state, CollectionKind::Artists)
                    .indices
                    .len()
            }),
        Route::Section(Screen::Songs) => collection_view(state, CollectionKind::Songs)
            .source_len
            .map_or(state.library.len(), |_| {
                collection_view(state, CollectionKind::Songs).indices.len()
            }),
        Route::Section(Screen::Search) => state.search_results.len(),
        Route::Section(Screen::MadeForYou) => state.playlists.len(),
        Route::Section(Screen::Playlists) => visible_playlist_entries(state).len(),
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
        Route::NowPlaying => None,
        Route::Section(Screen::Songs) => {
            selected_collection_source_index(state, CollectionKind::Songs)
                .and_then(|index| state.library.get(index))
                .map(|track| BackendCommand::PlayTrack(track.id.clone()))
        }
        Route::Section(Screen::RecentlyPlayed) => state
            .recently_played
            .get(state.content_selection)
            .map(|entry| BackendCommand::PlayTrack(entry.track_id.clone())),
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
            .and_then(|playlist| {
                playlist.tracks.get(state.content_selection).map(|_| {
                    BackendCommand::PlayPlaylistTrack {
                        playlist_id: playlist_id.clone(),
                        ordered_track_ids: playlist
                            .tracks
                            .iter()
                            .map(|track| track.id.clone())
                            .collect(),
                        selected_index: state.content_selection,
                        complete: playlist.contents_state.is_complete(),
                    }
                })
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

fn apply_backend_event(state: &mut AppState, event: BackendEvent) -> Vec<Command> {
    let mut commands = Vec::new();
    match event {
        BackendEvent::Update(BackendUpdate::LibraryRefreshStarted {
            availability,
            playback,
        }) => {
            apply_playback_update(state, availability, playback);
            state.library_status = crate::domain::CollectionLoadState::Refreshing {
                loaded: 0,
                total: 0,
            };
            state.notification = Some("Refreshing library…".to_owned());
        }
        BackendEvent::Update(BackendUpdate::LibraryRefreshFailed {
            availability,
            playback,
            message,
        }) => {
            apply_playback_update(state, availability, playback);
            state.library_status = crate::domain::CollectionLoadState::Error(message);
            state.notification = Some("Library refresh failed; cached data remains".to_owned());
        }
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
            state.recently_played = snapshot.recently_played;
            state.stations = snapshot.stations;
            replace_playlists(state, snapshot.playlists);
            state.library_status = snapshot.library_status;
            state.playlist_status = snapshot.playlist_status;
            rebuild_search_index(state);
            rebuild_library_views(state);
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
            state.recently_played = snapshot.recently_played;
            state.stations = snapshot.stations;
            replace_playlists(state, snapshot.playlists);
            state.library_status = snapshot.library_status;
            state.playlist_status = snapshot.playlist_status;
            rebuild_search_index(state);
            rebuild_library_views(state);
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
            replace_playlists(state, playlists);
            state.view_status = ViewStatus::Loaded;
            state.playlist_status = crate::domain::CollectionLoadState::Loaded {
                total: state.playlists.len(),
            };
            if matches!(
                state.library_status,
                crate::domain::CollectionLoadState::Cached { .. }
                    | crate::domain::CollectionLoadState::Refreshing { .. }
            ) {
                state.library_status = crate::domain::CollectionLoadState::Refreshing {
                    loaded: 0,
                    total: 0,
                };
            }
            rebuild_search_index(state);
            refresh_search(state);
            clamp_selections(state);
        }
        BackendEvent::Update(BackendUpdate::LibraryBatch {
            availability,
            playback,
            tracks,
            authoritative_tracks,
            loaded,
            total,
            complete,
            artists,
            albums,
            recently_added,
            recently_played,
        }) => {
            let reducer_started = std::time::Instant::now();
            let selected_before = active_collection(state)
                .and_then(|collection| selected_collection_identity(state, collection));
            let selection_before = state.content_selection;
            apply_playback_update(state, availability, playback);
            let refreshing_cached_library = matches!(
                state.library_status,
                crate::domain::CollectionLoadState::Cached { .. }
                    | crate::domain::CollectionLoadState::Refreshing { .. }
            );
            let starts_new_load = loaded == tracks.len();
            if starts_new_load && !refreshing_cached_library {
                state.library.clear();
                state.artists.clear();
                state.albums.clear();
                state.recently_added.clear();
                state.recently_played.clear();
                state
                    .search_index
                    .retain(|entry| matches!(entry.result, LocalSearchResult::Playlist(_)));
            }
            if let Some(authoritative_tracks) = authoritative_tracks {
                state.library = authoritative_tracks;
            } else {
                merge_library_tracks(state, &tracks);
                refresh_track_search_entries(state, &tracks);
            }
            let was_refreshing = refreshing_cached_library;
            state.library_status = if complete {
                crate::domain::CollectionLoadState::Loaded { total }
            } else if refreshing_cached_library {
                crate::domain::CollectionLoadState::Refreshing { loaded, total }
            } else {
                crate::domain::CollectionLoadState::Loading { loaded, total }
            };
            if complete {
                state.artists = artists;
                state.albums = albums;
                state.recently_added = recently_added;
                state.recently_played = recently_played;
                if was_refreshing {
                    state.notification = Some("Library refresh complete".to_owned());
                }
            }
            if complete {
                let search_started = std::time::Instant::now();
                rebuild_search_index(state);
                tracing::debug!(
                    entries = state.search_index.len(),
                    search_index_ms = search_started.elapsed().as_secs_f64() * 1_000.0,
                    "local search index construction timing"
                );
            }
            if !refreshing_cached_library || complete {
                rebuild_library_views(state);
            }
            refresh_search(state);
            state.view_status = if state.library.is_empty() && complete {
                ViewStatus::Empty
            } else {
                ViewStatus::Loaded
            };
            if !refreshing_cached_library || complete {
                clamp_selections(state);
            }
            tracing::debug!(
                loaded,
                complete,
                selection_before,
                selection_after = state.content_selection,
                selected_before = ?selected_before,
                selected_after = ?active_collection(state).and_then(|collection| selected_collection_identity(state, collection)),
                presented_songs_generation = state.library_views.songs.rebuild_count,
                reducer_merge_ms = reducer_started.elapsed().as_secs_f64() * 1_000.0,
                "local library reducer timing"
            );
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
                playlist.contents_state = if complete && total == 0 {
                    PlaylistLoadState::Empty
                } else if complete {
                    PlaylistLoadState::Loaded { total }
                } else if loaded == 0 {
                    PlaylistLoadState::Loading {
                        loaded,
                        total: Some(total),
                    }
                } else {
                    PlaylistLoadState::PartiallyLoaded { loaded, total }
                };
            }
            clamp_selections(state);
        }
        BackendEvent::Update(BackendUpdate::PlaylistLoadFailed {
            availability,
            playback,
            playlist_id,
            message,
        }) => {
            apply_playback_update(state, availability, playback);
            if let Some(playlist) = state
                .playlists
                .iter_mut()
                .find(|playlist| playlist.id == playlist_id)
            {
                playlist.contents_state = PlaylistLoadState::Error(message.clone());
            }
            state.notification = Some(format!("Failed to load playlist: {message}"));
        }
        BackendEvent::Update(BackendUpdate::PlaylistTrackRemoved {
            availability,
            playback,
            playlist_id,
            index,
            expected_track_id,
        }) => {
            state.playlist_track_removal_in_flight = false;
            apply_playback_update(state, availability, playback);
            if let Some(playlist) = state
                .playlists
                .iter_mut()
                .find(|playlist| playlist.id == playlist_id)
                && playlist
                    .tracks
                    .get(index)
                    .is_some_and(|track| track.id == expected_track_id)
            {
                playlist.tracks.remove(index);
                playlist.track_count = playlist.track_count.saturating_sub(1);
                playlist.contents_state =
                    if playlist.tracks.is_empty() && playlist.contents_state.is_complete() {
                        PlaylistLoadState::Empty
                    } else if playlist.contents_state.is_complete() {
                        PlaylistLoadState::Loaded {
                            total: playlist.track_count,
                        }
                    } else {
                        playlist.contents_state.clone()
                    };
                state.content_selection = state
                    .content_selection
                    .min(playlist.tracks.len().saturating_sub(1));
                state.notification = Some("Removed track from playlist".to_owned());
            } else {
                state.notification =
                    Some("Playlist changed before removal could be applied; reload it".to_owned());
            }
        }
        BackendEvent::Update(BackendUpdate::Stopped {
            availability,
            playback,
        }) => {
            apply_playback_update(state, availability, playback);
        }
        BackendEvent::Update(BackendUpdate::PlaybackContextFailed {
            availability,
            playback,
            message,
        }) => {
            apply_playback_update(state, availability, playback);
            state.notification = Some(message);
        }
        BackendEvent::Update(BackendUpdate::Notice {
            availability,
            playback,
            message,
        }) => {
            apply_playback_update(state, availability, playback);
            state.notification = Some(message);
        }
        BackendEvent::Update(BackendUpdate::Artwork {
            availability,
            playback,
            key,
            result,
        }) => {
            let stale_for_current_track = !artwork_key_is_current(state, &key);
            if stale_for_current_track {
                tracing::debug!(key = ?key, stale_result_ignored = true, "artwork update did not replace newer playback state");
            } else {
                apply_playback_update(state, availability, playback);
            }
            let transient_failure = matches!(&result, ArtworkResult::Transient(_))
                || matches!(
                    &result,
                    ArtworkResult::Invalid(message) if message.starts_with("Music.app artwork query failed:")
                );
            let retry_attempt = state.artwork_retry_attempts.get(&key).copied().unwrap_or(0);
            if transient_failure
                && !stale_for_current_track
                && retry_attempt == 0
                && let Some(track_id) = state.artwork_request_tracks.get(&key).cloned()
            {
                state.artwork_retry_attempts.insert(key.clone(), 1);
                tracing::debug!(
                    key = ?key,
                    track_id = %track_id,
                    retry_attempt = 1,
                    "Music.app artwork query failed transiently; scheduling one retry"
                );
                commands.push(Command::RetryArtwork { key, track_id });
                return commands;
            }
            let entry = match result {
                ArtworkResult::Ready(artwork) => {
                    let source_fingerprint = artwork.fingerprint();
                    match artwork.media_type {
                        crate::domain::ArtworkMediaType::Png => {
                            if !renderable_artwork_is_cached(state, &key, source_fingerprint) {
                                insert_renderable_artwork_cache(
                                    state,
                                    key.clone(),
                                    RenderableArtworkCacheEntry::Ready {
                                        source_fingerprint,
                                        artwork: artwork.clone(),
                                    },
                                );
                            }
                        }
                        crate::domain::ArtworkMediaType::Jpeg => {
                            if !renderable_artwork_is_cached(state, &key, source_fingerprint) {
                                if let Some(cached) =
                                    renderable_artwork_for_source(state, source_fingerprint)
                                {
                                    tracing::debug!(
                                        key = ?key,
                                        source = "JPEG",
                                        transmitted = "PNG",
                                        conversion = "cached",
                                        "reused Kitty renderable artwork"
                                    );
                                    insert_renderable_artwork_cache(
                                        state,
                                        key.clone(),
                                        RenderableArtworkCacheEntry::Ready {
                                            source_fingerprint,
                                            artwork: cached,
                                        },
                                    );
                                } else {
                                    insert_renderable_artwork_cache(
                                        state,
                                        key.clone(),
                                        RenderableArtworkCacheEntry::Loading { source_fingerprint },
                                    );
                                    commands.push(Command::ConvertArtwork {
                                        key: key.clone(),
                                        source_fingerprint,
                                        source: artwork.clone(),
                                    });
                                }
                            }
                        }
                        crate::domain::ArtworkMediaType::Gif
                        | crate::domain::ArtworkMediaType::Unknown => {}
                    }
                    ArtworkCacheEntry::Ready(artwork)
                }
                ArtworkResult::Missing => {
                    ArtworkCacheEntry::Unavailable("No local Music.app artwork".to_owned())
                }
                ArtworkResult::TooLarge { encoded_bytes } => ArtworkCacheEntry::Unavailable(
                    format!("Artwork exceeds the 2 MiB limit ({encoded_bytes} bytes)"),
                ),
                ArtworkResult::Transient(message) => ArtworkCacheEntry::Transient(message),
                ArtworkResult::Invalid(message) => ArtworkCacheEntry::Unavailable(message),
            };
            match &entry {
                ArtworkCacheEntry::Ready(artwork) => tracing::debug!(
                    key = ?key,
                    bytes = artwork.bytes.len(),
                    format = ?artwork.media_type,
                    "artwork cache populated"
                ),
                ArtworkCacheEntry::Unavailable(reason) => tracing::debug!(
                    key = ?key,
                    %reason,
                    "artwork unavailable"
                ),
                ArtworkCacheEntry::Transient(reason) => tracing::debug!(
                    key = ?key,
                    %reason,
                    "artwork resolution remains transient and is not permanently cached"
                ),
                ArtworkCacheEntry::Loading => {}
            }
            insert_artwork_cache(state, key.clone(), entry);
            state.artwork_request_tracks.remove(&key);
            state.artwork_retry_attempts.remove(&key);
        }
        BackendEvent::Error(message) => {
            if state.playlist_track_removal_in_flight {
                state.playlist_track_removal_in_flight = false;
                tracing::debug!(%message, "Music.app playlist removal failed");
                state.notification =
                    Some("Remove failed: Music.app rejected the operation".to_owned());
                return commands;
            }
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
    commands
}

fn insert_artwork_cache(state: &mut AppState, key: ArtworkKey, entry: ArtworkCacheEntry) {
    const MAX_ARTWORK_CACHE_ENTRIES: usize = 16;
    state
        .artwork_cache_order
        .retain(|candidate| *candidate != key);
    state.artwork_cache_order.push(key.clone());
    state.artwork_cache.insert(key, entry);
    while state.artwork_cache_order.len() > MAX_ARTWORK_CACHE_ENTRIES {
        let oldest = state.artwork_cache_order.remove(0);
        state.artwork_cache.remove(&oldest);
        state.artwork_request_tracks.remove(&oldest);
        state.artwork_retry_attempts.remove(&oldest);
        state.renderable_artwork_cache.remove(&oldest);
        state
            .renderable_artwork_cache_order
            .retain(|candidate| *candidate != oldest);
    }
}

fn artwork_key_is_current(state: &AppState, key: &ArtworkKey) -> bool {
    match key {
        ArtworkKey::Track(track_id) => state
            .playback
            .current_track
            .as_ref()
            .is_some_and(|track| track.id == *track_id),
        ArtworkKey::Album(album_id) => {
            state
                .playback
                .current_track
                .as_ref()
                .is_some_and(|track| state.artwork_key_for_track(&track.id) == *key)
                || matches!(
                    &state.navigation.active,
                    Route::AlbumDetail { album_id: active } if active == album_id
                )
        }
        ArtworkKey::Playlist(_) => false,
    }
}

fn apply_artwork_conversion(
    state: &mut AppState,
    key: ArtworkKey,
    source_fingerprint: u64,
    result: Result<crate::domain::Artwork, String>,
) {
    let source_matches = matches!(
        state.artwork_cache.get(&key),
        Some(ArtworkCacheEntry::Ready(source)) if source.fingerprint() == source_fingerprint
    );
    if !source_matches {
        return;
    }
    let entry = match result {
        Ok(artwork) => {
            tracing::debug!(
                key = ?key,
                source = "JPEG",
                transmitted = "PNG",
                conversion = "new",
                bytes = artwork.bytes.len(),
                "Kitty renderable artwork prepared"
            );
            RenderableArtworkCacheEntry::Ready {
                source_fingerprint,
                artwork,
            }
        }
        Err(message) => {
            tracing::debug!(
                key = ?key,
                source = "JPEG",
                conversion = "failed",
                %message,
                "Kitty renderable artwork unavailable"
            );
            RenderableArtworkCacheEntry::Unavailable {
                source_fingerprint,
                message,
            }
        }
    };
    insert_renderable_artwork_cache(state, key, entry);
}

fn insert_renderable_artwork_cache(
    state: &mut AppState,
    key: ArtworkKey,
    entry: RenderableArtworkCacheEntry,
) {
    const MAX_ARTWORK_CACHE_ENTRIES: usize = 16;
    state
        .renderable_artwork_cache_order
        .retain(|candidate| *candidate != key);
    state.renderable_artwork_cache_order.push(key.clone());
    state.renderable_artwork_cache.insert(key, entry);
    while state.renderable_artwork_cache_order.len() > MAX_ARTWORK_CACHE_ENTRIES {
        let oldest = state.renderable_artwork_cache_order.remove(0);
        state.renderable_artwork_cache.remove(&oldest);
    }
}

fn renderable_artwork_is_cached(
    state: &AppState,
    key: &ArtworkKey,
    source_fingerprint: u64,
) -> bool {
    matches!(
        state.renderable_artwork_cache.get(key),
        Some(RenderableArtworkCacheEntry::Loading { source_fingerprint: cached })
            | Some(RenderableArtworkCacheEntry::Ready { source_fingerprint: cached, .. })
            | Some(RenderableArtworkCacheEntry::Unavailable { source_fingerprint: cached, .. })
            if *cached == source_fingerprint
    )
}

fn renderable_artwork_for_source(
    state: &AppState,
    source_fingerprint: u64,
) -> Option<crate::domain::Artwork> {
    state
        .renderable_artwork_cache
        .values()
        .find_map(|entry| match entry {
            RenderableArtworkCacheEntry::Ready {
                source_fingerprint: cached_fingerprint,
                artwork,
            } if *cached_fingerprint == source_fingerprint => Some(artwork.clone()),
            RenderableArtworkCacheEntry::Loading { .. }
            | RenderableArtworkCacheEntry::Ready { .. }
            | RenderableArtworkCacheEntry::Unavailable { .. } => None,
        })
}

fn playlist_by_id<'a>(
    state: &'a AppState,
    playlist_id: &PlaylistId,
) -> Option<&'a crate::domain::Playlist> {
    state
        .playlists
        .iter()
        .find(|playlist| playlist.id == *playlist_id)
}

fn visible_playlist_entries(state: &AppState) -> Vec<VisiblePlaylistEntry> {
    state.visible_playlist_entries()
}

fn selected_playlist_id(state: &AppState) -> Option<PlaylistId> {
    visible_playlist_entries(state)
        .get(state.content_selection)
        .map(|entry| entry.playlist_id.clone())
}

fn toggle_playlist_folder(state: &mut AppState, playlist_id: PlaylistId) {
    if !state.expanded_playlist_folders.remove(&playlist_id) {
        state.expanded_playlist_folders.insert(playlist_id);
    }
}

fn replace_playlists(state: &mut AppState, playlists: Vec<crate::domain::Playlist>) {
    let selected_id = if matches!(state.navigation.active, Route::Section(Screen::Playlists)) {
        selected_playlist_id(state)
    } else {
        None
    };
    state.playlists = playlists
        .into_iter()
        .map(|mut playlist| {
            if let Some(previous) = state
                .playlists
                .iter()
                .find(|existing| existing.id == playlist.id)
                && previous.contents_state.is_complete()
            {
                playlist.tracks = previous.tracks.clone();
                playlist.contents_state = previous.contents_state.clone();
            }
            playlist
        })
        .collect();
    state.playlist_hierarchy = PlaylistHierarchy::from_playlists(&state.playlists);
    let folder_ids = state
        .playlists
        .iter()
        .filter(|playlist| playlist.kind == PlaylistKind::Folder)
        .map(|playlist| playlist.id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    state
        .expanded_playlist_folders
        .retain(|playlist_id| folder_ids.contains(playlist_id));
    if let Some(selected_id) = selected_id
        && let Some(index) = visible_playlist_entries(state)
            .iter()
            .position(|entry| entry.playlist_id == selected_id)
    {
        state.content_selection = index;
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
    // `content_selection` is shared by every content screen.  Library batches refresh this
    // index so cached search remains immediately useful, but an inactive (usually empty)
    // search result set must never clamp the selection in Songs/Albums/Artists.
    if matches!(state.navigation.active, Route::Section(Screen::Search)) {
        state.content_selection = state
            .content_selection
            .min(state.search_results.len().saturating_sub(1));
    }
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

/// Merge one authoritative scan batch into last-known state without duplicating stable IDs.
fn merge_library_tracks(state: &mut AppState, tracks: &[crate::domain::Track]) {
    let positions = state
        .library
        .iter()
        .enumerate()
        .map(|(index, track)| (track.id.clone(), index))
        .collect::<HashMap<_, _>>();
    for track in tracks {
        if let Some(index) = positions.get(&track.id) {
            state.library[*index] = track.clone();
        } else {
            state.library.push(track.clone());
        }
    }
}

/// Keep cached search usable while metadata changes arrive in authoritative batches.
fn refresh_track_search_entries(state: &mut AppState, tracks: &[crate::domain::Track]) {
    let ids = tracks
        .iter()
        .map(|track| &track.id)
        .collect::<std::collections::BTreeSet<_>>();
    state.search_index.retain(|entry| match &entry.result {
        LocalSearchResult::Track(id) => !ids.contains(id),
        _ => true,
    });
    state
        .search_index
        .extend(tracks.iter().map(track_search_index_entry));
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
            state::{
                AppState, ArtworkCacheEntry, BackendStatus, CollectionKind, CollectionSort,
                ContextAction, ContextTarget, Focus, LocalSearchResult,
                RenderableArtworkCacheEntry, Route, Screen, ViewStatus,
            },
        },
        backend::{
            BackendCommand, BackendEvent, BackendUpdate, MusicBackend, capabilities::Capabilities,
            mock::MockMusicBackend,
        },
        domain::{
            Album, AlbumId, Artist, ArtistId, Artwork, ArtworkKey, ArtworkMediaType, ArtworkResult,
            BackendSnapshot, PlaybackContext, PlaybackSnapshot, PlaybackStatus, Playlist,
            PlaylistHierarchy, PlaylistId, PlaylistKind, PlaylistLoadState, Track, TrackId,
        },
    };

    use super::{
        content_length, insert_artwork_cache, insert_renderable_artwork_cache,
        rebuild_collection_view, rebuild_library_views, rebuild_search_index, reduce,
        refresh_search, selected_collection_identity,
    };

    #[test]
    fn navigation_updates_sidebar_and_opens_screen() {
        let mut state = AppState::default();
        reduce(&mut state, Action::MoveDown);
        reduce(&mut state, Action::OpenSelected);

        assert_eq!(state.navigation.active, Route::Section(Screen::Browse));
        assert_eq!(state.focus, Focus::Content);
    }

    #[test]
    fn compact_list_navigation_jumps_and_pages_without_losing_bounds() {
        let mut state = AppState {
            focus: Focus::Content,
            navigation: super::super::state::NavigationState {
                active: Route::Section(Screen::Songs),
                history: Vec::new(),
            },
            library: (0..40)
                .map(|index| {
                    Track::new(
                        format!("track-{index}"),
                        format!("Track {index}"),
                        "Artist",
                        "Album",
                        Duration::from_secs(60),
                    )
                })
                .collect(),
            terminal_size: (100, 24),
            ..AppState::default()
        };
        reduce(&mut state, Action::JumpToEnd);
        assert_eq!(state.content_selection, 39);
        reduce(&mut state, Action::PageUp);
        assert_eq!(state.content_selection, 27);
        reduce(&mut state, Action::JumpToStart);
        reduce(&mut state, Action::PageDown);
        assert_eq!(state.content_selection, 12);
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
                    artist_id: ArtistId::new("mock-artist-ferris"),
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
        assert_eq!(
            state.playlists[0].contents_state,
            PlaylistLoadState::Loading {
                loaded: 0,
                total: None,
            }
        );
    }

    #[test]
    fn playlist_batches_render_partial_tracks_and_reentry_does_not_restart_loading() {
        let playlist_id = PlaylistId::new("musicapp:playlist:persistent:PARTIAL");
        let mut state = AppState {
            navigation: crate::app::state::NavigationState {
                active: Route::Section(Screen::Playlists),
                history: Vec::new(),
            },
            focus: Focus::Content,
            capabilities: Capabilities::macos(),
            playlists: vec![Playlist::unloaded(
                playlist_id.to_string(),
                "Progressive Playlist",
                None,
                PlaylistKind::User,
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
        let first = Track::new("track-1", "One", "Artist", "Album", Duration::from_secs(60));
        reduce(
            &mut state,
            Action::Backend(Box::new(BackendEvent::Update(
                BackendUpdate::PlaylistBatch {
                    availability: crate::domain::BackendAvailability::Available,
                    playback: PlaybackSnapshot::default(),
                    playlist_id: playlist_id.clone(),
                    tracks: vec![first.clone()],
                    loaded: 1,
                    total: 2,
                    complete: false,
                },
            ))),
        );
        assert_eq!(state.playlists[0].tracks, vec![first]);
        assert_eq!(
            state.playlists[0].contents_state,
            PlaylistLoadState::PartiallyLoaded {
                loaded: 1,
                total: 2,
            }
        );

        reduce(&mut state, Action::Back);
        assert_eq!(state.navigation.active, Route::Section(Screen::Playlists));
        assert!(reduce(&mut state, Action::OpenSelected).is_empty());
        assert_eq!(
            state.playlists[0].contents_state,
            PlaylistLoadState::PartiallyLoaded {
                loaded: 1,
                total: 2,
            }
        );
    }

    #[test]
    fn completed_empty_and_failed_playlist_loads_are_explicit_states() {
        let empty_id = PlaylistId::new("musicapp:playlist:persistent:EMPTY");
        let failed_id = PlaylistId::new("musicapp:playlist:persistent:FAILED");
        let mut state = AppState {
            playlists: vec![
                Playlist::unloaded(
                    empty_id.to_string(),
                    "Empty",
                    None,
                    PlaylistKind::User,
                    None,
                ),
                Playlist::unloaded(
                    failed_id.to_string(),
                    "Failed",
                    None,
                    PlaylistKind::User,
                    None,
                ),
            ],
            ..AppState::default()
        };
        reduce(
            &mut state,
            Action::Backend(Box::new(BackendEvent::Update(
                BackendUpdate::PlaylistBatch {
                    availability: crate::domain::BackendAvailability::Available,
                    playback: PlaybackSnapshot::default(),
                    playlist_id: empty_id,
                    tracks: Vec::new(),
                    loaded: 0,
                    total: 0,
                    complete: true,
                },
            ))),
        );
        reduce(
            &mut state,
            Action::Backend(Box::new(BackendEvent::Update(
                BackendUpdate::PlaylistLoadFailed {
                    availability: crate::domain::BackendAvailability::Available,
                    playback: PlaybackSnapshot::default(),
                    playlist_id: failed_id,
                    message: "Automation timed out".to_owned(),
                },
            ))),
        );

        assert_eq!(state.playlists[0].contents_state, PlaylistLoadState::Empty);
        assert_eq!(
            state.playlists[1].contents_state,
            PlaylistLoadState::Error("Automation timed out".to_owned())
        );
        assert_eq!(
            state.notification.as_deref(),
            Some("Failed to load playlist: Automation timed out")
        );
    }

    #[test]
    fn playlist_batches_preserve_music_app_order() {
        let playlist_id = PlaylistId::new("musicapp:playlist:persistent:ORDER");
        let mut state = AppState {
            playlists: vec![Playlist::unloaded(
                playlist_id.to_string(),
                "Ordered",
                None,
                PlaylistKind::User,
                None,
            )],
            ..AppState::default()
        };
        for (tracks, loaded, complete) in [
            (
                vec![
                    Track::new("t1", "Same", "Artist", "Album", Duration::from_secs(1)),
                    Track::new("t2", "Same", "Artist", "Album", Duration::from_secs(1)),
                ],
                2,
                false,
            ),
            (
                vec![Track::new(
                    "t3",
                    "Different",
                    "Artist",
                    "Album",
                    Duration::from_secs(1),
                )],
                3,
                true,
            ),
        ] {
            reduce(
                &mut state,
                Action::Backend(Box::new(BackendEvent::Update(
                    BackendUpdate::PlaylistBatch {
                        availability: crate::domain::BackendAvailability::Available,
                        playback: PlaybackSnapshot::default(),
                        playlist_id: playlist_id.clone(),
                        tracks,
                        loaded,
                        total: 3,
                        complete,
                    },
                ))),
            );
        }
        assert_eq!(
            state.playlists[0]
                .tracks
                .iter()
                .map(|track| track.id.clone())
                .collect::<Vec<_>>(),
            vec![TrackId::new("t1"), TrackId::new("t2"), TrackId::new("t3")]
        );
        assert_eq!(
            state.playlists[0].contents_state,
            PlaylistLoadState::Loaded { total: 3 }
        );
    }

    #[test]
    fn nested_folders_expand_with_stable_selection_and_open_exact_duplicate_name() {
        let root_folder_id = PlaylistId::new("folder-root");
        let child_folder_id = PlaylistId::new("folder-child");
        let nested_playlist_id = PlaylistId::new("playlist-nested-duplicate");
        let root_duplicate_id = PlaylistId::new("playlist-root-duplicate");

        let mut root_folder = playlist("folder-root", "Projects");
        root_folder.kind = PlaylistKind::Folder;
        root_folder.tracks.clear();
        root_folder.track_count = 0;
        let mut child_folder = playlist("folder-child", "Archive");
        child_folder.kind = PlaylistKind::Folder;
        child_folder.parent_id = Some(root_folder_id.clone());
        child_folder.tracks.clear();
        child_folder.track_count = 0;
        let nested = crate::domain::Playlist::unloaded(
            nested_playlist_id.to_string(),
            "Duplicate Name",
            None,
            PlaylistKind::User,
            Some(child_folder_id.clone()),
        );
        let root_duplicate = crate::domain::Playlist::unloaded(
            root_duplicate_id.to_string(),
            "Duplicate Name",
            None,
            PlaylistKind::User,
            None,
        );
        let playlists = vec![root_folder, child_folder, nested, root_duplicate];
        let mut state = AppState {
            navigation: crate::app::state::NavigationState {
                active: Route::Section(Screen::Playlists),
                history: Vec::new(),
            },
            focus: Focus::Content,
            capabilities: Capabilities::macos(),
            playlist_hierarchy: PlaylistHierarchy::from_playlists(&playlists),
            playlists,
            ..AppState::default()
        };

        assert_eq!(
            state
                .visible_playlist_entries()
                .iter()
                .map(|entry| (&entry.playlist_id, entry.depth))
                .collect::<Vec<_>>(),
            vec![(&root_folder_id, 0), (&root_duplicate_id, 0)]
        );

        assert!(reduce(&mut state, Action::OpenSelected).is_empty());
        assert_eq!(state.content_selection, 0);
        assert_eq!(
            state.visible_playlist_entries()[1].playlist_id,
            child_folder_id
        );
        assert_eq!(state.visible_playlist_entries()[1].depth, 1);

        reduce(&mut state, Action::MoveDown);
        assert!(reduce(&mut state, Action::OpenSelected).is_empty());
        assert_eq!(state.content_selection, 1);
        assert_eq!(
            state.visible_playlist_entries()[2].playlist_id,
            nested_playlist_id
        );
        assert_eq!(state.visible_playlist_entries()[2].depth, 2);

        assert!(reduce(&mut state, Action::OpenSelected).is_empty());
        assert_eq!(state.content_selection, 1);
        assert_eq!(state.visible_playlist_entries().len(), 3);
        assert!(reduce(&mut state, Action::OpenSelected).is_empty());
        assert_eq!(state.content_selection, 1);
        assert_eq!(
            state.visible_playlist_entries()[2].playlist_id,
            nested_playlist_id
        );

        reduce(&mut state, Action::MoveDown);
        assert_eq!(
            reduce(&mut state, Action::OpenSelected),
            vec![Command::Backend(BackendCommand::LoadPlaylist(
                nested_playlist_id.clone()
            ))]
        );
        assert_eq!(
            state.navigation.active,
            Route::PlaylistDetail {
                playlist_id: nested_playlist_id
            }
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
                    authoritative_tracks: None,
                    loaded: 1,
                    total: 2,
                    complete: false,
                    artists: Vec::new(),
                    albums: Vec::new(),
                    recently_added: Vec::new(),
                    recently_played: Vec::new(),
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
                    authoritative_tracks: None,
                    loaded: 2,
                    total: 2,
                    complete: true,
                    artists: Vec::new(),
                    albums: vec![album.clone()],
                    recently_added: vec![album],
                    recently_played: Vec::new(),
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
    fn authoritative_refresh_reconciles_cached_tracks_without_duplicates_or_stale_search() {
        let mut state = AppState::default();
        let cached = Track::new(
            "T1",
            "Old title",
            "Artist",
            "Album",
            Duration::from_secs(60),
        );
        let stale = Track::new(
            "T2",
            "Deleted song",
            "Artist",
            "Album",
            Duration::from_secs(60),
        );
        state.library = vec![cached, stale];
        state.library_status = crate::domain::CollectionLoadState::Cached { total: 2 };
        rebuild_search_index(&mut state);

        let changed = Track::new(
            "T1",
            "New title",
            "Artist",
            "Album",
            Duration::from_secs(60),
        );
        let added = Track::new("T3", "New song", "Artist", "Album", Duration::from_secs(60));
        reduce(
            &mut state,
            Action::Backend(Box::new(BackendEvent::Update(
                BackendUpdate::LibraryBatch {
                    availability: crate::domain::BackendAvailability::Available,
                    playback: PlaybackSnapshot::default(),
                    tracks: vec![changed.clone(), added.clone()],
                    authoritative_tracks: None,
                    loaded: 2,
                    total: 2,
                    complete: false,
                    artists: Vec::new(),
                    albums: Vec::new(),
                    recently_added: Vec::new(),
                    recently_played: Vec::new(),
                },
            ))),
        );
        assert_eq!(state.library.len(), 3);
        assert_eq!(state.library[0].title, "New title");
        state.search_query = "new title".to_owned();
        refresh_search(&mut state);
        assert_eq!(
            state.search_results,
            vec![LocalSearchResult::Track(changed.id.clone())]
        );

        reduce(
            &mut state,
            Action::Backend(Box::new(BackendEvent::Update(
                BackendUpdate::LibraryBatch {
                    availability: crate::domain::BackendAvailability::Available,
                    playback: PlaybackSnapshot::default(),
                    tracks: vec![changed.clone(), added.clone()],
                    authoritative_tracks: Some(vec![changed.clone(), added.clone()]),
                    loaded: 2,
                    total: 2,
                    complete: true,
                    artists: Vec::new(),
                    albums: Vec::new(),
                    recently_added: Vec::new(),
                    recently_played: Vec::new(),
                },
            ))),
        );
        assert_eq!(state.library, vec![changed, added]);
        state.search_query = "deleted".to_owned();
        refresh_search(&mut state);
        assert!(state.search_results.is_empty());
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
            reduce(&mut state, Action::PlaySelectedCollection),
            vec![Command::Backend(BackendCommand::PlayPlaylist(playlist_id))]
        );
    }

    #[tokio::test]
    async fn action_menu_captures_song_identity_navigates_and_executes_existing_routes() {
        let mut state = loaded_mock_state().await;
        reduce(&mut state, Action::GoTo(Screen::Songs));
        reduce(&mut state, Action::MoveDown);
        let original_selection = state.content_selection;

        reduce(&mut state, Action::OpenActions);
        let menu = state.action_menu.as_ref().expect("song action menu");
        assert_eq!(menu.target, ContextTarget::Track(TrackId::new("mock-002")));
        assert_eq!(
            menu.actions,
            vec![
                ContextAction::PlayTrack,
                ContextAction::OpenAlbum,
                ContextAction::OpenArtist
            ]
        );

        reduce(&mut state, Action::MoveDown);
        reduce(&mut state, Action::MoveDown);
        assert_eq!(state.action_menu.as_ref().expect("menu").selection, 2);
        reduce(&mut state, Action::MoveUp);
        assert_eq!(state.action_menu.as_ref().expect("menu").selection, 1);
        assert_eq!(state.content_selection, original_selection);

        assert!(reduce(&mut state, Action::OpenSelected).is_empty());
        assert!(state.action_menu.is_none());
        assert_eq!(
            state.navigation.active,
            Route::AlbumDetail {
                album_id: AlbumId::new("mock-album-safe-transitions")
            }
        );
        assert_eq!(state.content_selection, 0);
    }

    #[tokio::test]
    async fn action_menu_supports_every_selection_type_with_stable_ids() {
        let mut state = loaded_mock_state().await;

        reduce(&mut state, Action::GoTo(Screen::Albums));
        reduce(&mut state, Action::OpenActions);
        assert_eq!(
            state.action_menu.as_ref().expect("album menu").actions,
            vec![ContextAction::OpenAlbum, ContextAction::PlayAlbum]
        );

        reduce(&mut state, Action::Back);
        reduce(&mut state, Action::GoTo(Screen::Artists));
        reduce(&mut state, Action::OpenActions);
        assert_eq!(
            state.action_menu.as_ref().expect("artist menu").actions,
            vec![ContextAction::OpenArtist]
        );

        reduce(&mut state, Action::Back);
        reduce(&mut state, Action::GoTo(Screen::Playlists));
        reduce(&mut state, Action::OpenActions);
        let playlist_id = match &state.action_menu.as_ref().expect("playlist menu").target {
            ContextTarget::Playlist(id) => id.clone(),
            target => panic!("unexpected target: {target:?}"),
        };
        assert_eq!(
            state.action_menu.as_ref().expect("playlist menu").actions,
            vec![ContextAction::OpenPlaylist, ContextAction::PlayPlaylist]
        );
        reduce(&mut state, Action::MoveDown);
        assert_eq!(
            reduce(&mut state, Action::OpenSelected),
            vec![Command::Backend(BackendCommand::PlayPlaylist(playlist_id))]
        );

        let folder_id = PlaylistId::new("folder");
        let mut folder = Playlist::unloaded(
            folder_id.to_string(),
            "Folder",
            None,
            PlaylistKind::Folder,
            None,
        );
        folder.track_count = 0;
        state.playlists.insert(0, folder);
        state.playlist_hierarchy = PlaylistHierarchy::from_playlists(&state.playlists);
        state.content_selection = 0;
        reduce(&mut state, Action::OpenActions);
        assert_eq!(
            state.action_menu.as_ref().expect("folder menu").actions,
            vec![ContextAction::ExpandFolder]
        );
        reduce(&mut state, Action::OpenSelected);
        assert!(state.expanded_playlist_folders.contains(&folder_id));
        reduce(&mut state, Action::OpenActions);
        assert_eq!(
            state
                .action_menu
                .as_ref()
                .expect("expanded folder menu")
                .actions,
            vec![ContextAction::CollapseFolder]
        );
    }

    #[tokio::test]
    async fn action_menu_routes_search_and_playlist_detail_without_index_reuse() {
        let mut state = loaded_mock_state().await;
        let track_id = state.library[0].id.clone();
        let album_id = state.albums[0].id.clone();
        let artist_id = state.artists[0].id.clone();
        let playlist_id = state.playlists[0].id.clone();
        reduce(&mut state, Action::GoTo(Screen::Search));
        state.search_input_active = false;

        for (result, expected) in [
            (
                LocalSearchResult::Track(track_id.clone()),
                ContextTarget::Track(track_id.clone()),
            ),
            (
                LocalSearchResult::Album(album_id.clone()),
                ContextTarget::Album(album_id.clone()),
            ),
            (
                LocalSearchResult::Artist(artist_id.clone()),
                ContextTarget::Artist(artist_id.clone()),
            ),
            (
                LocalSearchResult::Playlist(playlist_id.clone()),
                ContextTarget::Playlist(playlist_id.clone()),
            ),
        ] {
            state.search_results = vec![result];
            state.content_selection = 0;
            reduce(&mut state, Action::OpenActions);
            assert_eq!(
                state.action_menu.as_ref().expect("search menu").target,
                expected
            );
            reduce(&mut state, Action::Back);
        }

        state.navigation.active = Route::PlaylistDetail {
            playlist_id: playlist_id.clone(),
        };
        state.content_selection = 1;
        reduce(&mut state, Action::OpenActions);
        let menu = state.action_menu.as_ref().expect("playlist track menu");
        assert!(matches!(
            menu.target,
            ContextTarget::PlaylistTrack {
                ref playlist_id,
                ref track_id,
                ..
            } if *playlist_id == state.playlists[0].id && *track_id == state.playlists[0].tracks[1].id
        ));
        assert_eq!(menu.actions.first(), Some(&ContextAction::PlayTrack));

        state.playlists[0].tracks.clear();
        let commands = reduce(&mut state, Action::OpenSelected);
        assert!(commands.is_empty());
        assert!(state.action_menu.is_none());
        assert_eq!(
            state.notification.as_deref(),
            Some("Selected item is no longer available")
        );
    }

    #[tokio::test]
    async fn action_menu_quit_and_back_close_without_quitting_or_moving_selection() {
        let mut state = loaded_mock_state().await;
        reduce(&mut state, Action::GoTo(Screen::Songs));
        reduce(&mut state, Action::MoveDown);
        let selection = state.content_selection;
        reduce(&mut state, Action::OpenActions);
        reduce(
            &mut state,
            Action::Resize {
                width: 54,
                height: 12,
            },
        );
        assert_eq!(state.terminal_size, (54, 12));
        assert!(state.action_menu.is_some());
        reduce(&mut state, Action::Quit);
        assert!(state.action_menu.is_none());
        assert!(!state.should_quit);
        assert_eq!(state.content_selection, selection);

        reduce(&mut state, Action::OpenActions);
        reduce(&mut state, Action::Back);
        assert!(state.action_menu.is_none());
        assert!(!state.should_quit);
        assert_eq!(state.content_selection, selection);
    }

    #[test]
    fn unsupported_context_actions_are_not_advertised() {
        let track = Track::new("track", "Track", "Artist", "Album", Duration::from_secs(1));
        let album = Album::new("album", "Album", "Artist", 2024, "", vec![track.clone()]);
        let mut state = AppState {
            navigation: crate::app::state::NavigationState {
                active: Route::Section(Screen::Songs),
                history: Vec::new(),
            },
            focus: Focus::Content,
            library: vec![track],
            albums: vec![album],
            ..AppState::default()
        };

        reduce(&mut state, Action::OpenActions);
        assert_eq!(
            state.action_menu.as_ref().expect("track menu").actions,
            vec![ContextAction::OpenAlbum]
        );
    }

    #[test]
    fn collection_views_sort_filter_and_preserve_stable_selection() {
        let mut first = Track::new("one", "Zulu", "Beta", "Second", Duration::from_secs(1));
        first.metadata.date_added = Some("2024-01-01".to_owned());
        first.metadata.year = Some(2020);
        first.metadata.play_count = Some(2);
        first.metadata.genre = Some("Jazz".to_owned());
        let mut second = Track::new("two", "Alpha", "Alpha", "First", Duration::from_secs(1));
        second.metadata.date_added = Some("2025-01-01".to_owned());
        second.metadata.year = Some(2024);
        second.metadata.play_count = Some(9);
        let third = Track::new("three", "Alpha", "Gamma", "Third", Duration::from_secs(1));
        let album_a = Album::new(
            "album-a",
            "Zulu Album",
            "Beta",
            2020,
            "2024-01-01",
            vec![first.clone()],
        );
        let album_b = Album::new(
            "album-b",
            "Alpha Album",
            "Alpha",
            2024,
            "2025-01-01",
            vec![second.clone(), third.clone()],
        );
        let artist_a = Artist::new(
            "artist-a",
            "Zulu",
            vec![album_a.id.clone()],
            vec![first.id.clone()],
        );
        let artist_b = Artist::new(
            "artist-b",
            "Alpha",
            vec![album_b.id.clone(), AlbumId::new("album-extra")],
            vec![second.id.clone(), third.id.clone()],
        );
        let mut state = AppState {
            navigation: crate::app::state::NavigationState {
                active: Route::Section(Screen::Songs),
                history: Vec::new(),
            },
            focus: Focus::Content,
            library: vec![first, second, third],
            albums: vec![album_a, album_b],
            artists: vec![artist_a, artist_b],
            ..AppState::default()
        };
        rebuild_library_views(&mut state);
        assert_eq!(state.library_views.songs.indices, vec![2, 1, 0]);

        state.content_selection = 2;
        state.library_views.songs.sort = CollectionSort::SongArtist;
        rebuild_collection_view(&mut state, CollectionKind::Songs);
        assert_eq!(state.library_views.songs.indices, vec![1, 0, 2]);
        assert_eq!(
            state.library[state.library_views.songs.indices[state.content_selection]].id,
            TrackId::new("one")
        );

        state.library_views.songs.filter = "jAzZ".to_owned();
        rebuild_collection_view(&mut state, CollectionKind::Songs);
        assert_eq!(state.library_views.songs.indices, vec![0]);
        assert_eq!(state.content_selection, 0);
        state.library_views.songs.filter.clear();
        state.library_views.songs.sort = CollectionSort::SongDateAdded;
        state.library_views.songs.descending = true;
        rebuild_collection_view(&mut state, CollectionKind::Songs);
        assert_eq!(state.library_views.songs.indices[0], 1);
        state.content_selection = 1;
        reduce(&mut state, Action::OpenActions);
        assert_eq!(
            state
                .action_menu
                .as_ref()
                .expect("sorted action target")
                .target,
            ContextTarget::Track(TrackId::new("one"))
        );
        reduce(&mut state, Action::Back);

        state.navigation.active = Route::Section(Screen::Albums);
        state.library_views.albums.sort = CollectionSort::AlbumArtist;
        rebuild_collection_view(&mut state, CollectionKind::Albums);
        assert_eq!(state.library_views.albums.indices, vec![1, 0]);
        state.library_views.albums.sort = CollectionSort::AlbumYear;
        rebuild_collection_view(&mut state, CollectionKind::Albums);
        assert_eq!(state.library_views.albums.indices, vec![0, 1]);
        state.library_views.albums.sort = CollectionSort::AlbumRecentlyAdded;
        state.library_views.albums.descending = true;
        rebuild_collection_view(&mut state, CollectionKind::Albums);
        assert_eq!(state.library_views.albums.indices, vec![1, 0]);

        state.navigation.active = Route::Section(Screen::Artists);
        state.library_views.artists.sort = CollectionSort::ArtistAlbumCount;
        state.library_views.artists.descending = true;
        rebuild_collection_view(&mut state, CollectionKind::Artists);
        assert_eq!(state.library_views.artists.indices, vec![1, 0]);
        state.library_views.artists.sort = CollectionSort::ArtistTrackCount;
        rebuild_collection_view(&mut state, CollectionKind::Artists);
        assert_eq!(state.library_views.artists.indices, vec![1, 0]);
    }

    #[test]
    fn collection_filter_modal_and_sort_menu_are_modal_without_recomputing_on_idle() {
        let mut state = AppState {
            navigation: crate::app::state::NavigationState {
                active: Route::Section(Screen::Songs),
                history: Vec::new(),
            },
            focus: Focus::Content,
            library: vec![Track::new(
                "one",
                "Alpha",
                "Artist",
                "Album",
                Duration::from_secs(1),
            )],
            ..AppState::default()
        };
        rebuild_library_views(&mut state);
        let initial_rebuilds = state.library_views.songs.rebuild_count;
        reduce(&mut state, Action::StartCollectionFilter);
        reduce(&mut state, Action::CollectionFilterInput('a'));
        assert_eq!(state.filter_editor.as_ref().expect("editor").draft, "a");
        reduce(&mut state, Action::Back);
        assert!(state.filter_editor.is_none());
        assert!(state.library_views.songs.filter.is_empty());
        reduce(&mut state, Action::OpenCollectionSort);
        reduce(&mut state, Action::PlayPause);
        assert!(state.library_views.songs.descending);
        reduce(&mut state, Action::Quit);
        assert!(state.sort_menu.is_none());
        assert!(!state.should_quit);
        assert!(state.library_views.songs.rebuild_count > initial_rebuilds);
        let after_actions = state.library_views.songs.rebuild_count;
        let _ = content_length(&state);
        assert_eq!(state.library_views.songs.rebuild_count, after_actions);
    }

    #[test]
    fn progressive_library_batch_respects_active_song_sort_and_filter() {
        let initial = Track::new("one", "Zulu", "Artist", "Album", Duration::from_secs(1));
        let mut state = AppState {
            navigation: crate::app::state::NavigationState {
                active: Route::Section(Screen::Songs),
                history: Vec::new(),
            },
            focus: Focus::Content,
            library: vec![initial],
            library_status: crate::domain::CollectionLoadState::Loading {
                loaded: 1,
                total: 2,
            },
            ..AppState::default()
        };
        state.library_views.songs.filter = "artist".to_owned();
        rebuild_library_views(&mut state);
        let incoming = Track::new("two", "Alpha", "Artist", "Album", Duration::from_secs(1));
        reduce(
            &mut state,
            Action::Backend(Box::new(BackendEvent::Update(
                BackendUpdate::LibraryBatch {
                    availability: crate::domain::BackendAvailability::Available,
                    playback: PlaybackSnapshot::default(),
                    tracks: vec![incoming],
                    authoritative_tracks: None,
                    loaded: 2,
                    total: 2,
                    complete: false,
                    artists: Vec::new(),
                    albums: Vec::new(),
                    recently_added: Vec::new(),
                    recently_played: Vec::new(),
                },
            ))),
        );
        assert_eq!(state.library_views.songs.indices, vec![1, 0]);
        assert_eq!(state.library_views.songs.filter, "artist");
    }

    #[test]
    fn cached_refresh_batches_do_not_clamp_active_collection_navigation() {
        let tracks = ["A", "B", "C", "D", "E"]
            .into_iter()
            .map(|id| Track::new(id, id, "Artist", "Album", Duration::from_secs(1)))
            .collect::<Vec<_>>();
        let mut state = AppState {
            navigation: crate::app::state::NavigationState {
                active: Route::Section(Screen::Songs),
                history: Vec::new(),
            },
            focus: Focus::Content,
            library: tracks.clone(),
            library_status: crate::domain::CollectionLoadState::Cached {
                total: tracks.len(),
            },
            ..AppState::default()
        };
        state.library_views.songs.sort = CollectionSort::SongTitle;
        rebuild_library_views(&mut state);
        let presented_generation = state.library_views.songs.rebuild_count;

        for (action, expected_id) in [
            (Action::MoveDown, "B"),
            (Action::MoveDown, "C"),
            (Action::MoveUp, "B"),
            (Action::MoveDown, "C"),
        ] {
            reduce(&mut state, action);
            let selected_before = selected_collection_identity(&state, CollectionKind::Songs)
                .expect("selected cached song");
            assert_eq!(selected_before, expected_id);
            reduce(
                &mut state,
                Action::Backend(Box::new(BackendEvent::Update(
                    BackendUpdate::LibraryBatch {
                        availability: crate::domain::BackendAvailability::Available,
                        playback: PlaybackSnapshot::default(),
                        tracks: vec![Track::new(
                            "incoming",
                            "Incoming",
                            "Artist",
                            "Album",
                            Duration::from_secs(1),
                        )],
                        authoritative_tracks: None,
                        loaded: 1,
                        total: 6,
                        complete: false,
                        artists: Vec::new(),
                        albums: Vec::new(),
                        recently_added: Vec::new(),
                        recently_played: Vec::new(),
                    },
                ))),
            );
            assert_eq!(
                selected_collection_identity(&state, CollectionKind::Songs),
                Some(selected_before)
            );
            assert_eq!(
                state.library_views.songs.rebuild_count,
                presented_generation
            );
            assert_eq!(
                state.library_views.songs.indices.len(),
                tracks.len(),
                "partial refresh must not alter displayed cached order"
            );
        }
    }

    #[test]
    fn manual_refresh_preserves_cached_library_selection_filters_and_playback() {
        let first = Track::new("one", "Zulu", "Artist", "Album", Duration::from_secs(1));
        let second = Track::new("two", "Alpha", "Artist", "Album", Duration::from_secs(1));
        let playback = crate::domain::PlaybackSnapshot {
            current_track: Some(first.clone()),
            context: crate::domain::PlaybackContext::Album {
                album_id: AlbumId::new("album"),
                ordered_track_ids: vec![first.id.clone(), second.id.clone()],
                current_index: 0,
            },
            ..Default::default()
        };
        let mut state = AppState {
            library: vec![first, second],
            library_status: crate::domain::CollectionLoadState::Cached { total: 2 },
            playback: playback.clone(),
            ..AppState::default()
        };
        state.library_views.songs.filter = "artist".to_owned();
        state.library_views.songs.sort = CollectionSort::SongTitle;
        rebuild_library_views(&mut state);
        state.content_selection = 1;

        assert_eq!(
            reduce(&mut state, Action::RefreshLibrary),
            vec![Command::Backend(BackendCommand::RefreshLibrary)]
        );
        reduce(
            &mut state,
            Action::Backend(Box::new(BackendEvent::Update(
                BackendUpdate::LibraryRefreshStarted {
                    availability: crate::domain::BackendAvailability::Available,
                    playback: playback.clone(),
                },
            ))),
        );
        assert!(matches!(
            state.library_status,
            crate::domain::CollectionLoadState::Refreshing { .. }
        ));
        assert_eq!(state.library_views.songs.filter, "artist");
        assert_eq!(state.playback, playback);
        assert_eq!(state.library.len(), 2);

        reduce(
            &mut state,
            Action::Backend(Box::new(BackendEvent::Update(
                BackendUpdate::LibraryRefreshFailed {
                    availability: crate::domain::BackendAvailability::Available,
                    playback,
                    message: "safe failure".to_owned(),
                },
            ))),
        );
        assert!(matches!(
            state.library_status,
            crate::domain::CollectionLoadState::Error(_)
        ));
        assert_eq!(state.library.len(), 2);
        assert_eq!(state.library_views.songs.filter, "artist");
    }

    #[test]
    fn full_now_playing_uses_history_and_q_returns_without_quitting() {
        let track = Track::new(
            "track",
            "Current",
            "Artist",
            "Album",
            Duration::from_secs(60),
        );
        let mut state = AppState {
            navigation: crate::app::state::NavigationState {
                active: Route::Section(Screen::Songs),
                history: Vec::new(),
            },
            content_selection: 4,
            playback: crate::domain::PlaybackSnapshot {
                current_track: Some(track),
                ..Default::default()
            },
            ..AppState::default()
        };
        reduce(&mut state, Action::OpenNowPlaying);
        assert!(matches!(state.navigation.active, Route::NowPlaying));
        assert_eq!(state.navigation.history.len(), 1);
        reduce(&mut state, Action::Quit);
        assert!(!state.should_quit);
        assert_eq!(state.navigation.active, Route::Section(Screen::Songs));
        assert_eq!(state.content_selection, 4);

        reduce(&mut state, Action::OpenNowPlaying);
        reduce(&mut state, Action::Back);
        assert_eq!(state.navigation.active, Route::Section(Screen::Songs));
        assert_eq!(state.content_selection, 4);
    }

    #[tokio::test]
    async fn play_selected_album_preserves_derived_track_order() {
        let mut state = loaded_mock_state().await;
        reduce(&mut state, Action::GoTo(Screen::Albums));

        assert_eq!(
            reduce(&mut state, Action::PlaySelectedCollection),
            vec![Command::Backend(BackendCommand::PlayAlbum {
                album_id: AlbumId::new("mock-album-event-loop"),
                track_ids: vec![TrackId::new("mock-001"), TrackId::new("mock-003")],
            })]
        );
    }

    #[test]
    fn album_detail_lazy_loads_and_caches_bounded_artwork_by_album_identity() {
        let album_id = AlbumId::new("album-art");
        let track_id = TrackId::new("musicapp:persistent:ART");
        let album = Album::new(
            album_id.to_string(),
            "Artwork Album",
            "Artist",
            2026,
            "2026-01-01",
            vec![Track::new(
                track_id.to_string(),
                "Track",
                "Artist",
                "Artwork Album",
                Duration::from_secs(60),
            )],
        );
        let mut state = AppState {
            navigation: crate::app::state::NavigationState {
                active: Route::Section(Screen::Albums),
                history: Vec::new(),
            },
            focus: Focus::Content,
            capabilities: Capabilities::macos(),
            albums: vec![album],
            ..AppState::default()
        };

        assert_eq!(
            reduce(&mut state, Action::OpenSelected),
            vec![Command::Backend(BackendCommand::LoadTrackArtwork {
                key: ArtworkKey::Album(album_id.clone()),
                track_id,
            })]
        );
        assert!(matches!(
            state
                .artwork_cache
                .get(&ArtworkKey::Album(album_id.clone())),
            Some(crate::app::state::ArtworkCacheEntry::Loading)
        ));

        reduce(
            &mut state,
            Action::Backend(Box::new(BackendEvent::Update(BackendUpdate::Artwork {
                availability: crate::domain::BackendAvailability::Available,
                playback: PlaybackSnapshot::default(),
                key: ArtworkKey::Album(album_id.clone()),
                result: ArtworkResult::Ready(Artwork {
                    media_type: ArtworkMediaType::Jpeg,
                    bytes: vec![0xff, 0xd8, 0xff, 0xd9],
                }),
            }))),
        );
        assert!(matches!(
            state.artwork_cache.get(&ArtworkKey::Album(album_id)),
            Some(crate::app::state::ArtworkCacheEntry::Ready(_))
        ));
    }

    #[test]
    fn jpeg_artwork_conversion_is_queued_once_and_cached_by_identity() {
        let key = ArtworkKey::Album(AlbumId::new("album-renderable"));
        let artwork = Artwork {
            media_type: ArtworkMediaType::Jpeg,
            bytes: vec![0xff, 0xd8, 0xff, 0xd9],
        };
        let mut state = AppState::default();
        let event = || {
            BackendEvent::Update(BackendUpdate::Artwork {
                availability: crate::domain::BackendAvailability::Available,
                playback: PlaybackSnapshot::default(),
                key: key.clone(),
                result: ArtworkResult::Ready(artwork.clone()),
            })
        };

        let commands = reduce(&mut state, Action::Backend(Box::new(event())));
        assert!(matches!(
            commands.as_slice(),
            [Command::ConvertArtwork { key: command_key, .. }] if command_key == &key
        ));
        assert!(reduce(&mut state, Action::Backend(Box::new(event()))).is_empty());

        reduce(
            &mut state,
            Action::ArtworkConversionCompleted {
                key: key.clone(),
                source_fingerprint: artwork.fingerprint(),
                result: Ok(Artwork {
                    media_type: ArtworkMediaType::Png,
                    bytes: b"\x89PNG\r\n\x1a\nconverted".to_vec(),
                }),
            },
        );
        assert!(matches!(
            state.renderable_artwork_cache.get(&key),
            Some(RenderableArtworkCacheEntry::Ready { artwork, .. })
                if artwork.media_type == ArtworkMediaType::Png
        ));
    }

    #[test]
    fn renderable_artwork_cache_is_bounded_with_source_cache() {
        let mut state = AppState::default();
        for index in 0..17 {
            let key = ArtworkKey::Track(TrackId::new(format!("track-{index}")));
            insert_artwork_cache(
                &mut state,
                key.clone(),
                ArtworkCacheEntry::Ready(Artwork {
                    media_type: ArtworkMediaType::Png,
                    bytes: vec![index as u8],
                }),
            );
            insert_renderable_artwork_cache(
                &mut state,
                key,
                RenderableArtworkCacheEntry::Ready {
                    source_fingerprint: index,
                    artwork: Artwork {
                        media_type: ArtworkMediaType::Png,
                        bytes: vec![index as u8],
                    },
                },
            );
        }
        assert_eq!(state.artwork_cache.len(), 16);
        assert_eq!(state.renderable_artwork_cache.len(), 16);
        assert!(
            !state
                .renderable_artwork_cache
                .contains_key(&ArtworkKey::Track(TrackId::new("track-0")))
        );
    }

    #[test]
    fn album_detail_reuses_a_matching_now_playing_renderable() {
        let track_key = ArtworkKey::Track(TrackId::new("track-artwork"));
        let album_key = ArtworkKey::Album(AlbumId::new("album-artwork"));
        let source = Artwork {
            media_type: ArtworkMediaType::Jpeg,
            bytes: vec![0xff, 0xd8, 0xff, 0xd9],
        };
        let renderable = Artwork {
            media_type: ArtworkMediaType::Png,
            bytes: b"\x89PNG\r\n\x1a\nconverted".to_vec(),
        };
        let mut state = AppState::default();
        insert_artwork_cache(
            &mut state,
            track_key,
            ArtworkCacheEntry::Ready(source.clone()),
        );
        insert_renderable_artwork_cache(
            &mut state,
            ArtworkKey::Track(TrackId::new("track-artwork")),
            RenderableArtworkCacheEntry::Ready {
                source_fingerprint: source.fingerprint(),
                artwork: renderable.clone(),
            },
        );

        let commands = reduce(
            &mut state,
            Action::Backend(Box::new(BackendEvent::Update(BackendUpdate::Artwork {
                availability: crate::domain::BackendAvailability::Available,
                playback: PlaybackSnapshot::default(),
                key: album_key.clone(),
                result: ArtworkResult::Ready(source),
            }))),
        );
        assert!(commands.is_empty());
        assert!(matches!(
            state.renderable_artwork_cache.get(&album_key),
            Some(RenderableArtworkCacheEntry::Ready { artwork, .. }) if artwork == &renderable
        ));
    }

    #[test]
    fn now_playing_requests_artwork_once_per_stable_track_identity() {
        let track = Track::new(
            "musicapp:persistent:NOW",
            "Now",
            "Artist",
            "Album",
            Duration::from_secs(60),
        );
        let mut state = AppState {
            capabilities: Capabilities::macos(),
            ..AppState::default()
        };
        let event = || {
            BackendEvent::Update(BackendUpdate::Playback {
                availability: crate::domain::BackendAvailability::Available,
                playback: PlaybackSnapshot {
                    current_track: Some(track.clone()),
                    ..PlaybackSnapshot::default()
                },
            })
        };
        assert_eq!(
            reduce(&mut state, Action::Backend(Box::new(event()))),
            vec![Command::Backend(BackendCommand::LoadTrackArtwork {
                key: ArtworkKey::Track(track.id.clone()),
                track_id: track.id.clone(),
            })]
        );
        assert!(reduce(&mut state, Action::Backend(Box::new(event()))).is_empty());
    }

    #[test]
    fn transient_current_track_artwork_failure_retries_once_then_accepts_success() {
        let track = Track::new(
            "musicapp:persistent:RETRY",
            "Retry",
            "Artist",
            "Album",
            Duration::from_secs(60),
        );
        let key = ArtworkKey::Track(track.id.clone());
        let playback = PlaybackSnapshot {
            current_track: Some(track.clone()),
            ..PlaybackSnapshot::default()
        };
        let mut state = AppState {
            capabilities: Capabilities::macos(),
            ..AppState::default()
        };
        assert!(matches!(
            reduce(
                &mut state,
                Action::Backend(Box::new(BackendEvent::Update(BackendUpdate::Playback {
                    availability: crate::domain::BackendAvailability::Available,
                    playback: playback.clone(),
                })))
            ),
            commands if matches!(commands.as_slice(), [Command::Backend(BackendCommand::LoadTrackArtwork { key: request_key, .. })] if request_key == &key)
        ));

        assert!(matches!(
            reduce(
                &mut state,
                Action::Backend(Box::new(BackendEvent::Update(BackendUpdate::Artwork {
                    availability: crate::domain::BackendAvailability::Available,
                    playback: playback.clone(),
                    key: key.clone(),
                    result: ArtworkResult::Invalid("Music.app artwork query failed: stale object".to_owned()),
                })))
            ),
            commands if matches!(commands.as_slice(), [Command::RetryArtwork { key: retry_key, .. }] if retry_key == &key)
        ));
        assert!(matches!(
            state.artwork_cache.get(&key),
            Some(ArtworkCacheEntry::Loading)
        ));

        reduce(
            &mut state,
            Action::Backend(Box::new(BackendEvent::Update(BackendUpdate::Artwork {
                availability: crate::domain::BackendAvailability::Available,
                playback,
                key: key.clone(),
                result: ArtworkResult::Ready(Artwork {
                    media_type: ArtworkMediaType::Png,
                    bytes: b"\x89PNG\r\n\x1a\nvalid".to_vec(),
                }),
            }))),
        );
        assert!(matches!(
            state.artwork_cache.get(&key),
            Some(ArtworkCacheEntry::Ready(_))
        ));
        assert!(!state.artwork_retry_attempts.contains_key(&key));
    }

    #[test]
    fn typed_transient_artwork_resolution_retries_once() {
        let track = Track::new(
            "musicapp:persistent:CLOUD",
            "Cloud Track",
            "Artist",
            "Album",
            Duration::from_secs(60),
        );
        let key = ArtworkKey::Track(track.id.clone());
        let playback = PlaybackSnapshot {
            current_track: Some(track),
            ..PlaybackSnapshot::default()
        };
        let mut state = AppState {
            capabilities: Capabilities::macos(),
            ..AppState::default()
        };
        reduce(
            &mut state,
            Action::Backend(Box::new(BackendEvent::Update(BackendUpdate::Playback {
                availability: crate::domain::BackendAvailability::Available,
                playback: playback.clone(),
            }))),
        );

        let transient = || {
            Action::Backend(Box::new(BackendEvent::Update(BackendUpdate::Artwork {
                availability: crate::domain::BackendAvailability::Available,
                playback: playback.clone(),
                key: key.clone(),
                result: ArtworkResult::Transient("fresh object unavailable".to_owned()),
            })))
        };
        assert!(matches!(
            reduce(&mut state, transient()).as_slice(),
            [Command::RetryArtwork { .. }]
        ));
        assert!(reduce(&mut state, transient()).is_empty());
        assert!(matches!(
            state.artwork_cache.get(&key),
            Some(ArtworkCacheEntry::Transient(message)) if message.contains("fresh object unavailable")
        ));
    }

    #[test]
    fn second_transient_artwork_failure_is_a_stable_fallback() {
        let track = Track::new(
            "musicapp:persistent:RETRY-ONCE",
            "Retry Once",
            "Artist",
            "Album",
            Duration::from_secs(60),
        );
        let key = ArtworkKey::Track(track.id.clone());
        let playback = PlaybackSnapshot {
            current_track: Some(track.clone()),
            ..PlaybackSnapshot::default()
        };
        let mut state = AppState {
            capabilities: Capabilities::macos(),
            ..AppState::default()
        };
        reduce(
            &mut state,
            Action::Backend(Box::new(BackendEvent::Update(BackendUpdate::Playback {
                availability: crate::domain::BackendAvailability::Available,
                playback: playback.clone(),
            }))),
        );
        let failure = || {
            Action::Backend(Box::new(BackendEvent::Update(BackendUpdate::Artwork {
                availability: crate::domain::BackendAvailability::Available,
                playback: playback.clone(),
                key: key.clone(),
                result: ArtworkResult::Invalid(
                    "Music.app artwork query failed: temporary".to_owned(),
                ),
            })))
        };
        assert!(matches!(
            reduce(&mut state, failure()).as_slice(),
            [Command::RetryArtwork { .. }]
        ));
        assert!(reduce(&mut state, failure()).is_empty());
        assert!(matches!(
            state.artwork_cache.get(&key),
            Some(ArtworkCacheEntry::Unavailable(message)) if message.contains("query failed")
        ));
    }

    #[test]
    fn stale_track_artwork_event_cannot_replace_newer_playback() {
        let old = Track::new(
            "musicapp:persistent:OLD",
            "Old",
            "Artist",
            "Old",
            Duration::from_secs(60),
        );
        let new = Track::new(
            "musicapp:persistent:NEW",
            "New",
            "Artist",
            "New",
            Duration::from_secs(60),
        );
        let mut state = AppState {
            playback: PlaybackSnapshot {
                current_track: Some(new.clone()),
                ..PlaybackSnapshot::default()
            },
            ..AppState::default()
        };
        reduce(
            &mut state,
            Action::Backend(Box::new(BackendEvent::Update(BackendUpdate::Artwork {
                availability: crate::domain::BackendAvailability::Available,
                playback: PlaybackSnapshot {
                    current_track: Some(old.clone()),
                    ..PlaybackSnapshot::default()
                },
                key: ArtworkKey::Track(old.id.clone()),
                result: ArtworkResult::Missing,
            }))),
        );
        assert_eq!(
            state.playback.current_track.as_ref().map(|track| &track.id),
            Some(&new.id)
        );
    }

    #[test]
    fn same_album_track_change_reuses_the_album_artwork_identity() {
        let first = Track::new(
            "musicapp:persistent:ONE",
            "One",
            "Artist",
            "Album",
            Duration::from_secs(60),
        );
        let second = Track::new(
            "musicapp:persistent:TWO",
            "Two",
            "Artist",
            "Album",
            Duration::from_secs(60),
        );
        let album = Album::new(
            "album-shared",
            "Album",
            "Artist",
            2026,
            "2026",
            vec![first.clone(), second.clone()],
        );
        let key = ArtworkKey::Album(album.id.clone());
        let mut state = AppState {
            capabilities: Capabilities::macos(),
            albums: vec![album],
            playback: PlaybackSnapshot {
                current_track: Some(first.clone()),
                ..PlaybackSnapshot::default()
            },
            ..AppState::default()
        };
        insert_artwork_cache(
            &mut state,
            key,
            ArtworkCacheEntry::Ready(Artwork {
                media_type: ArtworkMediaType::Png,
                bytes: b"\x89PNG\r\n\x1a\nshared".to_vec(),
            }),
        );
        assert!(
            reduce(
                &mut state,
                Action::Backend(Box::new(BackendEvent::Update(BackendUpdate::Playback {
                    availability: crate::domain::BackendAvailability::Available,
                    playback: PlaybackSnapshot {
                        current_track: Some(second),
                        ..PlaybackSnapshot::default()
                    },
                })))
            )
            .is_empty()
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
                ordered_track_ids: vec![track_id],
                selected_index: 0,
                complete: true,
            })]
        );
    }

    #[test]
    fn jump_to_playing_playlist_track_uses_the_session_source_occurrence_without_commands() {
        let playlist_id = PlaylistId::new("musicapp:playlist:persistent:ACTIVE");
        let duplicate = Track::new(
            "musicapp:persistent:DUPLICATE",
            "Duplicate",
            "Artist",
            "Album",
            Duration::from_secs(60),
        );
        let playlist = Playlist::new(
            playlist_id.to_string(),
            "Same Name",
            None,
            vec![
                duplicate.clone(),
                Track::new(
                    "middle",
                    "Middle",
                    "Artist",
                    "Album",
                    Duration::from_secs(60),
                ),
                duplicate.clone(),
                Track::new("last", "Last", "Artist", "Album", Duration::from_secs(60)),
            ],
        );
        let mut state = AppState {
            navigation: crate::app::state::NavigationState {
                active: Route::PlaylistDetail {
                    playlist_id: playlist_id.clone(),
                },
                history: Vec::new(),
            },
            focus: Focus::Content,
            content_selection: 0,
            playlists: vec![playlist],
            playback: PlaybackSnapshot {
                current_track: Some(duplicate),
                shuffle: true,
                context: PlaybackContext::Playlist {
                    playlist_id,
                    // Playback order is deliberately not canonical under shuffle.
                    ordered_track_ids: vec![
                        TrackId::new("middle"),
                        TrackId::new("musicapp:persistent:DUPLICATE"),
                        TrackId::new("last"),
                        TrackId::new("musicapp:persistent:DUPLICATE"),
                    ],
                    current_index: 1,
                    current_source_index: 2,
                    complete: true,
                },
                ..PlaybackSnapshot::default()
            },
            ..AppState::default()
        };

        assert!(reduce(&mut state, Action::JumpToPlayingTrack).is_empty());
        assert_eq!(
            state.content_selection, 2,
            "must not select the first duplicate"
        );
        assert!(matches!(
            state.playback.context,
            PlaybackContext::Playlist {
                current_index: 1,
                current_source_index: 2,
                ..
            }
        ));
    }

    #[test]
    fn jump_to_playing_playlist_track_is_a_silent_no_op_for_other_or_unloaded_playlists() {
        let active_id = PlaylistId::new("active");
        let other_id = PlaylistId::new("other");
        let mut state = AppState {
            navigation: crate::app::state::NavigationState {
                active: Route::PlaylistDetail {
                    playlist_id: active_id.clone(),
                },
                history: Vec::new(),
            },
            content_selection: 1,
            playlists: vec![playlist(&active_id.to_string(), "Duplicate Name")],
            playback: PlaybackSnapshot {
                context: PlaybackContext::Playlist {
                    playlist_id: other_id,
                    ordered_track_ids: vec![TrackId::new("other-track")],
                    current_index: 0,
                    current_source_index: 0,
                    complete: true,
                },
                ..PlaybackSnapshot::default()
            },
            ..AppState::default()
        };
        assert!(reduce(&mut state, Action::JumpToPlayingTrack).is_empty());
        assert_eq!(state.content_selection, 1);

        state.playback.context = PlaybackContext::NoContext;
        assert!(reduce(&mut state, Action::JumpToPlayingTrack).is_empty());
        assert_eq!(state.content_selection, 1);

        state.playback.context = PlaybackContext::Playlist {
            playlist_id: active_id,
            ordered_track_ids: vec![TrackId::new("not-loaded")],
            current_index: 0,
            current_source_index: 5,
            complete: false,
        };
        assert!(reduce(&mut state, Action::JumpToPlayingTrack).is_empty());
        assert_eq!(state.content_selection, 1);
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
    fn help_is_modal_and_q_closes_it_without_quitting() {
        let mut state = AppState::default();
        reduce(&mut state, Action::ToggleHelp);
        assert!(state.help_open);
        assert!(!state.should_quit);

        reduce(&mut state, Action::Quit);
        assert!(!state.help_open);
        assert!(!state.should_quit);

        reduce(&mut state, Action::Quit);
        assert!(state.should_quit);
    }

    #[test]
    fn help_toggle_back_and_navigation_control_its_scroll_state() {
        let mut state = AppState::default();
        reduce(&mut state, Action::ToggleHelp);
        reduce(&mut state, Action::MoveDown);
        reduce(&mut state, Action::PageDown);
        assert_eq!(state.help_scroll, 2);
        reduce(&mut state, Action::JumpToStart);
        assert_eq!(state.help_scroll, 0);
        reduce(&mut state, Action::Back);
        assert!(!state.help_open);
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
