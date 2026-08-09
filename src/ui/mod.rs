pub mod artwork;
mod theme;

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Gauge, List, ListItem, ListState, Paragraph, Wrap},
};
use std::ops::Range;

use crate::{
    app::state::{
        AppState, BackendStatus, CollectionKind, CollectionViewState, ContextAction, Focus,
        LocalSearchResult, QUEUE_PANE_MIN_WIDTH, Route, Screen, ViewStatus,
    },
    backend::capabilities::Capability,
    domain::{
        Album, Artist, ArtworkKey, BackendAvailability, CollectionLoadState, DataOrigin,
        PlaybackContext, PlaybackStatus, PlaylistKind, PlaylistLoadState, RepeatMode, Station,
        Track,
    },
    input::{BindingGroup, bindings},
};

use self::theme::{DEFAULT, Theme};

pub fn render(frame: &mut Frame<'_>, state: &AppState) {
    let _ = render_with_artwork_layout(frame, state);
}

pub(crate) fn render_with_artwork_layout(
    frame: &mut Frame<'_>,
    state: &AppState,
) -> Option<artwork::ArtworkLayout> {
    let area = frame.area();
    frame.render_widget(
        Block::default().style(Style::default().bg(DEFAULT.background)),
        area,
    );

    if matches!(state.navigation.active, Route::NowPlaying) {
        let artwork = render_full_now_playing(frame, area, state, DEFAULT);
        if state.help_open {
            render_help(frame, area, state, DEFAULT);
            return None;
        }
        return artwork;
    }

    let player_height = if area.height >= 30 { 9 } else { 7 };
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(player_height)])
        .split(area);

    let detail_artwork = render_body(frame, vertical[0], state, DEFAULT);
    let player_artwork = render_player(frame, vertical[1], state, DEFAULT);

    if state.help_open {
        render_help(frame, area, state, DEFAULT);
        return None;
    }
    if state.action_menu.is_some() {
        render_action_menu(frame, area, state, DEFAULT);
        return None;
    }
    if state.sort_menu.is_some() {
        render_sort_menu(frame, area, state, DEFAULT);
        return None;
    }
    if state.filter_editor.is_some() {
        render_filter_editor(frame, area, state, DEFAULT);
        return None;
    }
    if state.playlist_track_removal_confirmation.is_some() {
        render_playlist_track_removal_confirmation(frame, area, state, DEFAULT);
        return None;
    }
    detail_artwork.or(player_artwork)
}

fn render_full_now_playing(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &AppState,
    theme: Theme,
) -> Option<artwork::ArtworkLayout> {
    let block = Block::default()
        .title(" Now Playing ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .style(Style::default().bg(theme.background));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let Some(track) = state.playback.current_track.as_ref() else {
        frame.render_widget(
            Paragraph::new("▶  Nothing Playing\n\nMusic.app is idle")
                .style(Style::default().fg(theme.muted))
                .alignment(Alignment::Center),
            inner,
        );
        return None;
    };

    let layout = full_now_playing_layout(inner.width, inner.height);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(layout.artwork_height),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);
    let artwork_layout = if layout.show_artwork {
        let artwork_width = layout.artwork_width.min(rows[0].width);
        let artwork_area = Rect::new(
            rows[0]
                .x
                .saturating_add(rows[0].width.saturating_sub(artwork_width) / 2),
            rows[0].y,
            artwork_width,
            rows[0].height,
        );
        render_now_playing_artwork_panel(
            frame,
            artwork_area,
            state,
            state.artwork_key_for_track(&track.id),
            theme,
        )
    } else {
        None
    };
    let control = playback_control_icon(state.playback.status);
    frame.render_widget(
        Paragraph::new(format!(
            "{control}  {}",
            truncate_cell(&track.title, usize::from(inner.width))
        ))
        .style(
            Style::default()
                .fg(theme.foreground)
                .add_modifier(Modifier::BOLD),
        )
        .alignment(Alignment::Center),
        rows[1],
    );
    frame.render_widget(
        Paragraph::new(truncate_cell(&track.artist, usize::from(inner.width)))
            .style(Style::default().fg(theme.foreground))
            .alignment(Alignment::Center),
        rows[2],
    );
    frame.render_widget(
        Paragraph::new(truncate_cell(&track.album, usize::from(inner.width)))
            .style(Style::default().fg(theme.muted))
            .alignment(Alignment::Center),
        rows[3],
    );
    frame.render_widget(
        Paragraph::new(full_playback_context(state))
            .style(Style::default().fg(theme.accent))
            .alignment(Alignment::Center),
        rows[4],
    );
    let duration = track.duration.as_secs();
    let position = state.playback.position.as_secs().min(duration);
    let ratio = if duration == 0 {
        0.0
    } else {
        position as f64 / duration as f64
    };
    frame.render_widget(
        Gauge::default()
            .gauge_style(Style::default().fg(theme.accent).bg(theme.selection))
            .ratio(ratio)
            .label(format!(
                "{} / {}",
                format_time(position),
                format_time(duration)
            )),
        rows[6],
    );
    frame.render_widget(
        Paragraph::new(full_now_playing_status(state, track, inner.width))
            .style(Style::default().fg(theme.muted))
            .alignment(Alignment::Center),
        rows[7],
    );
    artwork_layout
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FullNowPlayingLayout {
    show_artwork: bool,
    artwork_width: u16,
    artwork_height: u16,
}

const fn full_now_playing_layout(width: u16, height: u16) -> FullNowPlayingLayout {
    if width >= 96 && height >= 28 {
        FullNowPlayingLayout {
            show_artwork: true,
            artwork_width: 36,
            artwork_height: 15,
        }
    } else if width >= 72 && height >= 22 {
        FullNowPlayingLayout {
            show_artwork: true,
            artwork_width: 24,
            artwork_height: 9,
        }
    } else if width >= 54 && height >= 18 {
        FullNowPlayingLayout {
            show_artwork: true,
            artwork_width: 18,
            artwork_height: 5,
        }
    } else {
        FullNowPlayingLayout {
            show_artwork: false,
            artwork_width: 0,
            artwork_height: 0,
        }
    }
}

fn full_playback_context(state: &AppState) -> String {
    match &state.playback.context {
        PlaybackContext::NoContext => String::new(),
        PlaybackContext::Playlist {
            ordered_track_ids,
            current_index,
            ..
        } => {
            format!(
                "Playlist · {} / {}",
                current_index + 1,
                ordered_track_ids.len()
            )
        }
        PlaybackContext::Album {
            ordered_track_ids,
            current_index,
            ..
        } => {
            format!(
                "Album · {} / {}",
                current_index + 1,
                ordered_track_ids.len()
            )
        }
    }
}

fn full_now_playing_status(state: &AppState, track: &Track, width: u16) -> String {
    let repeat = match state.playback.repeat {
        RepeatMode::Off => "off",
        RepeatMode::All => "all",
        RepeatMode::One => "one",
    };
    let mut fields = vec![
        format!(
            "{}",
            match state.playback.status {
                PlaybackStatus::Playing => "Playing",
                PlaybackStatus::Paused => "Paused",
                PlaybackStatus::Stopped => "Stopped",
            }
        ),
        format!("vol {}%", state.playback.volume),
        format!(
            "shuffle {}",
            if state.playback.shuffle { "on" } else { "off" }
        ),
        format!("repeat {repeat}"),
    ];
    if track.is_favorite {
        fields.push("♥".to_owned());
    }
    if let Some(year) = track.metadata.year {
        fields.push(year.to_string());
    }
    if let Some(genre) = track
        .metadata
        .genre
        .as_deref()
        .filter(|genre| !genre.is_empty())
    {
        fields.push(genre.to_owned());
    }
    truncate_cell(&fields.join("  •  "), usize::from(width))
}

fn render_body(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &AppState,
    theme: Theme,
) -> Option<artwork::ArtworkLayout> {
    if area.width >= QUEUE_PANE_MIN_WIDTH && state.capabilities.supports(Capability::QueueRead) {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(22),
                Constraint::Min(32),
                Constraint::Length(32),
            ])
            .split(area);
        render_sidebar(frame, columns[0], state, theme);
        let artwork = render_content(frame, columns[1], state, theme);
        render_queue(frame, columns[2], state, theme);
        artwork
    } else {
        let sidebar_width = area.width.min(22);
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(sidebar_width), Constraint::Min(1)])
            .split(area);
        render_sidebar(frame, columns[0], state, theme);
        render_content(frame, columns[1], state, theme)
    }
}

fn render_sidebar(frame: &mut Frame<'_>, area: Rect, state: &AppState, theme: Theme) {
    let mut items = Vec::new();
    for index in 0..3 {
        items.push(sidebar_screen_item(index, state, theme));
    }
    items.push(sidebar_heading("LIBRARY", theme));
    for index in 3..9 {
        items.push(sidebar_screen_item(index, state, theme));
    }
    items.push(sidebar_heading("PLAYLISTS", theme));
    items.push(sidebar_screen_item(9, state, theme));
    items.push(sidebar_heading("LOCAL", theme));
    items.push(sidebar_screen_item(10, state, theme));

    let list = List::new(items)
        .block(panel_block(
            sidebar_title(state),
            state.focus == Focus::Sidebar,
            theme,
        ))
        .style(Style::default().fg(theme.foreground));
    let selected_row = match state.sidebar_selection {
        0..=2 => state.sidebar_selection,
        3..=8 => state.sidebar_selection + 1,
        9 => 11,
        10 => 13,
        _ => 0,
    };
    let mut list_state = ListState::default().with_selected(Some(selected_row));
    frame.render_stateful_widget(list, area, &mut list_state);
}

fn sidebar_title(state: &AppState) -> &'static str {
    match &state.backend_status {
        BackendStatus::Ready { name } if name.starts_with("Mock") => " Apple Music • MOCK ",
        _ => " Apple Music ",
    }
}

fn sidebar_screen_item(index: usize, state: &AppState, theme: Theme) -> ListItem<'static> {
    let screen = Screen::ALL[index];
    let selected = index == state.sidebar_selection;
    let marker = if selected { "› " } else { "  " };
    let style = if selected {
        Style::default()
            .bg(theme.selection)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    ListItem::new(Line::from(vec![
        Span::styled(marker, Style::default().fg(theme.accent)),
        Span::raw(screen.label()),
    ]))
    .style(style)
}

fn sidebar_heading(title: &'static str, theme: Theme) -> ListItem<'static> {
    ListItem::new(Line::styled(
        format!("  {title}"),
        Style::default()
            .fg(theme.muted)
            .add_modifier(Modifier::BOLD),
    ))
}

fn render_content(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &AppState,
    theme: Theme,
) -> Option<artwork::ArtworkLayout> {
    let title = format!(" {} ", state.navigation.active.label());
    let block = panel_block(&title, state.focus == Focus::Content, theme);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(1)])
        .split(inner);
    frame.render_widget(render_backend_status(state, theme), rows[0]);

    match &state.view_status {
        ViewStatus::Loading => {
            frame.render_widget(
                Paragraph::new("Loading backend…")
                    .style(Style::default().fg(theme.muted))
                    .alignment(Alignment::Center),
                rows[1],
            );
            None
        }
        ViewStatus::Empty => {
            frame.render_widget(
                Paragraph::new("No items")
                    .style(Style::default().fg(theme.muted))
                    .alignment(Alignment::Center),
                rows[1],
            );
            None
        }
        ViewStatus::Error(message) => {
            frame.render_widget(
                Paragraph::new(message.as_str()).style(Style::default().fg(theme.error)),
                rows[1],
            );
            None
        }
        ViewStatus::Loaded => render_loaded_content(frame, rows[1], state, theme),
    }
}

fn render_loaded_content(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &AppState,
    theme: Theme,
) -> Option<artwork::ArtworkLayout> {
    if let Route::AlbumDetail { album_id } = &state.navigation.active {
        if let Some(album) = state.albums.iter().find(|album| album.id == *album_id) {
            return render_album_detail(frame, area, state, album, theme);
        }
        render_missing_detail(frame, area, "album", theme);
        return None;
    }
    match &state.navigation.active {
        Route::Section(Screen::ListenNow) => render_listen_now(frame, area, state, theme),
        Route::Section(Screen::Browse) => render_browse(frame, area, state, theme),
        Route::Section(Screen::Radio) => render_stations(frame, area, state, theme),
        Route::Section(Screen::RecentlyAdded) => {
            if state.recently_added.is_empty() && collection_is_loading(&state.library_status) {
                render_collection_loading(frame, area, "recently added albums", theme);
            } else {
                render_albums(
                    frame,
                    area,
                    &state.recently_added,
                    state.content_selection,
                    true,
                    theme,
                );
            }
        }
        Route::Section(Screen::RecentlyPlayed) => {
            if state.recently_played.is_empty() && collection_is_loading(&state.library_status) {
                render_collection_loading(frame, area, "local play history", theme);
            } else {
                render_recently_played(frame, area, state, theme);
            }
        }
        Route::Section(Screen::Artists) => {
            if state.artists.is_empty() && collection_is_loading(&state.library_status) {
                render_collection_loading(frame, area, "artists", theme);
            } else {
                render_library_artists(frame, area, state, theme);
            }
        }
        Route::Section(Screen::Albums) => {
            if state.albums.is_empty() && collection_is_loading(&state.library_status) {
                render_collection_loading(frame, area, "albums", theme);
            } else {
                render_library_albums(frame, area, state, theme);
            }
        }
        Route::Section(Screen::Songs) => {
            if state.library.is_empty() && collection_is_loading(&state.library_status) {
                render_collection_loading(frame, area, "songs", theme);
            } else {
                render_library_songs(frame, area, state, theme);
            }
        }
        Route::Section(Screen::Search) => render_search(frame, area, state, theme),
        Route::NowPlaying => {
            unreachable!("full-screen Now Playing renders outside the normal body")
        }
        Route::Section(Screen::MadeForYou) => render_playlists(frame, area, state, theme),
        Route::Section(Screen::Playlists) => render_playlists(frame, area, state, theme),
        Route::ArtistDetail { artist_id } => {
            let artist = state.artists.iter().find(|artist| artist.id == *artist_id);
            if let Some(artist) = artist {
                render_artist_detail(frame, area, state, artist, theme);
            } else {
                render_missing_detail(frame, area, "artist", theme);
            }
        }
        Route::AlbumDetail { .. } => unreachable!("handled before the shared content match"),
        Route::PlaylistDetail { playlist_id } => {
            let playlist = state
                .playlists
                .iter()
                .find(|playlist| playlist.id == *playlist_id);
            if let Some(playlist) = playlist {
                render_playlist_detail(frame, area, state, playlist, theme);
            } else {
                render_missing_detail(frame, area, "playlist", theme);
            }
        }
    }
    None
}

fn render_recently_played(frame: &mut Frame<'_>, area: Rect, state: &AppState, theme: Theme) {
    if state.recently_played.is_empty() {
        frame.render_widget(
            Paragraph::new("No local Music.app play dates are available")
                .style(Style::default().fg(theme.muted))
                .alignment(Alignment::Center),
            area,
        );
        return;
    }
    let range = visible_item_range(
        state.recently_played.len(),
        state.content_selection,
        area.height,
        1,
    );
    let items = state
        .recently_played
        .iter()
        .skip(range.start)
        .take(range.len())
        .map(|entry| {
            let suffix = entry.play_count.map_or_else(
                || entry.played_at.clone(),
                |count| format!("{} • {count} plays", entry.played_at),
            );
            ListItem::new(Line::from(vec![
                Span::styled(
                    truncate_cell(&entry.title, recently_played_title_width(area.width)),
                    Style::default()
                        .fg(theme.foreground)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("  {}  {suffix}", entry.artist),
                    Style::default().fg(theme.muted),
                ),
            ]))
        })
        .collect();
    render_selectable_list(
        frame,
        area,
        items,
        state.content_selection.saturating_sub(range.start),
        theme,
    );
}

fn render_search(frame: &mut Frame<'_>, area: Rect, state: &AppState, theme: Theme) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(1)])
        .split(area);
    let cursor = if state.search_input_active { "▌" } else { "" };
    frame.render_widget(
        Paragraph::new(format!("/ {}{cursor}", state.search_query))
            .style(Style::default().fg(theme.accent)),
        rows[0],
    );
    if state.search_query.trim().is_empty() {
        frame.render_widget(
            Paragraph::new("Type a track, artist, album, playlist, composer, or genre")
                .style(Style::default().fg(theme.muted))
                .alignment(Alignment::Center),
            rows[1],
        );
        return;
    }
    let range = visible_item_range(
        state.search_results.len(),
        state.content_selection,
        rows[1].height,
        1,
    );
    let items = state
        .search_results
        .iter()
        .skip(range.start)
        .take(range.len())
        .filter_map(|result| match result {
            LocalSearchResult::Track(track_id) => state
                .library
                .iter()
                .find(|track| track.id == *track_id)
                .map(|track| {
                    ListItem::new(Line::from(vec![
                        Span::styled("TRACK     ", Style::default().fg(theme.accent)),
                        Span::styled(track.title.clone(), Style::default().fg(theme.foreground)),
                        Span::styled(
                            format!(
                                " — {} • {}",
                                track.artist,
                                format_time(track.duration.as_secs())
                            ),
                            Style::default().fg(theme.muted),
                        ),
                    ]))
                }),
            LocalSearchResult::Artist(artist_id) => state
                .artists
                .iter()
                .find(|artist| artist.id == *artist_id)
                .map(|artist| {
                    ListItem::new(Line::from(vec![
                        Span::styled("ARTIST    ", Style::default().fg(theme.accent)),
                        Span::styled(artist.name.clone(), Style::default().fg(theme.foreground)),
                        Span::styled(
                            format!(" • {} local tracks", artist.top_track_ids.len()),
                            Style::default().fg(theme.muted),
                        ),
                    ]))
                }),
            LocalSearchResult::Album(album_id) => state
                .albums
                .iter()
                .find(|album| album.id == *album_id)
                .map(|album| {
                    ListItem::new(Line::from(vec![
                        Span::styled("ALBUM     ", Style::default().fg(theme.accent)),
                        Span::styled(album.title.clone(), Style::default().fg(theme.foreground)),
                        Span::styled(
                            format!(" — {} • {} tracks", album.artist, album.tracks.len()),
                            Style::default().fg(theme.muted),
                        ),
                    ]))
                }),
            LocalSearchResult::Playlist(playlist_id) => state
                .playlists
                .iter()
                .find(|playlist| playlist.id == *playlist_id)
                .map(|playlist| {
                    ListItem::new(Line::from(vec![
                        Span::styled("PLAYLIST  ", Style::default().fg(theme.accent)),
                        Span::styled(playlist.name.clone(), Style::default().fg(theme.foreground)),
                    ]))
                }),
        })
        .collect::<Vec<_>>();
    if items.is_empty() {
        let message = if collection_is_loading(&state.library_status) {
            "No matches in the loaded library yet • Music.app is still refreshing"
        } else {
            "No local matches"
        };
        frame.render_widget(
            Paragraph::new(message)
                .style(Style::default().fg(theme.muted))
                .alignment(Alignment::Center),
            rows[1],
        );
    } else {
        render_selectable_list(
            frame,
            rows[1],
            items,
            state.content_selection.saturating_sub(range.start),
            theme,
        );
    }
}

fn collection_is_loading(status: &CollectionLoadState) -> bool {
    matches!(
        status,
        CollectionLoadState::NotStarted
            | CollectionLoadState::Loading { .. }
            | CollectionLoadState::Refreshing { .. }
    )
}

fn render_collection_loading(frame: &mut Frame<'_>, area: Rect, name: &str, theme: Theme) {
    frame.render_widget(
        Paragraph::new(format!("Loading Music.app {name}…"))
            .style(Style::default().fg(theme.muted))
            .alignment(Alignment::Center),
        area,
    );
}

fn render_listen_now(frame: &mut Frame<'_>, area: Rect, state: &AppState, theme: Theme) {
    let recently_played = state
        .playback
        .current_track
        .as_ref()
        .or_else(|| state.library.first())
        .map_or("No local tracks", |track| track.title.as_str());
    let recommended = state
        .albums
        .iter()
        .take(2)
        .map(|album| album.title.as_str())
        .collect::<Vec<_>>()
        .join(" • ");
    let made_for_you = state
        .playlists
        .first()
        .map_or("No local playlists", |playlist| playlist.name.as_str());
    render_cards(
        frame,
        area,
        &[
            ("Recently Played", recently_played.to_owned()),
            ("Recommended Albums", recommended),
            ("Made for You", made_for_you.to_owned()),
        ],
        state.content_selection,
        theme,
    );
}

fn render_browse(frame: &mut Frame<'_>, area: Rect, state: &AppState, theme: Theme) {
    let releases = state
        .albums
        .iter()
        .take(2)
        .map(|album| album.title.as_str())
        .collect::<Vec<_>>()
        .join(" • ");
    let featured = state
        .playlists
        .get(1)
        .or_else(|| state.playlists.first())
        .map_or("No local playlists", |playlist| playlist.name.as_str());
    render_cards(
        frame,
        area,
        &[
            ("New Releases", releases),
            ("Featured Playlists", featured.to_owned()),
            ("Genres", "Alternative • Electronic • Chill".to_owned()),
        ],
        state.content_selection,
        theme,
    );
}

fn render_cards(
    frame: &mut Frame<'_>,
    area: Rect,
    cards: &[(&str, String)],
    selection: usize,
    theme: Theme,
) {
    let items = cards
        .iter()
        .map(|(heading, value)| {
            ListItem::new(vec![
                Line::styled(
                    (*heading).to_owned(),
                    Style::default()
                        .fg(theme.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Line::styled(format!("  {value}"), Style::default().fg(theme.foreground)),
            ])
        })
        .collect::<Vec<_>>();
    render_selectable_list(frame, area, items, selection, theme);
}

fn render_stations(frame: &mut Frame<'_>, area: Rect, state: &AppState, theme: Theme) {
    let items = state
        .stations
        .iter()
        .map(|station| station_item(station, theme))
        .collect::<Vec<_>>();
    render_selectable_list(frame, area, items, state.content_selection, theme);
}

fn station_item(station: &Station, theme: Theme) -> ListItem<'_> {
    ListItem::new(vec![
        Line::styled(
            station.name.as_str(),
            Style::default()
                .fg(theme.foreground)
                .add_modifier(Modifier::BOLD),
        ),
        Line::styled(
            format!("  {}", station.description),
            Style::default().fg(theme.muted),
        ),
    ])
}

fn collection_status(view: &CollectionViewState, label: &str, total: usize) -> String {
    let count = if view.source_len.is_some() {
        view.indices.len()
    } else {
        total
    };
    let arrow = if view.descending { "↓" } else { "↑" };
    let filter = if view.filter.is_empty() {
        String::new()
    } else {
        format!(" · Filter: {}", view.filter)
    };
    format!(
        "{label} · {count} / {total} · Sort: {} {arrow}{filter}",
        view.sort.label()
    )
}

fn render_collection_empty(
    frame: &mut Frame<'_>,
    area: Rect,
    label: &str,
    view: &CollectionViewState,
    theme: Theme,
) {
    let message = if view.filter.is_empty() {
        format!("No {label}")
    } else {
        format!("No {label} match \"{}\"", view.filter)
    };
    frame.render_widget(
        Paragraph::new(message)
            .style(Style::default().fg(theme.muted))
            .alignment(Alignment::Center),
        area,
    );
}

fn render_library_songs(frame: &mut Frame<'_>, area: Rect, state: &AppState, theme: Theme) {
    let view = &state.library_views.songs;
    if view.source_len.is_none() {
        let tracks = state.library.iter().collect::<Vec<_>>();
        render_track_list(
            frame,
            area,
            state,
            &tracks,
            state.content_selection,
            TrackListMode::Songs,
            theme,
        );
        return;
    }
    if view.indices.is_empty() {
        render_collection_empty(frame, area, "songs", view, theme);
        return;
    }
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(1)])
        .split(area);
    frame.render_widget(
        Paragraph::new(collection_status(view, "Songs", state.library.len()))
            .style(Style::default().fg(theme.muted)),
        rows[0],
    );
    let range = visible_item_range(
        view.indices.len(),
        state.content_selection,
        rows[1].height,
        1,
    );
    tracing::debug!(
        collection = "Songs",
        selection = state.content_selection,
        viewport_start = range.start,
        visible_row = state.content_selection.saturating_sub(range.start),
        generation = view.rebuild_count,
        "collection viewport render"
    );
    let items = view
        .indices
        .iter()
        .skip(range.start)
        .take(range.len())
        .enumerate()
        .map(|(offset, source_index)| {
            compact_track_item(
                &state.library[*source_index],
                range.start + offset,
                state,
                area.width,
                TrackListMode::Songs,
                theme,
            )
        })
        .collect::<Vec<_>>();
    render_selectable_list(
        frame,
        rows[1],
        items,
        state.content_selection.saturating_sub(range.start),
        theme,
    );
}

fn render_library_artists(frame: &mut Frame<'_>, area: Rect, state: &AppState, theme: Theme) {
    let view = &state.library_views.artists;
    if view.source_len.is_none() {
        render_artists(frame, area, state, theme);
        return;
    }
    if view.indices.is_empty() {
        render_collection_empty(frame, area, "artists", view, theme);
        return;
    }
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(area);
    frame.render_widget(
        Paragraph::new(collection_status(view, "Artists", state.artists.len()))
            .style(Style::default().fg(theme.muted)),
        rows[0],
    );
    let range = visible_item_range(
        view.indices.len(),
        state.content_selection,
        rows[1].height,
        1,
    );
    let items = view
        .indices
        .iter()
        .skip(range.start)
        .take(range.len())
        .map(|index| {
            let artist = &state.artists[*index];
            ListItem::new(Line::from(vec![
                Span::styled(
                    truncate_cell(&artist.name, artist_name_width(area.width)),
                    Style::default()
                        .fg(theme.foreground)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(
                        "  {} albums  {} tracks",
                        artist.album_ids.len(),
                        artist.top_track_ids.len()
                    ),
                    Style::default().fg(theme.muted),
                ),
            ]))
        })
        .collect::<Vec<_>>();
    render_selectable_list(
        frame,
        rows[1],
        items,
        state.content_selection.saturating_sub(range.start),
        theme,
    );
}

fn render_library_albums(frame: &mut Frame<'_>, area: Rect, state: &AppState, theme: Theme) {
    let view = &state.library_views.albums;
    if view.source_len.is_none() {
        render_albums(
            frame,
            area,
            &state.albums,
            state.content_selection,
            false,
            theme,
        );
        return;
    }
    if view.indices.is_empty() {
        render_collection_empty(frame, area, "albums", view, theme);
        return;
    }
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(area);
    frame.render_widget(
        Paragraph::new(collection_status(view, "Albums", state.albums.len()))
            .style(Style::default().fg(theme.muted)),
        rows[0],
    );
    let range = visible_item_range(
        view.indices.len(),
        state.content_selection,
        rows[1].height,
        1,
    );
    let items = view
        .indices
        .iter()
        .skip(range.start)
        .take(range.len())
        .map(|index| {
            let album = &state.albums[*index];
            ListItem::new(Line::from(vec![
                Span::styled(
                    truncate_cell(&album.title, album_title_width(area.width)),
                    Style::default()
                        .fg(theme.foreground)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(
                        "  {}  {}  {} tracks",
                        album.artist,
                        album.year,
                        album.tracks.len()
                    ),
                    Style::default().fg(theme.muted),
                ),
            ]))
        })
        .collect::<Vec<_>>();
    render_selectable_list(
        frame,
        rows[1],
        items,
        state.content_selection.saturating_sub(range.start),
        theme,
    );
}

fn render_artists(frame: &mut Frame<'_>, area: Rect, state: &AppState, theme: Theme) {
    let range = visible_item_range(state.artists.len(), state.content_selection, area.height, 1);
    let items = state
        .artists
        .iter()
        .skip(range.start)
        .take(range.len())
        .map(|artist| {
            ListItem::new(Line::from(vec![
                Span::styled(
                    truncate_cell(&artist.name, artist_name_width(area.width)),
                    Style::default()
                        .fg(theme.foreground)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(
                        "  {} albums  {} tracks",
                        artist.album_ids.len(),
                        artist.top_track_ids.len()
                    ),
                    Style::default().fg(theme.muted),
                ),
            ]))
        })
        .collect::<Vec<_>>();
    render_selectable_list(
        frame,
        area,
        items,
        state.content_selection.saturating_sub(range.start),
        theme,
    );
}

fn render_albums(
    frame: &mut Frame<'_>,
    area: Rect,
    albums: &[Album],
    selection: usize,
    show_added_date: bool,
    theme: Theme,
) {
    let range = visible_item_range(albums.len(), selection, area.height, 1);
    let items = albums
        .iter()
        .skip(range.start)
        .take(range.len())
        .map(|album| {
            let metadata = if show_added_date {
                format!(
                    "{}  {}  added {}",
                    album.artist, album.year, album.added_date
                )
            } else {
                format!(
                    "{}  {}  {} tracks",
                    album.artist,
                    album.year,
                    album.tracks.len()
                )
            };
            ListItem::new(Line::from(vec![
                Span::styled(
                    truncate_cell(&album.title, album_title_width(area.width)),
                    Style::default()
                        .fg(theme.foreground)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!("  {metadata}"), Style::default().fg(theme.muted)),
            ]))
        })
        .collect::<Vec<_>>();
    render_selectable_list(
        frame,
        area,
        items,
        selection.saturating_sub(range.start),
        theme,
    );
}

fn render_selectable_list(
    frame: &mut Frame<'_>,
    area: Rect,
    items: Vec<ListItem<'_>>,
    selection: usize,
    theme: Theme,
) {
    let list = List::new(items).highlight_style(
        Style::default()
            .bg(theme.selection)
            .add_modifier(Modifier::BOLD),
    );
    let mut list_state = ListState::default().with_selected(Some(selection));
    frame.render_stateful_widget(list, area, &mut list_state);
}

fn visible_item_range(
    length: usize,
    selection: usize,
    area_height: u16,
    item_height: u16,
) -> Range<usize> {
    if length == 0 {
        return 0..0;
    }
    let capacity = usize::from((area_height / item_height.max(1)).max(1)).min(length);
    let selected = selection.min(length - 1);
    let start = selected
        .saturating_sub(capacity / 2)
        .min(length.saturating_sub(capacity));
    start..start + capacity
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TrackColumnPlan {
    title: usize,
    artist: Option<usize>,
    album: Option<usize>,
    year: bool,
    plays: bool,
    favorite: bool,
}

const fn track_column_plan(width: u16, mode: TrackListMode) -> TrackColumnPlan {
    match (mode, width) {
        (TrackListMode::Songs, 104..) => TrackColumnPlan {
            title: 25,
            artist: Some(20),
            album: Some(20),
            year: true,
            plays: true,
            favorite: true,
        },
        (TrackListMode::Songs, 86..) => TrackColumnPlan {
            title: 31,
            artist: Some(20),
            album: None,
            year: true,
            plays: false,
            favorite: false,
        },
        (TrackListMode::Songs, 66..) => TrackColumnPlan {
            title: 29,
            artist: Some(18),
            album: None,
            year: false,
            plays: false,
            favorite: false,
        },
        (TrackListMode::Playlist, 98..) => TrackColumnPlan {
            title: 25,
            artist: Some(20),
            album: Some(20),
            year: false,
            plays: false,
            favorite: true,
        },
        (TrackListMode::Detail, 98..) => TrackColumnPlan {
            title: 25,
            artist: Some(20),
            album: Some(20),
            year: false,
            plays: false,
            favorite: true,
        },
        (TrackListMode::Playlist, 74..) | (TrackListMode::Detail, 74..) => TrackColumnPlan {
            title: 31,
            artist: Some(20),
            album: None,
            year: false,
            plays: false,
            favorite: false,
        },
        _ => TrackColumnPlan {
            title: 20,
            artist: None,
            album: None,
            year: false,
            plays: false,
            favorite: false,
        },
    }
}

fn track_list_header(mode: TrackListMode, width: u16) -> String {
    let plan = track_column_plan(width, mode);
    let mut columns = vec![" # ".to_owned(), pad_cell("Title", plan.title)];
    if let Some(width) = plan.artist {
        columns.push(pad_cell("Artist", width));
    }
    if let Some(width) = plan.album {
        columns.push(pad_cell("Album", width));
    }
    columns.push("Time".to_owned());
    if plan.year {
        columns.push("Year".to_owned());
    }
    if plan.plays {
        columns.push("Plays".to_owned());
    }
    columns.join("  ")
}

fn compact_track_item(
    track: &Track,
    index: usize,
    state: &AppState,
    width: u16,
    mode: TrackListMode,
    theme: Theme,
) -> ListItem<'static> {
    let plan = track_column_plan(width, mode);
    let playing = state
        .playback
        .current_track
        .as_ref()
        .is_some_and(|current| current.id == track.id);
    let mut spans = vec![
        Span::styled(
            format!("{}{:>2}", if playing { "▶" } else { " " }, index + 1),
            Style::default().fg(if playing { theme.playing } else { theme.muted }),
        ),
        Span::raw("  "),
        Span::styled(
            pad_cell(&track.title, plan.title),
            Style::default().fg(theme.foreground),
        ),
    ];
    if let Some(column_width) = plan.artist {
        spans.push(Span::styled("  ", Style::default()));
        spans.push(Span::styled(
            pad_cell(&track.artist, column_width),
            Style::default().fg(theme.muted),
        ));
    }
    if let Some(column_width) = plan.album {
        spans.push(Span::styled("  ", Style::default()));
        spans.push(Span::styled(
            pad_cell(&track.album, column_width),
            Style::default().fg(theme.muted),
        ));
    }
    spans.push(Span::styled(
        format!("  {:>5}", format_time(track.duration.as_secs())),
        Style::default().fg(theme.muted),
    ));
    if plan.year {
        let year = track
            .metadata
            .year
            .map_or_else(String::new, |year| year.to_string());
        spans.push(Span::styled(
            format!("  {year:>4}"),
            Style::default().fg(theme.muted),
        ));
    }
    if plan.plays {
        let plays = track
            .metadata
            .play_count
            .map_or_else(String::new, |plays| plays.to_string());
        spans.push(Span::styled(
            format!("  {plays:>5}"),
            Style::default().fg(theme.muted),
        ));
    }
    if plan.favorite && track.is_favorite {
        spans.push(Span::styled("  ♥", Style::default().fg(theme.accent)));
    }
    ListItem::new(Line::from(spans))
}

fn truncate_cell(value: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let count = value.chars().count();
    if count <= width {
        return value.to_owned();
    }
    if width == 1 {
        return "…".to_owned();
    }
    let prefix = value.chars().take(width - 1).collect::<String>();
    format!("{prefix}…")
}

fn pad_cell(value: &str, width: usize) -> String {
    format!("{:<width$}", truncate_cell(value, width))
}

const fn album_title_width(width: u16) -> usize {
    if width >= 94 {
        34
    } else if width >= 72 {
        26
    } else {
        18
    }
}

const fn artist_name_width(width: u16) -> usize {
    if width >= 94 {
        42
    } else if width >= 72 {
        30
    } else {
        20
    }
}

const fn recently_played_title_width(width: u16) -> usize {
    if width >= 94 {
        32
    } else if width >= 72 {
        24
    } else {
        16
    }
}

fn playlist_name_width(width: u16, depth: usize) -> usize {
    let reserved = 14usize.saturating_add(depth.saturating_mul(2));
    usize::from(width).saturating_sub(reserved).clamp(10, 42)
}

fn render_artist_detail(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &AppState,
    artist: &Artist,
    theme: Theme,
) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(area);
    let album_names = artist
        .album_ids
        .iter()
        .filter_map(|album_id| {
            state
                .albums
                .iter()
                .find(|album| album.id == *album_id)
                .map(|album| album.title.as_str())
        })
        .collect::<Vec<_>>()
        .join(" • ");
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(
                artist.name.as_str(),
                Style::default()
                    .fg(theme.foreground)
                    .add_modifier(Modifier::BOLD),
            ),
            Line::styled(
                format!("Albums: {album_names}"),
                Style::default().fg(theme.muted),
            ),
            Line::styled(
                format!("{} top tracks  •  Esc/h back", artist.top_track_ids.len()),
                Style::default().fg(theme.accent),
            ),
        ]),
        rows[0],
    );

    let tracks = artist
        .top_track_ids
        .iter()
        .filter_map(|track_id| state.library.iter().find(|track| track.id == *track_id))
        .collect::<Vec<_>>();
    render_track_list(
        frame,
        rows[1],
        state,
        &tracks,
        state.content_selection,
        TrackListMode::Detail,
        theme,
    );
}

fn render_album_detail(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &AppState,
    album: &Album,
    theme: Theme,
) -> Option<artwork::ArtworkLayout> {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(9), Constraint::Min(1)])
        .split(area);
    let genres = album
        .tracks
        .iter()
        .filter_map(|track| track.metadata.genre.as_deref())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join(" • ");
    let year = if album.year == 0 {
        "Year unavailable".to_owned()
    } else {
        album.year.to_string()
    };
    let mut metadata = vec![
        Line::styled(
            album.title.as_str(),
            Style::default()
                .fg(theme.foreground)
                .add_modifier(Modifier::BOLD),
        ),
        Line::styled(
            format!("{} • {year}", album.artist),
            Style::default().fg(theme.muted),
        ),
        Line::styled(
            if genres.is_empty() {
                "Genre unavailable".to_owned()
            } else {
                format!("Genre: {genres}")
            },
            Style::default().fg(theme.muted),
        ),
        Line::styled(
            format!(
                "{} tracks  •  P play album  •  Esc/h back",
                album.tracks.len()
            ),
            Style::default().fg(theme.accent),
        ),
    ];
    if rows[0].width >= 50 {
        let header = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(24), Constraint::Length(18)])
            .split(rows[0]);
        frame.render_widget(Paragraph::new(metadata), header[0]);
        let artwork = render_artwork_panel(
            frame,
            header[1],
            state,
            ArtworkKey::Album(album.id.clone()),
            "Artwork",
            theme,
        );
        let tracks = album.tracks.iter().collect::<Vec<_>>();
        render_track_list(
            frame,
            rows[1],
            state,
            &tracks,
            state.content_selection,
            TrackListMode::Detail,
            theme,
        );
        return artwork;
    } else {
        metadata.insert(
            3,
            Line::styled(
                album_artwork_status(state, album),
                Style::default().fg(theme.muted),
            ),
        );
        frame.render_widget(Paragraph::new(metadata), rows[0]);
    }

    let tracks = album.tracks.iter().collect::<Vec<_>>();
    render_track_list(
        frame,
        rows[1],
        state,
        &tracks,
        state.content_selection,
        TrackListMode::Detail,
        theme,
    );
    None
}

fn album_artwork_status(state: &AppState, album: &Album) -> String {
    let protocol = artwork::detected_protocol();
    match state
        .artwork_cache
        .get(&ArtworkKey::Album(album.id.clone()))
    {
        Some(crate::app::state::ArtworkCacheEntry::Loading) => {
            "Artwork: loading lazily…".to_owned()
        }
        Some(crate::app::state::ArtworkCacheEntry::Ready(artwork_data)) => {
            let presentation = if protocol == artwork::TerminalArtworkProtocol::Kitty
                && matches!(
                    state
                        .renderable_artwork_cache
                        .get(&ArtworkKey::Album(album.id.clone())),
                    Some(crate::app::state::RenderableArtworkCacheEntry::Ready { .. })
                ) {
                "Kitty inline • JPEG → PNG cached".to_owned()
            } else if artwork::can_render_inline(protocol, artwork_data) {
                format!("{} inline", protocol.label())
            } else {
                "Unicode placeholder ▣".to_owned()
            };
            format!(
                "Artwork: {} KiB {} cached • {presentation}",
                artwork_data.bytes.len().div_ceil(1024),
                artwork::media_type_label(artwork_data.media_type),
            )
        }
        Some(crate::app::state::ArtworkCacheEntry::Transient(message)) => {
            format!(
                "Artwork: waiting for fresh Music.app object ({message}) • Unicode placeholder ▣"
            )
        }
        Some(crate::app::state::ArtworkCacheEntry::Unavailable(message)) => {
            format!("Artwork: {message} • Unicode placeholder ▣")
        }
        None => "Artwork: not requested • Unicode placeholder ▣".to_owned(),
    }
}

fn render_artwork_panel(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &AppState,
    key: ArtworkKey,
    title: &str,
    theme: Theme,
) -> Option<artwork::ArtworkLayout> {
    let protocol = artwork::detected_protocol();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.muted))
        .title(format!(" {title} "));
    let inner = block.inner(area);
    if artwork::inline_artwork_is_ready(state, &key) && inner.width > 0 && inner.height > 0 {
        frame.render_widget(block.style(Style::default().bg(theme.background)), area);
        Some(artwork::ArtworkLayout::new(key, inner))
    } else {
        frame.render_widget(
            Paragraph::new(artwork_panel_text(state, &key, protocol))
                .block(block)
                .style(Style::default().fg(theme.muted))
                .wrap(Wrap { trim: true }),
            area,
        );
        None
    }
}

fn artwork_panel_text(
    state: &AppState,
    key: &ArtworkKey,
    protocol: artwork::TerminalArtworkProtocol,
) -> String {
    if artwork::inline_artwork_is_ready_for_protocol(state, key, protocol) {
        return String::new();
    }
    match state.artwork_cache.get(key) {
        Some(crate::app::state::ArtworkCacheEntry::Loading) => {
            "▣\nLoading Music.app…\nRenderer: pending".to_owned()
        }
        Some(crate::app::state::ArtworkCacheEntry::Transient(message)) => {
            format!("▣\nWaiting for Music.app…\n{message}")
        }
        Some(crate::app::state::ArtworkCacheEntry::Ready(artwork_data)) => {
            let size = artwork_data.bytes.len().div_ceil(1024);
            let kitty_renderable = state.renderable_artwork_cache.get(key);
            if protocol == artwork::TerminalArtworkProtocol::Kitty
                && matches!(
                    kitty_renderable,
                    Some(crate::app::state::RenderableArtworkCacheEntry::Ready { .. })
                )
            {
                format!(
                    "Artwork loaded\n{} → PNG • {size} KiB\nRenderer: Kitty (cached)",
                    artwork::media_type_label(artwork_data.media_type),
                )
            } else if protocol == artwork::TerminalArtworkProtocol::Kitty
                && matches!(
                    kitty_renderable,
                    Some(crate::app::state::RenderableArtworkCacheEntry::Loading { .. })
                )
            {
                format!(
                    "▣  Artwork\n{} • {size} KiB\nPreparing PNG for Kitty…",
                    artwork::media_type_label(artwork_data.media_type),
                )
            } else if protocol == artwork::TerminalArtworkProtocol::Kitty
                && let Some(crate::app::state::RenderableArtworkCacheEntry::Unavailable {
                    message,
                    ..
                }) = kitty_renderable
            {
                format!("▣  Artwork\n{message}\nUnicode fallback")
            } else if artwork::can_render_inline(protocol, artwork_data) {
                format!(
                    "Artwork loaded\n{} • {size} KiB\nRenderer: {}",
                    artwork::media_type_label(artwork_data.media_type),
                    protocol.label()
                )
            } else {
                format!(
                    "▣  Artwork\n{} • {size} KiB\nRenderer: {}",
                    artwork::media_type_label(artwork_data.media_type),
                    protocol.label()
                )
            }
        }
        Some(crate::app::state::ArtworkCacheEntry::Unavailable(reason)) => {
            if reason == "No local Music.app artwork" {
                format!(
                    "▣  No artwork\nMusic.app has none\nRenderer: {}",
                    protocol.label()
                )
            } else {
                format!(
                    "▣  Artwork unavailable\n{reason}\nRenderer: {}",
                    protocol.label()
                )
            }
        }
        None => "▣  Artwork\nNot requested yet\nWaiting for source…".to_owned(),
    }
}

fn render_missing_detail(frame: &mut Frame<'_>, area: Rect, detail_type: &str, theme: Theme) {
    frame.render_widget(
        Paragraph::new(format!("Selected {detail_type} is no longer available"))
            .style(Style::default().fg(theme.error)),
        area,
    );
}

fn render_playlists(frame: &mut Frame<'_>, area: Rect, state: &AppState, theme: Theme) {
    if state.playlists.is_empty() {
        let message = match state.playlist_status {
            CollectionLoadState::NotStarted
            | CollectionLoadState::Loading { .. }
            | CollectionLoadState::Refreshing { .. } => "Loading Music.app playlists…",
            CollectionLoadState::Cached { .. } => "No cached playlists",
            CollectionLoadState::Loaded { .. } => "No playlists",
            CollectionLoadState::Error(_) => "Could not load playlists",
        };
        frame.render_widget(
            Paragraph::new(message)
                .style(Style::default().fg(theme.muted))
                .alignment(Alignment::Center),
            area,
        );
        return;
    }

    let visible_entries = if matches!(state.navigation.active, Route::Section(Screen::Playlists)) {
        state.visible_playlist_entries()
    } else {
        state
            .playlists
            .iter()
            .map(|playlist| crate::domain::VisiblePlaylistEntry {
                playlist_id: playlist.id.clone(),
                depth: 0,
                has_children: false,
            })
            .collect()
    };
    let range = visible_item_range(
        visible_entries.len(),
        state.content_selection,
        area.height,
        1,
    );
    let items = visible_entries
        .iter()
        .skip(range.start)
        .take(range.len())
        .filter_map(|entry| {
            let playlist = state
                .playlists
                .iter()
                .find(|playlist| playlist.id == entry.playlist_id)?;
            let source = match playlist.origin {
                DataOrigin::Demo => "MOCK",
                DataOrigin::LocalMusicApp => match playlist.kind {
                    PlaylistKind::Smart => "SMART",
                    PlaylistKind::Folder => "FOLDER",
                    PlaylistKind::Subscription => "APPLE",
                    PlaylistKind::User => "LOCAL",
                    PlaylistKind::Library => "LIBRARY",
                    PlaylistKind::Unknown => "MUSIC",
                },
            };
            let count = playlist.track_count.max(playlist.tracks.len());
            let indent = "  ".repeat(entry.depth);
            let disclosure = if playlist.kind == PlaylistKind::Folder {
                if state.expanded_playlist_folders.contains(&playlist.id) {
                    "▼ "
                } else {
                    "▶ "
                }
            } else if entry.has_children {
                "◆ "
            } else {
                "  "
            };
            let suffix = if playlist.kind == PlaylistKind::Folder {
                "folder".to_owned()
            } else if count == 0 {
                String::new()
            } else {
                count.to_string()
            };
            let name_width = playlist_name_width(area.width, entry.depth);
            Some(ListItem::new(Line::from(vec![
                Span::styled(indent, Style::default().fg(theme.muted)),
                Span::styled(disclosure, Style::default().fg(theme.accent)),
                Span::styled(format!("{source:<6}"), Style::default().fg(theme.accent)),
                Span::styled(
                    truncate_cell(&playlist.name, name_width),
                    Style::default()
                        .fg(theme.foreground)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!("  {suffix}"), Style::default().fg(theme.muted)),
            ])))
        })
        .collect::<Vec<_>>();
    render_selectable_list(
        frame,
        area,
        items,
        state.content_selection.saturating_sub(range.start),
        theme,
    );
}

fn render_playlist_detail(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &AppState,
    playlist: &crate::domain::Playlist,
    theme: Theme,
) {
    let description = playlist
        .description
        .as_deref()
        .filter(|value| !value.trim().is_empty());
    let header_height = if description.is_some() { 3 } else { 2 };
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(header_height), Constraint::Min(1)])
        .split(area);
    let status = playlist_load_status(&playlist.contents_state);
    let mut header = vec![Line::styled(
        playlist.name.as_str(),
        Style::default()
            .fg(theme.foreground)
            .add_modifier(Modifier::BOLD),
    )];
    if let Some(description) = description {
        header.push(Line::styled(description, Style::default().fg(theme.muted)));
    }
    header.push(Line::styled(
        format!("{status}  •  P play playlist  •  Esc/h back"),
        Style::default().fg(theme.accent),
    ));
    frame.render_widget(Paragraph::new(header), rows[0]);

    if !playlist.tracks.is_empty() {
        let tracks = playlist.tracks.iter().collect::<Vec<_>>();
        render_track_list(
            frame,
            rows[1],
            state,
            &tracks,
            state.content_selection,
            TrackListMode::Playlist,
            theme,
        );
    } else {
        let message = match &playlist.contents_state {
            PlaylistLoadState::NotLoaded | PlaylistLoadState::Loading { total: None, .. } => {
                "Loading playlist…".to_owned()
            }
            PlaylistLoadState::Loading {
                loaded,
                total: Some(total),
            }
            | PlaylistLoadState::PartiallyLoaded { loaded, total } => {
                format!("Loading tracks… {loaded} / {total}")
            }
            PlaylistLoadState::Loaded { total: 0 } | PlaylistLoadState::Empty => {
                "Empty playlist".to_owned()
            }
            PlaylistLoadState::Loaded { total } => format!("{total} tracks"),
            PlaylistLoadState::Error(reason) => {
                format!("Failed to load playlist\n{reason}")
            }
        };
        frame.render_widget(
            Paragraph::new(message)
                .style(Style::default().fg(theme.muted))
                .alignment(Alignment::Center),
            rows[1],
        );
    }
}

fn playlist_load_status(state: &PlaylistLoadState) -> String {
    match state {
        PlaylistLoadState::NotLoaded => "Not loaded".to_owned(),
        PlaylistLoadState::Loading {
            loaded,
            total: None,
        } => {
            if *loaded == 0 {
                "Loading playlist…".to_owned()
            } else {
                format!("Loading tracks… {loaded}")
            }
        }
        PlaylistLoadState::Loading {
            loaded,
            total: Some(total),
        }
        | PlaylistLoadState::PartiallyLoaded { loaded, total } => {
            format!("Loading tracks… {loaded} / {total}")
        }
        PlaylistLoadState::Loaded { total } => format!("{total} tracks"),
        PlaylistLoadState::Empty => "Empty playlist".to_owned(),
        PlaylistLoadState::Error(_) => "Failed to load playlist".to_owned(),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TrackListMode {
    Songs,
    Playlist,
    Detail,
}

fn render_track_list(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &AppState,
    tracks: &[&Track],
    selection: usize,
    mode: TrackListMode,
    theme: Theme,
) {
    if tracks.is_empty() {
        frame.render_widget(
            Paragraph::new("No tracks")
                .style(Style::default().fg(theme.muted))
                .alignment(Alignment::Center),
            area,
        );
        return;
    }

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(area);
    frame.render_widget(
        Paragraph::new(track_list_header(mode, area.width)).style(Style::default().fg(theme.muted)),
        rows[0],
    );
    let range = visible_item_range(tracks.len(), selection, rows[1].height, 1);
    let items = tracks
        .iter()
        .enumerate()
        .skip(range.start)
        .take(range.len())
        .map(|(index, track)| compact_track_item(track, index, state, area.width, mode, theme))
        .collect::<Vec<_>>();
    let list = List::new(items).highlight_style(
        Style::default()
            .bg(theme.selection)
            .add_modifier(Modifier::BOLD),
    );
    let mut list_state =
        ListState::default().with_selected(Some(selection.saturating_sub(range.start)));
    frame.render_stateful_widget(list, rows[1], &mut list_state);
}

fn render_backend_status(state: &AppState, theme: Theme) -> Paragraph<'_> {
    let (text, color) = match &state.backend_status {
        BackendStatus::Initializing => ("Connecting to backend…".to_owned(), theme.warning),
        BackendStatus::Ready { name } => match &state.backend_availability {
            BackendAvailability::Available => {
                (format!("Backend: {name} • Connected"), theme.playing)
            }
            BackendAvailability::NotRunning => (
                format!("Backend: {name} • Music.app is not running (o to open)"),
                theme.warning,
            ),
            BackendAvailability::Unavailable => (
                format!("Backend: {name} • Music.app is unavailable"),
                theme.error,
            ),
            BackendAvailability::PermissionDenied => (
                format!("Backend: {name} • Permission to control Music.app was denied"),
                theme.error,
            ),
            BackendAvailability::Error(message) => (
                format!("Backend: {name} • Synchronization failed: {message}"),
                theme.error,
            ),
        },
        BackendStatus::Error { message } => (format!("Backend error: {message}"), theme.error),
    };
    let notification = state
        .notification
        .as_ref()
        .map_or(String::new(), |message| format!("  •  {message}"));
    let library = match &state.library_status {
        CollectionLoadState::NotStarted => "Library: waiting".to_owned(),
        CollectionLoadState::Cached { total } => format!("Library: Cached ({total} items)"),
        CollectionLoadState::Refreshing { total, .. } if *total == 0 => {
            "Library: Refreshing…".to_owned()
        }
        CollectionLoadState::Refreshing { loaded, total } => {
            format!("Library: Cached · Refreshing {loaded}/{total}")
        }
        CollectionLoadState::Loading { loaded, total } if *total == 0 => {
            "Library: discovering…".to_owned()
        }
        CollectionLoadState::Loading { loaded, total } => {
            format!("Library: {loaded}/{total}")
        }
        CollectionLoadState::Loaded { total } => format!("Library: Ready ({total} items)"),
        CollectionLoadState::Error(_) if !state.library.is_empty() => {
            "Library: Refresh failed · cached data shown".to_owned()
        }
        CollectionLoadState::Error(_) => "Library: unavailable".to_owned(),
    };
    let apple_api = state.auth_status.label();
    let help_hint = if state.terminal_size.0 >= 90 {
        "  •  ? Help"
    } else {
        ""
    };
    Paragraph::new(vec![
        Line::styled(format!("{text}{notification}"), Style::default().fg(color)),
        Line::styled(
            format!("{library}  •  Apple API: {apple_api} (optional){help_hint}"),
            Style::default().fg(theme.muted),
        ),
    ])
}

fn render_queue(frame: &mut Frame<'_>, area: Rect, state: &AppState, theme: Theme) {
    let current_id = state.playback.current_entry_id.as_ref();
    let items = state
        .queue
        .iter()
        .map(|item| {
            let marker = if Some(&item.id) == current_id {
                "▶ "
            } else {
                "  "
            };
            ListItem::new(Line::from(vec![
                Span::styled(marker, Style::default().fg(theme.playing)),
                Span::raw(&item.track.title),
                Span::styled(
                    format!("\n  {}", item.track.artist),
                    Style::default().fg(theme.muted),
                ),
            ]))
        })
        .collect::<Vec<_>>();
    let list = List::new(items)
        .block(panel_block(" Up Next ", state.focus == Focus::Queue, theme))
        .highlight_style(Style::default().bg(theme.selection));
    let mut list_state = ListState::default().with_selected(Some(state.queue_selection));
    frame.render_stateful_widget(list, area, &mut list_state);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PlayerLayout {
    artwork_width: Option<u16>,
    show_album: bool,
    show_context: bool,
    show_toggles: bool,
}

const fn player_layout(width: u16, height: u16) -> PlayerLayout {
    if width >= 108 && height >= 7 {
        PlayerLayout {
            artwork_width: Some(18),
            show_album: true,
            show_context: true,
            show_toggles: true,
        }
    } else if width >= 96 && height >= 5 {
        PlayerLayout {
            artwork_width: Some(14),
            show_album: true,
            show_context: true,
            show_toggles: true,
        }
    } else if width >= 80 && height >= 5 {
        PlayerLayout {
            artwork_width: Some(14),
            show_album: false,
            show_context: true,
            show_toggles: true,
        }
    } else {
        PlayerLayout {
            artwork_width: None,
            show_album: false,
            show_context: width >= 64,
            show_toggles: width >= 58,
        }
    }
}

fn player_title_line(track: Option<&Track>, control_icon: &str, width: u16) -> String {
    let Some(track) = track else {
        return format!("{control_icon}  Nothing playing");
    };
    let available = usize::from(width).saturating_sub(4);
    if available < 18 {
        return format!("{control_icon}  {}", truncate_cell(&track.title, available));
    }
    let title_width = available.saturating_mul(3) / 5;
    let artist_width = available.saturating_sub(title_width + 3);
    format!(
        "{control_icon}  {} — {}",
        truncate_cell(&track.title, title_width),
        truncate_cell(&track.artist, artist_width)
    )
}

fn player_status_line(state: &AppState, layout: PlayerLayout, width: u16) -> String {
    let Some(track) = state.playback.current_track.as_ref() else {
        return truncate_cell("Music.app idle", usize::from(width));
    };
    let mut fields = Vec::with_capacity(5);
    if layout.show_album {
        fields.push(format!("Album: {}", track.album));
    }
    if layout.show_context {
        let context = match &state.playback.context {
            PlaybackContext::NoContext => None,
            PlaybackContext::Playlist {
                playlist_id,
                ordered_track_ids,
                current_index,
                complete,
            } => {
                let name = state
                    .playlists
                    .iter()
                    .find(|playlist| playlist.id == *playlist_id)
                    .map_or("Playlist", |playlist| playlist.name.as_str());
                let suffix = if *complete { "" } else { "+" };
                Some(format!(
                    "{name} {}/{}{suffix}",
                    current_index + 1,
                    ordered_track_ids.len()
                ))
            }
            PlaybackContext::Album {
                ordered_track_ids,
                current_index,
                ..
            } => Some(format!(
                "Album {}/{}",
                current_index + 1,
                ordered_track_ids.len()
            )),
        };
        if let Some(context) = context {
            fields.push(context);
        }
    }
    fields.push(format!(
        "vol {}%{}",
        state.playback.volume,
        if state.playback.muted { " muted" } else { "" }
    ));
    if layout.show_toggles {
        fields.push(format!(
            "shuffle {}",
            if state.playback.shuffle { "on" } else { "off" }
        ));
        let repeat = match state.playback.repeat {
            RepeatMode::Off => "off",
            RepeatMode::All => "all",
            RepeatMode::One => "one",
        };
        fields.push(format!("repeat {repeat}"));
    }
    if track.is_favorite && layout.show_toggles {
        fields.push("♥".to_owned());
    }
    truncate_cell(&fields.join("  •  "), usize::from(width))
}

fn render_now_playing_artwork_panel(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &AppState,
    key: ArtworkKey,
    theme: Theme,
) -> Option<artwork::ArtworkLayout> {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.muted))
        .title(" Now Playing ");
    let inner = block.inner(area);
    if artwork::inline_artwork_is_ready(state, &key) && inner.width > 0 && inner.height > 0 {
        frame.render_widget(block.style(Style::default().bg(theme.background)), area);
        return Some(artwork::ArtworkLayout::new(key, inner));
    }
    let message = now_playing_artwork_text(state, &key);
    frame.render_widget(
        Paragraph::new(message)
            .block(block)
            .style(Style::default().fg(theme.muted))
            .alignment(Alignment::Center),
        area,
    );
    None
}

fn now_playing_artwork_text(state: &AppState, key: &ArtworkKey) -> &'static str {
    match state.artwork_cache.get(key) {
        Some(crate::app::state::ArtworkCacheEntry::Loading)
        | Some(crate::app::state::ArtworkCacheEntry::Transient(_)) => "▣\nArtwork loading",
        Some(crate::app::state::ArtworkCacheEntry::Ready(_))
        | Some(crate::app::state::ArtworkCacheEntry::Unavailable(_))
        | None => "▣\nArtwork unavailable",
    }
}

fn render_player(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &AppState,
    theme: Theme,
) -> Option<artwork::ArtworkLayout> {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .style(Style::default().bg(theme.background));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let track = state.playback.current_track.as_ref();
    let layout = player_layout(inner.width, inner.height);
    let (artwork_area, metadata_area) = if let Some(artwork_width) = layout.artwork_width {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(artwork_width), Constraint::Min(1)])
            .split(inner);
        (Some(columns[0]), columns[1])
    } else {
        (None, inner)
    };
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(metadata_area);
    let control_icon = playback_control_icon(state.playback.status);
    frame.render_widget(
        Paragraph::new(player_title_line(track, control_icon, metadata_area.width))
            .style(Style::default().fg(theme.foreground)),
        rows[0],
    );
    frame.render_widget(
        Paragraph::new(player_status_line(state, layout, metadata_area.width))
            .style(Style::default().fg(theme.muted)),
        rows[1],
    );

    let duration = track.map_or(0, |track| track.duration.as_secs());
    let position = state.playback.position.as_secs().min(duration);
    let ratio = if duration == 0 {
        0.0
    } else {
        position as f64 / duration as f64
    };
    let label = format!("{} / {}", format_time(position), format_time(duration));
    frame.render_widget(
        Gauge::default()
            .gauge_style(Style::default().fg(theme.accent).bg(theme.selection))
            .ratio(ratio)
            .label(label),
        rows[3],
    );
    if let (Some(artwork_area), Some(track)) = (artwork_area, track) {
        return render_now_playing_artwork_panel(
            frame,
            artwork_area,
            state,
            state.artwork_key_for_track(&track.id),
            theme,
        );
    }
    None
}

const fn playback_control_icon(status: PlaybackStatus) -> &'static str {
    match status {
        PlaybackStatus::Playing => "⏸",
        PlaybackStatus::Paused | PlaybackStatus::Stopped => "▶",
    }
}

fn render_help(frame: &mut Frame<'_>, area: Rect, state: &AppState, theme: Theme) {
    let popup = centered_rect(area, 86, 82);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(" Help • Esc / ? / q close ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .style(Style::default().bg(theme.background));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    if help_column_count(inner.width) == 2 {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(inner);
        frame.render_widget(
            Paragraph::new(visible_help_lines(
                &help_lines(&[BindingGroup::Navigation, BindingGroup::General], theme),
                state.help_scroll,
                columns[0].height,
            ))
            .wrap(Wrap { trim: false }),
            columns[0],
        );
        frame.render_widget(
            Paragraph::new(visible_help_lines(
                &help_lines(&[BindingGroup::Playback], theme)
                    .into_iter()
                    .chain(context_help_lines(state, theme))
                    .collect::<Vec<_>>(),
                state.help_scroll,
                columns[1].height,
            ))
            .wrap(Wrap { trim: false }),
            columns[1],
        );
    } else {
        let lines = help_lines(
            &[
                BindingGroup::Navigation,
                BindingGroup::Playback,
                BindingGroup::General,
            ],
            theme,
        )
        .into_iter()
        .chain(context_help_lines(state, theme))
        .collect::<Vec<_>>();
        frame.render_widget(
            Paragraph::new(visible_help_lines(&lines, state.help_scroll, inner.height))
                .wrap(Wrap { trim: false }),
            inner,
        );
    }
}

fn render_action_menu(frame: &mut Frame<'_>, area: Rect, state: &AppState, theme: Theme) {
    let Some(menu) = state.action_menu.as_ref() else {
        return;
    };
    let width = area.width.clamp(1, 42);
    let height = area
        .height
        .min((menu.actions.len() as u16).saturating_add(2))
        .max(1);
    let popup = centered_popup(area, width, height);
    frame.render_widget(Clear, popup);
    let title = format!(" Actions — {} ", context_target_label(&menu.target));
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .style(Style::default().bg(theme.background));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let items = menu
        .actions
        .iter()
        .map(|action| {
            ListItem::new(Line::styled(
                context_action_label(*action),
                Style::default().fg(theme.foreground),
            ))
        })
        .collect::<Vec<_>>();
    let list = List::new(items).highlight_style(
        Style::default()
            .bg(theme.selection)
            .add_modifier(Modifier::BOLD),
    );
    let mut list_state = ListState::default().with_selected(Some(menu.selection));
    frame.render_stateful_widget(list, inner, &mut list_state);
}

fn sort_fields_for_ui(collection: CollectionKind) -> &'static [crate::app::state::CollectionSort] {
    use crate::app::state::CollectionSort;
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

fn render_sort_menu(frame: &mut Frame<'_>, area: Rect, state: &AppState, theme: Theme) {
    let Some(menu) = state.sort_menu.as_ref() else {
        return;
    };
    let view = match menu.collection {
        CollectionKind::Songs => &state.library_views.songs,
        CollectionKind::Albums => &state.library_views.albums,
        CollectionKind::Artists => &state.library_views.artists,
    };
    let fields = sort_fields_for_ui(menu.collection);
    let popup = centered_popup(
        area,
        area.width.clamp(1, 38),
        area.height.min(fields.len() as u16 + 2),
    );
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(format!(" Sort {} ", menu.collection.label()))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .style(Style::default().bg(theme.background));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let arrow = if view.descending { "↓" } else { "↑" };
    let items = fields
        .iter()
        .map(|field| {
            let marker = if *field == view.sort { "●" } else { " " };
            ListItem::new(Line::styled(
                format!("{marker} {}  {arrow}", field.label()),
                Style::default().fg(theme.foreground),
            ))
        })
        .collect::<Vec<_>>();
    let mut list_state = ListState::default().with_selected(Some(menu.selection));
    frame.render_stateful_widget(
        List::new(items).highlight_style(
            Style::default()
                .bg(theme.selection)
                .add_modifier(Modifier::BOLD),
        ),
        inner,
        &mut list_state,
    );
}

fn render_filter_editor(frame: &mut Frame<'_>, area: Rect, state: &AppState, theme: Theme) {
    let Some(editor) = state.filter_editor.as_ref() else {
        return;
    };
    let popup = centered_popup(area, area.width.clamp(1, 52), area.height.min(4));
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(format!(" Filter {} ", editor.collection.label()))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .style(Style::default().bg(theme.background));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    frame.render_widget(
        Paragraph::new(format!(
            "{}▌\nEnter apply · Esc cancel · Ctrl-l clear",
            editor.draft
        ))
        .style(Style::default().fg(theme.foreground)),
        inner,
    );
}

fn render_playlist_track_removal_confirmation(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &AppState,
    theme: Theme,
) {
    let Some(confirmation) = state.playlist_track_removal_confirmation.as_ref() else {
        return;
    };
    let popup = centered_popup(area, area.width.clamp(1, 58), area.height.min(7));
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(" Remove from playlist? ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .style(Style::default().bg(theme.background));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    frame.render_widget(
        Paragraph::new(format!(
            "{}\nfrom \"{}\"\n\n[d/Enter] Remove  [Esc/q] Cancel",
            confirmation.track_title, confirmation.playlist_name
        ))
        .style(Style::default().fg(theme.foreground)),
        inner,
    );
}

const fn context_target_label(target: &crate::app::state::ContextTarget) -> &'static str {
    match target {
        crate::app::state::ContextTarget::Track(_)
        | crate::app::state::ContextTarget::PlaylistTrack { .. } => "Track",
        crate::app::state::ContextTarget::Album(_) => "Album",
        crate::app::state::ContextTarget::Artist(_) => "Artist",
        crate::app::state::ContextTarget::Playlist(_) => "Playlist",
        crate::app::state::ContextTarget::Folder(_) => "Folder",
    }
}

const fn context_action_label(action: ContextAction) -> &'static str {
    match action {
        ContextAction::PlayTrack => "▶  Play",
        ContextAction::OpenAlbum => "   Open Album",
        ContextAction::OpenArtist => "   Open Artist",
        ContextAction::PlayAlbum => "▶  Play Album  P",
        ContextAction::OpenPlaylist => "   Open Playlist",
        ContextAction::PlayPlaylist => "▶  Play Playlist  P",
        ContextAction::ExpandFolder => "   Expand Folder",
        ContextAction::CollapseFolder => "   Collapse Folder",
        ContextAction::RemoveFromPlaylist => "×  Remove from Playlist  d",
    }
}

const fn help_column_count(width: u16) -> u8 {
    if width >= 72 { 2 } else { 1 }
}

fn help_lines(groups: &[BindingGroup], theme: Theme) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for (group_index, group) in groups.iter().enumerate() {
        if group_index > 0 {
            lines.push(Line::default());
        }
        let title = match group {
            BindingGroup::Navigation => "Navigation",
            BindingGroup::Playback => "Playback",
            BindingGroup::General => "General",
        };
        lines.push(Line::styled(
            title,
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ));
        lines.extend(
            bindings()
                .iter()
                .filter(|binding| {
                    binding.group == *group
                        && !matches!(binding.keys, "P" | "R" | "S" | "F" | "Ctrl-l")
                })
                .map(|binding| {
                    Line::from(vec![
                        Span::styled(
                            format!("  {:<12}", binding.keys),
                            Style::default().fg(theme.foreground),
                        ),
                        Span::styled(binding.description, Style::default().fg(theme.muted)),
                    ])
                }),
        );
    }
    lines
}

fn context_help_lines(state: &AppState, theme: Theme) -> Vec<Line<'static>> {
    let (title, entries) = match &state.navigation.active {
        Route::Section(Screen::Songs) => (
            "Current View — Tracks",
            vec![
                ("Enter", "play selected track"),
                ("a", "actions"),
                ("R", "refresh library"),
                ("S", "sort"),
                ("F", "filter"),
                ("Ctrl-l", "clear filter"),
            ],
        ),
        Route::Section(Screen::RecentlyPlayed) => (
            "Current View — Tracks",
            vec![("Enter", "play selected track"), ("a", "actions")],
        ),
        Route::Section(Screen::Albums) => (
            "Current View — Albums",
            vec![
                ("Enter", "open selected album"),
                ("P", "play selected album"),
                ("a", "actions"),
                ("R", "refresh library"),
                ("S", "sort"),
                ("F", "filter"),
                ("Ctrl-l", "clear filter"),
            ],
        ),
        Route::Section(Screen::RecentlyAdded) => (
            "Current View — Albums",
            vec![
                ("Enter", "open selected album"),
                ("P", "play selected album"),
                ("a", "actions"),
            ],
        ),
        Route::Section(Screen::Artists) => (
            "Current View — Artists",
            vec![
                ("Enter", "open selected artist"),
                ("a", "actions"),
                ("R", "refresh library"),
                ("S", "sort"),
                ("F", "filter"),
                ("Ctrl-l", "clear filter"),
            ],
        ),
        Route::Section(Screen::Playlists | Screen::MadeForYou) => {
            let has_folder = state.playlists.iter().any(|playlist| {
                matches!(state.navigation.active, Route::Section(Screen::Playlists))
                    && playlist.kind == PlaylistKind::Folder
            });
            let entry = if has_folder {
                "open playlist / expand folder"
            } else {
                "open selected playlist"
            };
            (
                "Current View — Playlists",
                vec![
                    ("Enter", entry),
                    ("P", "play selected playlist"),
                    ("a", "actions"),
                ],
            )
        }
        Route::PlaylistDetail { playlist_id } => {
            let removable = state
                .playlists
                .iter()
                .find(|playlist| playlist.id == *playlist_id)
                .is_some_and(|playlist| {
                    playlist.kind == PlaylistKind::User
                        && state.capabilities.supports(Capability::PlaylistTrackRemove)
                        && !playlist.tracks.is_empty()
                });
            let mut entries = vec![
                ("Enter", "play selected track"),
                ("P", "play playlist"),
                ("a", "actions"),
            ];
            if removable {
                entries.push(("d", "remove from playlist"));
            }
            ("Current View — Playlist Detail", entries)
        }
        Route::AlbumDetail { .. } => (
            "Current View — Album Detail",
            vec![
                ("Enter", "play selected track"),
                ("P", "play album"),
                ("a", "actions"),
            ],
        ),
        Route::ArtistDetail { .. } => (
            "Current View — Artist Detail",
            vec![("Enter", "play selected track"), ("a", "actions")],
        ),
        Route::NowPlaying => (
            "Current View — Now Playing",
            vec![
                ("Esc / h / q", "return to previous view"),
                ("Space / n / p", "play/pause · next · previous"),
                ("[ / ]", "seek backward / forward"),
                ("- / +", "volume"),
                ("s / r", "shuffle / repeat"),
            ],
        ),
        Route::Section(Screen::Search) if state.search_input_active => (
            "Current View — Search",
            vec![
                ("Type", "edit local query"),
                ("Enter", "finish editing"),
                ("Esc", "leave search editing"),
            ],
        ),
        Route::Section(Screen::Search) => (
            "Current View — Search",
            vec![
                ("j / k", "move results"),
                ("Enter", "open selected result"),
                ("/", "edit query"),
                ("a", "actions"),
            ],
        ),
        _ => ("Current View", Vec::new()),
    };
    if entries.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![
        Line::default(),
        Line::styled(
            title,
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
    ];
    lines.extend(entries.into_iter().map(|(key, description)| {
        Line::from(vec![
            Span::styled(
                format!("  {key:<12}"),
                Style::default().fg(theme.foreground),
            ),
            Span::styled(description, Style::default().fg(theme.muted)),
        ])
    }));
    lines
}

fn visible_help_lines(lines: &[Line<'static>], scroll: usize, height: u16) -> Vec<Line<'static>> {
    let capacity = usize::from(height.max(1));
    let start = scroll.min(lines.len().saturating_sub(capacity));
    lines.iter().skip(start).take(capacity).cloned().collect()
}

fn panel_block<'a>(title: &'a str, focused: bool, theme: Theme) -> Block<'a> {
    let border = if focused { theme.accent } else { theme.border };
    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border))
        .style(Style::default().bg(theme.background))
}

fn centered_rect(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1]);
    horizontal[1]
}

fn centered_popup(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect::new(
        area.x.saturating_add(area.width.saturating_sub(width) / 2),
        area.y
            .saturating_add(area.height.saturating_sub(height) / 2),
        width,
        height,
    )
}

fn format_time(seconds: u64) -> String {
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use ratatui::{Terminal, backend::TestBackend, layout::Rect};

    use crate::{
        app::{
            action::Action,
            reducer::reduce,
            state::{
                ActionMenuState, AppState, ArtworkCacheEntry, BackendStatus, ContextAction,
                ContextTarget, LocalSearchResult, NavigationState, RenderableArtworkCacheEntry,
                Route, Screen, ViewStatus,
            },
        },
        auth::AuthStatus,
        backend::{BackendEvent, MusicBackend, mock::MockMusicBackend},
        domain::{
            Artwork, ArtworkKey, ArtworkMediaType, CollectionLoadState, PlaybackContext,
            PlaybackSnapshot, PlaybackStatus, Playlist, PlaylistId, PlaylistKind,
            PlaylistLoadState, Track, TrackId,
        },
    };

    use super::artwork::TerminalArtworkProtocol;
    use super::{
        BindingGroup, TrackListMode, artwork_panel_text, centered_popup, context_action_label,
        context_help_lines, full_now_playing_layout, help_column_count, help_lines,
        now_playing_artwork_text, playback_control_icon, player_layout, player_status_line,
        player_title_line, track_column_plan, truncate_cell, visible_help_lines,
        visible_item_range,
    };

    #[test]
    fn playback_control_icon_comes_from_authoritative_playback_status() {
        assert_eq!(playback_control_icon(PlaybackStatus::Playing), "⏸");
        assert_eq!(playback_control_icon(PlaybackStatus::Paused), "▶");
        assert_eq!(playback_control_icon(PlaybackStatus::Stopped), "▶");
    }

    #[test]
    fn full_now_playing_is_responsive_and_renders_authoritative_context() {
        let track = Track::new(
            "track",
            "A Very Long Current Song",
            "Current Artist",
            "Current Album",
            Duration::from_secs(240),
        );
        let state = AppState {
            navigation: NavigationState {
                active: Route::NowPlaying,
                history: Vec::new(),
            },
            playback: PlaybackSnapshot {
                current_track: Some(track),
                position: Duration::from_secs(42),
                status: PlaybackStatus::Playing,
                context: PlaybackContext::Album {
                    album_id: crate::domain::AlbumId::new("album"),
                    ordered_track_ids: vec![TrackId::new("one"), TrackId::new("two")],
                    current_index: 1,
                },
                ..Default::default()
            },
            ..AppState::default()
        };
        let wide = render_text_at(&state, 110, 32);
        assert!(wide.contains("A Very Long Current Song"));
        assert!(wide.contains("Album · 2 / 2"));
        assert!(wide.contains("0:42 / 4:00"));
        assert!(!wide.contains("Backend:"));
        let medium = render_text_at(&state, 80, 24);
        assert!(medium.contains("Current Artist"));
        let small = render_text_at(&state, 40, 12);
        assert!(small.contains("Current Song"));
        assert!(!full_now_playing_layout(40, 12).show_artwork);
        assert!(full_now_playing_layout(110, 32).show_artwork);
    }

    #[test]
    fn full_now_playing_renders_the_reconciled_collection_position() {
        let ordered_track_ids = (1..=100)
            .map(|index| TrackId::new(format!("track-{index}")))
            .collect::<Vec<_>>();
        let state = AppState {
            navigation: NavigationState {
                active: Route::NowPlaying,
                history: Vec::new(),
            },
            playback: PlaybackSnapshot {
                current_track: Some(Track::new(
                    "track-37",
                    "Thirty Seven",
                    "Artist",
                    "Album",
                    Duration::from_secs(60),
                )),
                context: PlaybackContext::Playlist {
                    playlist_id: PlaylistId::new("playlist"),
                    ordered_track_ids,
                    current_index: 36,
                    complete: true,
                },
                ..PlaybackSnapshot::default()
            },
            ..AppState::default()
        };

        assert!(render_text_at(&state, 110, 32).contains("Playlist · 37 / 100"));
    }

    #[test]
    fn full_now_playing_idle_and_help_are_clean_and_specific() {
        let state = AppState {
            navigation: NavigationState {
                active: Route::NowPlaying,
                history: Vec::new(),
            },
            ..AppState::default()
        };
        assert!(render_text_at(&state, 80, 24).contains("Nothing Playing"));
        let help = context_help_lines(&state, super::DEFAULT);
        let text = format!("{help:?}");
        assert!(text.contains("return to previous view"));
        assert!(text.contains("shuffle / repeat"));
    }

    #[test]
    fn help_context_is_specific_to_album_playlist_folder_and_search_views() {
        let album_state = AppState {
            navigation: NavigationState {
                active: Route::AlbumDetail {
                    album_id: crate::domain::AlbumId::new("album"),
                },
                history: Vec::new(),
            },
            ..AppState::default()
        };
        let album = context_help_lines(&album_state, super::DEFAULT);
        assert!(format!("{album:?}").contains("play album"));
        assert!(format!("{album:?}").contains("actions"));

        let folder = Playlist::unloaded("folder", "Folder", None, PlaylistKind::Folder, None);
        let playlist_state = AppState {
            navigation: NavigationState {
                active: Route::Section(Screen::Playlists),
                history: Vec::new(),
            },
            playlists: vec![folder],
            ..AppState::default()
        };
        let playlist = context_help_lines(&playlist_state, super::DEFAULT);
        assert!(format!("{playlist:?}").contains("expand folder"));

        let search_state = AppState {
            navigation: NavigationState {
                active: Route::Section(Screen::Search),
                history: Vec::new(),
            },
            search_input_active: true,
            ..AppState::default()
        };
        let search = context_help_lines(&search_state, super::DEFAULT);
        assert!(format!("{search:?}").contains("finish editing"));
    }

    #[test]
    fn action_menu_popup_is_content_sized_and_uses_supported_action_labels() {
        assert_eq!(
            centered_popup(Rect::new(0, 0, 80, 24), 42, 5),
            Rect::new(19, 9, 42, 5)
        );
        assert_eq!(
            centered_popup(Rect::new(0, 0, 20, 3), 42, 5),
            Rect::new(0, 0, 20, 3)
        );
        assert_eq!(context_action_label(ContextAction::PlayTrack), "▶  Play");
        assert_eq!(
            context_action_label(ContextAction::CollapseFolder),
            "   Collapse Folder"
        );

        let state = AppState {
            action_menu: Some(ActionMenuState {
                target: ContextTarget::Album(crate::domain::AlbumId::new("album")),
                actions: vec![ContextAction::OpenAlbum, ContextAction::PlayAlbum],
                selection: 1,
            }),
            ..AppState::default()
        };
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| super::render(frame, &state))
            .expect("draw");
        let rendered = format!("{:?}", terminal.backend().buffer());
        assert!(rendered.contains("Actions"));
        assert!(rendered.contains("Open Album"));
        assert!(rendered.contains("Play Album"));
    }

    #[test]
    fn help_layout_is_responsive_scrollable_and_uses_display_key_notation() {
        assert_eq!(help_column_count(100), 2);
        assert_eq!(help_column_count(60), 1);

        let lines = help_lines(
            &[
                BindingGroup::Navigation,
                BindingGroup::Playback,
                BindingGroup::General,
            ],
            super::DEFAULT,
        );
        let first = visible_help_lines(&lines, 0, 4);
        let later = visible_help_lines(&lines, 4, 4);
        assert_eq!(first.len(), 4);
        assert_eq!(later.len(), 4);
        assert_ne!(format!("{first:?}"), format!("{later:?}"));
        let text = format!("{lines:?}");
        assert!(text.contains("Ctrl-u") && text.contains("Space") && text.contains("gg"));
        assert!(!text.contains("KeyCode") && !text.contains("KeyModifiers"));
    }

    #[test]
    fn player_layout_collapses_artwork_before_essential_metadata() {
        let wide = player_layout(120, 7);
        assert_eq!(wide.artwork_width, Some(18));
        assert!(wide.show_album && wide.show_context && wide.show_toggles);

        let medium = player_layout(86, 5);
        assert_eq!(medium.artwork_width, Some(14));
        assert!(!medium.show_album && medium.show_context && medium.show_toggles);

        let small = player_layout(79, 5);
        assert_eq!(small.artwork_width, None);
        assert!(small.show_context);

        let narrow = player_layout(54, 5);
        assert_eq!(narrow.artwork_width, None);
        assert!(!narrow.show_context && !narrow.show_toggles);
    }

    #[test]
    fn player_title_and_status_truncate_without_hiding_the_time_row() {
        let track = Track::new(
            "long-track",
            "An Extremely Long Song Title That Must Not Push Playback Time Away",
            "An Artist Name That Is Also Deliberately Long",
            "A Long Album Name",
            Duration::from_secs(367),
        );
        let title = player_title_line(Some(&track), "⏸", 40);
        assert!(title.contains('…'));
        assert!(!title.contains('\n'));
        let state = AppState {
            playback: PlaybackSnapshot {
                current_track: Some(track),
                position: Duration::from_secs(61),
                ..PlaybackSnapshot::default()
            },
            ..AppState::default()
        };
        let rendered = render_text_at(&state, 80, 24);
        assert!(rendered.contains("1:01 / 6:07"));
    }

    #[test]
    fn player_context_and_idle_state_are_compact_and_authoritative() {
        let playlist = Playlist::new(
            "playlist-context",
            "A Playlist Name That Is Longer Than A Narrow Player Can Display",
            None,
            Vec::new(),
        );
        let state = AppState {
            playback: PlaybackSnapshot {
                current_track: Some(Track::new(
                    "context-track",
                    "Track",
                    "Artist",
                    "Album",
                    Duration::from_secs(60),
                )),
                context: PlaybackContext::Playlist {
                    playlist_id: playlist.id.clone(),
                    ordered_track_ids: vec![TrackId::new("one"), TrackId::new("two")],
                    current_index: 1,
                    complete: true,
                },
                ..PlaybackSnapshot::default()
            },
            playlists: vec![playlist],
            ..AppState::default()
        };
        assert!(player_status_line(&state, player_layout(86, 5), 72).contains("2/2"));

        let idle = AppState::default();
        assert_eq!(player_title_line(None, "▶", 40), "▶  Nothing playing");
        assert_eq!(
            player_status_line(&idle, player_layout(80, 5), 40),
            "Music.app idle"
        );
    }

    #[test]
    fn now_playing_artwork_fallback_is_concise_and_hides_debug_details() {
        let track = Track::new(
            "missing-art",
            "Track",
            "Artist",
            "Album",
            Duration::from_secs(60),
        );
        let state = AppState {
            playback: PlaybackSnapshot {
                current_track: Some(track),
                ..PlaybackSnapshot::default()
            },
            ..AppState::default()
        };
        let key = ArtworkKey::Track(TrackId::new("missing-art"));
        let fallback = now_playing_artwork_text(&state, &key);
        assert_eq!(fallback, "▣\nArtwork unavailable");
        assert!(!fallback.contains("Renderer:"));
        assert!(!fallback.contains("cached"));
    }

    #[test]
    fn shared_artwork_panel_has_visible_loading_missing_error_and_unsupported_states() {
        let key = ArtworkKey::Track(TrackId::new("track-artwork"));
        let mut state = AppState::default();

        state
            .artwork_cache
            .insert(key.clone(), ArtworkCacheEntry::Loading);
        assert!(
            artwork_panel_text(&state, &key, TerminalArtworkProtocol::Unicode)
                .contains("Loading Music.app")
        );

        state.artwork_cache.insert(
            key.clone(),
            ArtworkCacheEntry::Ready(Artwork {
                media_type: ArtworkMediaType::Jpeg,
                bytes: vec![0xff, 0xd8, 0xff, 0xd9],
            }),
        );
        let unsupported = artwork_panel_text(&state, &key, TerminalArtworkProtocol::Unicode);
        assert!(unsupported.contains("Artwork"));
        assert!(unsupported.contains("Unicode fallback"));

        state.artwork_cache.insert(
            key.clone(),
            ArtworkCacheEntry::Unavailable("No local Music.app artwork".to_owned()),
        );
        assert!(
            artwork_panel_text(&state, &key, TerminalArtworkProtocol::Unicode)
                .contains("No artwork")
        );

        state.artwork_cache.insert(
            key.clone(),
            ArtworkCacheEntry::Unavailable("Music.app artwork query failed: denied".to_owned()),
        );
        assert!(
            artwork_panel_text(&state, &key, TerminalArtworkProtocol::Unicode)
                .contains("query failed: denied")
        );
    }

    #[test]
    fn kitty_ready_artwork_panel_does_not_render_fallback_text() {
        let key = ArtworkKey::Track(TrackId::new("kitty-artwork"));
        let source = Artwork {
            media_type: ArtworkMediaType::Jpeg,
            bytes: vec![1, 2, 3],
        };
        let mut state = AppState::default();
        state
            .artwork_cache
            .insert(key.clone(), ArtworkCacheEntry::Ready(source.clone()));
        state.renderable_artwork_cache.insert(
            key.clone(),
            RenderableArtworkCacheEntry::Ready {
                source_fingerprint: source.fingerprint(),
                artwork: Artwork {
                    media_type: ArtworkMediaType::Png,
                    bytes: b"\x89PNG\r\n\x1a\nimage".to_vec(),
                },
            },
        );
        assert!(artwork_panel_text(&state, &key, TerminalArtworkProtocol::Kitty).is_empty());
    }

    #[test]
    fn large_collection_render_window_tracks_selection_without_building_every_row() {
        assert_eq!(visible_item_range(12_997, 0, 16, 2), 0..8);
        assert_eq!(visible_item_range(12_997, 100, 16, 2), 96..104);
        assert_eq!(visible_item_range(12_997, 12_996, 16, 2), 12_989..12_997);
        assert_eq!(visible_item_range(12_997, 100, 16, 1), 92..108);
    }

    #[test]
    fn compact_track_columns_drop_secondary_metadata_as_space_shrinks() {
        let wide = track_column_plan(110, TrackListMode::Songs);
        assert!(wide.album.is_some() && wide.artist.is_some() && wide.year && wide.plays);

        let medium = track_column_plan(90, TrackListMode::Songs);
        assert!(medium.artist.is_some());
        assert!(medium.album.is_none());
        assert!(!medium.plays);

        let narrow = track_column_plan(64, TrackListMode::Songs);
        assert!(narrow.artist.is_none() && narrow.album.is_none() && !narrow.favorite);

        let playlist_wide = track_column_plan(100, TrackListMode::Playlist);
        assert!(playlist_wide.artist.is_some() && playlist_wide.album.is_some());
    }

    #[test]
    fn compact_cells_truncate_without_creating_wrapped_rows() {
        assert_eq!(truncate_cell("A very long title", 8), "A very …");
        assert_eq!(truncate_cell("Long", 1), "…");
        assert!(!truncate_cell("A very long title", 8).contains('\n'));
    }

    #[test]
    fn compact_playlist_rows_do_not_render_no_description_filler() {
        let playlist = Playlist::unloaded(
            "playlist-without-description",
            "Compact Playlist",
            None,
            PlaylistKind::User,
            None,
        );
        let state = AppState {
            navigation: NavigationState {
                active: Route::Section(Screen::MadeForYou),
                history: Vec::new(),
            },
            playlists: vec![playlist],
            view_status: ViewStatus::Loaded,
            ..AppState::default()
        };
        let rendered = render_text(&state);
        assert!(rendered.contains("Compact Playlist"));
        assert!(!rendered.contains("No description"));
    }

    #[test]
    fn songs_show_a_subtle_playing_marker_in_the_compact_row() {
        let first = Track::new("first", "First", "Artist", "Album", Duration::from_secs(60));
        let second = Track::new(
            "second",
            "Second",
            "Artist",
            "Album",
            Duration::from_secs(60),
        );
        let state = AppState {
            navigation: NavigationState {
                active: Route::Section(Screen::Songs),
                history: Vec::new(),
            },
            library: vec![first, second.clone()],
            playback: PlaybackSnapshot {
                current_track: Some(second),
                ..PlaybackSnapshot::default()
            },
            view_status: ViewStatus::Loaded,
            ..AppState::default()
        };
        assert!(render_text(&state).contains("▶ 2  Second"));
    }

    #[test]
    fn search_explains_that_a_cold_refresh_may_not_have_found_a_match_yet() {
        let state = AppState {
            navigation: NavigationState {
                active: Route::Section(Screen::Search),
                history: Vec::new(),
            },
            search_query: "not loaded yet".to_owned(),
            search_results: Vec::<LocalSearchResult>::new(),
            library_status: CollectionLoadState::Loading {
                loaded: 400,
                total: 12_997,
            },
            view_status: ViewStatus::Loaded,
            ..AppState::default()
        };
        assert!(render_text(&state).contains("No matches in the loaded library yet"));
    }

    use super::render;

    #[test]
    fn renders_at_minimum_and_small_sizes_without_panicking() {
        for (width, height) in [(80, 24), (40, 10)] {
            let backend = TestBackend::new(width, height);
            let mut terminal = Terminal::new(backend).expect("test terminal");
            let state = AppState {
                help_open: true,
                ..AppState::default()
            };
            terminal
                .draw(|frame| render(frame, &state))
                .expect("render succeeds");
        }
    }

    #[test]
    fn playlist_detail_renders_mock_metadata_and_tracks() {
        let playlist = Playlist::new(
            "playlist-one",
            "Terminal Focus",
            Some("A focused mock playlist.".to_owned()),
            vec![Track::new(
                "track-one",
                "Midnight Terminal",
                "The Asyncs",
                "Event Loop",
                Duration::from_secs(238),
            )],
        );
        let state = AppState {
            navigation: NavigationState {
                active: Route::PlaylistDetail {
                    playlist_id: PlaylistId::new("playlist-one"),
                },
                history: Vec::new(),
            },
            playlists: vec![playlist],
            backend_status: BackendStatus::Ready {
                name: "Mock Playback (no audio)".to_owned(),
            },
            view_status: ViewStatus::Loaded,
            ..AppState::default()
        };
        let backend = TestBackend::new(100, 24);
        let mut terminal = Terminal::new(backend).expect("test terminal");

        terminal
            .draw(|frame| render(frame, &state))
            .expect("render succeeds");

        let rendered =
            terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .fold(String::new(), |mut output, cell| {
                    output.push_str(cell.symbol());
                    output
                });
        assert!(rendered.contains("Playlist Detail"));
        assert!(rendered.contains("Terminal Focus"));
        assert!(rendered.contains("A focused mock playlist."));
        assert!(rendered.contains("1 tracks"));
        assert!(rendered.contains("Midnight Terminal"));
        assert!(rendered.contains("The Asyncs"));
        assert!(rendered.contains("3:58"));
        assert!(rendered.contains("P play playlist"));
        assert!(rendered.contains("Mock Playback (no audio)"));
    }

    #[test]
    fn playlist_loading_empty_and_error_states_render_distinctly() {
        let playlist_id = PlaylistId::new("musicapp:playlist:persistent:STATE");
        let mut playlist = Playlist::unloaded(
            playlist_id.to_string(),
            "Stateful Playlist",
            None,
            PlaylistKind::User,
            None,
        );
        playlist.contents_state = PlaylistLoadState::Loading {
            loaded: 0,
            total: Some(127),
        };
        playlist.track_count = 127;
        let mut state = AppState {
            navigation: NavigationState {
                active: Route::PlaylistDetail {
                    playlist_id: playlist_id.clone(),
                },
                history: Vec::new(),
            },
            playlists: vec![playlist],
            view_status: ViewStatus::Loaded,
            ..AppState::default()
        };

        let loading = render_text(&state);
        assert!(loading.contains("Loading tracks… 0 / 127"));
        assert!(!loading.contains("Empty playlist"));

        state.playlists[0].tracks.push(Track::new(
            "musicapp:persistent:T1",
            "Visible Immediately",
            "Artist",
            "Album",
            Duration::from_secs(60),
        ));
        state.playlists[0].contents_state = PlaylistLoadState::PartiallyLoaded {
            loaded: 1,
            total: 127,
        };
        let partial = render_text(&state);
        assert!(partial.contains("Loading tracks… 1 / 127"));
        assert!(partial.contains("Visible Immediately"));

        state.playlists[0].tracks.clear();
        state.playlists[0].track_count = 0;
        state.playlists[0].contents_state = PlaylistLoadState::Empty;
        assert!(render_text(&state).contains("Empty playlist"));

        state.playlists[0].contents_state =
            PlaylistLoadState::Error("Automation timed out safely".to_owned());
        let failed = render_text(&state);
        assert!(failed.contains("Failed to load playlist"));
        assert!(failed.contains("Automation timed out safely"));
    }

    #[test]
    fn persistent_player_bar_renders_album_metadata() {
        let state = AppState {
            playback: PlaybackSnapshot {
                current_track: Some(Track::new(
                    "track-one",
                    "Current Song",
                    "Current Artist",
                    "Current Album",
                    Duration::from_secs(180),
                )),
                ..PlaybackSnapshot::default()
            },
            ..AppState::default()
        };

        let rendered = render_text(&state);

        assert!(rendered.contains("Current Song — Current Artist"));
        assert!(rendered.contains("Album: Current Album"));
        assert!(rendered.contains("Now Playing"));
    }

    #[test]
    fn now_playing_artwork_panel_collapses_before_essential_controls_on_small_terminals() {
        let state = AppState {
            playback: PlaybackSnapshot {
                current_track: Some(Track::new(
                    "track-one",
                    "Current Song",
                    "Current Artist",
                    "Current Album",
                    Duration::from_secs(180),
                )),
                ..PlaybackSnapshot::default()
            },
            ..AppState::default()
        };

        let large = render_text_at(&state, 100, 24);
        assert!(large.contains("Now Playing"));
        assert!(large.contains("Current Song — Current Artist"));

        let small = render_text_at(&state, 40, 10);
        assert!(!small.contains("Now Playing"));
        assert!(small.contains("Current Song"));
    }

    #[test]
    fn player_bar_derives_playlist_position_from_app_playback_context() {
        let playlist = Playlist::new(
            "playlist-context",
            "Context Mix",
            None,
            vec![
                Track::new("t1", "One", "Artist", "Album", Duration::from_secs(60)),
                Track::new("t2", "Two", "Artist", "Album", Duration::from_secs(60)),
                Track::new("t3", "Three", "Artist", "Album", Duration::from_secs(60)),
            ],
        );
        let state = AppState {
            playback: PlaybackSnapshot {
                current_track: Some(playlist.tracks[1].clone()),
                context: PlaybackContext::Playlist {
                    playlist_id: playlist.id.clone(),
                    ordered_track_ids: playlist
                        .tracks
                        .iter()
                        .map(|track| track.id.clone())
                        .collect(),
                    current_index: 1,
                    complete: true,
                },
                ..PlaybackSnapshot::default()
            },
            playlists: vec![playlist],
            ..AppState::default()
        };

        assert!(render_text(&state).contains("Context Mix 2/3"));
    }

    #[test]
    fn status_line_renders_structured_apple_api_state() {
        let state = AppState {
            auth_status: AuthStatus::CredentialsStored,
            ..AppState::default()
        };

        assert!(render_text(&state).contains("Apple API: Credentials stored"));
    }

    #[test]
    fn player_bar_renders_control_icon_from_current_app_state() {
        let mut state = AppState {
            playback: PlaybackSnapshot {
                status: PlaybackStatus::Playing,
                ..PlaybackSnapshot::default()
            },
            ..AppState::default()
        };

        let playing = render_text(&state);
        assert!(playing.contains("⏸"));
        assert!(!playing.contains("▶"));

        state.playback.status = PlaybackStatus::Paused;
        let paused = render_text(&state);
        assert!(paused.contains("▶"));
        assert!(!paused.contains("⏸"));
    }

    #[tokio::test]
    async fn every_sidebar_section_renders_distinct_mock_content() {
        let mut state = loaded_mock_state().await;
        let cases = [
            (Screen::ListenNow, "Recently Played"),
            (Screen::Browse, "New Releases"),
            (Screen::Radio, "Apple Music 1 (Mock)"),
            (Screen::RecentlyAdded, "added 2026-07-30"),
            (Screen::RecentlyPlayed, "12 plays"),
            (Screen::Artists, "The Asyncs"),
            (Screen::Albums, "Event Loop"),
            (Screen::Songs, "Midnight Terminal"),
            (Screen::MadeForYou, "Terminal Focus"),
            (Screen::Playlists, "Terminal Focus"),
        ];

        for (screen, expected_content) in cases {
            reduce(&mut state, Action::GoTo(screen));
            let rendered = render_text(&state);

            assert!(rendered.contains(screen.label()));
            assert!(
                rendered.contains(expected_content),
                "{} should render '{expected_content}'",
                screen.label()
            );
        }
    }

    #[tokio::test]
    async fn artist_and_album_details_render_required_metadata() {
        let mut state = loaded_mock_state().await;

        state.navigation.active = Route::ArtistDetail {
            artist_id: crate::domain::ArtistId::new("mock-artist-asyncs"),
        };
        let artist_detail = render_text(&state);
        assert!(artist_detail.contains("Artist Detail"));
        assert!(artist_detail.contains("The Asyncs"));
        assert!(artist_detail.contains("Albums: Event Loop"));
        assert!(artist_detail.contains("top tracks"));

        state.navigation.active = Route::AlbumDetail {
            album_id: crate::domain::AlbumId::new("mock-album-event-loop"),
        };
        let album_detail = render_text(&state);
        assert!(album_detail.contains("Album Detail"));
        assert!(album_detail.contains("Event Loop"));
        assert!(album_detail.contains("The Asyncs • 2026"));
        assert!(album_detail.contains("Midnight Terminal"));
        assert!(album_detail.contains("Artwork"));
    }

    fn render_text(state: &AppState) -> String {
        render_text_at(state, 100, 24)
    }

    fn render_text_at(state: &AppState, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| render(frame, state))
            .expect("render succeeds");
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .fold(String::new(), |mut output, cell| {
                output.push_str(cell.symbol());
                output
            })
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
