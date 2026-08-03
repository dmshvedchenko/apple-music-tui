mod theme;

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Gauge, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::{
    app::state::{
        AppState, BackendStatus, Focus, LocalSearchResult, QUEUE_PANE_MIN_WIDTH, Route, Screen,
        ViewStatus,
    },
    backend::capabilities::Capability,
    domain::{
        Album, Artist, BackendAvailability, CollectionLoadState, DataOrigin, PlaybackStatus,
        PlaylistKind, RepeatMode, Station, Track,
    },
    input::{BindingGroup, bindings},
};

use self::theme::{DEFAULT, Theme};

pub fn render(frame: &mut Frame<'_>, state: &AppState) {
    let area = frame.area();
    frame.render_widget(
        Block::default().style(Style::default().bg(DEFAULT.background)),
        area,
    );

    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(5)])
        .split(area);

    render_body(frame, vertical[0], state, DEFAULT);
    render_player(frame, vertical[1], state, DEFAULT);

    if state.help_open {
        render_help(frame, area, DEFAULT);
    }
}

fn render_body(frame: &mut Frame<'_>, area: Rect, state: &AppState, theme: Theme) {
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
        render_content(frame, columns[1], state, theme);
        render_queue(frame, columns[2], state, theme);
    } else {
        let sidebar_width = area.width.min(22);
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(sidebar_width), Constraint::Min(1)])
            .split(area);
        render_sidebar(frame, columns[0], state, theme);
        render_content(frame, columns[1], state, theme);
    }
}

fn render_sidebar(frame: &mut Frame<'_>, area: Rect, state: &AppState, theme: Theme) {
    let mut items = Vec::new();
    for index in 0..3 {
        items.push(sidebar_screen_item(index, state, theme));
    }
    items.push(sidebar_heading("LIBRARY", theme));
    for index in 3..8 {
        items.push(sidebar_screen_item(index, state, theme));
    }
    items.push(sidebar_heading("PLAYLISTS", theme));
    items.push(sidebar_screen_item(8, state, theme));
    items.push(sidebar_heading("LOCAL", theme));
    items.push(sidebar_screen_item(9, state, theme));

    let list = List::new(items)
        .block(panel_block(
            sidebar_title(state),
            state.focus == Focus::Sidebar,
            theme,
        ))
        .style(Style::default().fg(theme.foreground));
    frame.render_widget(list, area);
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

fn render_content(frame: &mut Frame<'_>, area: Rect, state: &AppState, theme: Theme) {
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
        ViewStatus::Loading => frame.render_widget(
            Paragraph::new("Loading backend…")
                .style(Style::default().fg(theme.muted))
                .alignment(Alignment::Center),
            rows[1],
        ),
        ViewStatus::Empty => frame.render_widget(
            Paragraph::new("No items")
                .style(Style::default().fg(theme.muted))
                .alignment(Alignment::Center),
            rows[1],
        ),
        ViewStatus::Error(message) => frame.render_widget(
            Paragraph::new(message.as_str()).style(Style::default().fg(theme.error)),
            rows[1],
        ),
        ViewStatus::Loaded => render_loaded_content(frame, rows[1], state, theme),
    }
}

fn render_loaded_content(frame: &mut Frame<'_>, area: Rect, state: &AppState, theme: Theme) {
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
        Route::Section(Screen::Artists) => {
            if state.artists.is_empty() && collection_is_loading(&state.library_status) {
                render_collection_loading(frame, area, "artists", theme);
            } else {
                render_artists(frame, area, state, theme);
            }
        }
        Route::Section(Screen::Albums) => {
            if state.albums.is_empty() && collection_is_loading(&state.library_status) {
                render_collection_loading(frame, area, "albums", theme);
            } else {
                render_albums(
                    frame,
                    area,
                    &state.albums,
                    state.content_selection,
                    false,
                    theme,
                );
            }
        }
        Route::Section(Screen::Songs) => {
            if state.library.is_empty() && collection_is_loading(&state.library_status) {
                render_collection_loading(frame, area, "songs", theme);
            } else {
                let tracks = state.library.iter().collect::<Vec<_>>();
                render_track_list(frame, area, &tracks, state.content_selection, theme);
            }
        }
        Route::Section(Screen::Search) => render_search(frame, area, state, theme),
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
        Route::AlbumDetail { album_id } => {
            let album = state.albums.iter().find(|album| album.id == *album_id);
            if let Some(album) = album {
                render_album_detail(frame, area, state, album, theme);
            } else {
                render_missing_detail(frame, area, "album", theme);
            }
        }
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
    let items = state
        .search_results
        .iter()
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
        frame.render_widget(
            Paragraph::new("No local matches")
                .style(Style::default().fg(theme.muted))
                .alignment(Alignment::Center),
            rows[1],
        );
    } else {
        render_selectable_list(frame, rows[1], items, state.content_selection, theme);
    }
}

fn collection_is_loading(status: &CollectionLoadState) -> bool {
    matches!(
        status,
        CollectionLoadState::NotStarted | CollectionLoadState::Loading { .. }
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

fn render_artists(frame: &mut Frame<'_>, area: Rect, state: &AppState, theme: Theme) {
    let items = state
        .artists
        .iter()
        .map(|artist| {
            ListItem::new(vec![
                Line::styled(
                    artist.name.as_str(),
                    Style::default()
                        .fg(theme.foreground)
                        .add_modifier(Modifier::BOLD),
                ),
                Line::styled(
                    format!(
                        "  {} album(s) • {} top track(s)",
                        artist.album_ids.len(),
                        artist.top_track_ids.len()
                    ),
                    Style::default().fg(theme.muted),
                ),
            ])
        })
        .collect::<Vec<_>>();
    render_selectable_list(frame, area, items, state.content_selection, theme);
}

fn render_albums(
    frame: &mut Frame<'_>,
    area: Rect,
    albums: &[Album],
    selection: usize,
    show_added_date: bool,
    theme: Theme,
) {
    let items = albums
        .iter()
        .map(|album| {
            let metadata = if show_added_date {
                format!(
                    "{} • {} • added {}",
                    album.artist, album.year, album.added_date
                )
            } else {
                format!(
                    "{} • {} • {} tracks",
                    album.artist,
                    album.year,
                    album.tracks.len()
                )
            };
            ListItem::new(vec![
                Line::styled(
                    album.title.as_str(),
                    Style::default()
                        .fg(theme.foreground)
                        .add_modifier(Modifier::BOLD),
                ),
                Line::styled(format!("  {metadata}"), Style::default().fg(theme.muted)),
            ])
        })
        .collect::<Vec<_>>();
    render_selectable_list(frame, area, items, selection, theme);
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
    render_track_list(frame, rows[1], &tracks, state.content_selection, theme);
}

fn render_album_detail(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &AppState,
    album: &Album,
    theme: Theme,
) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(4), Constraint::Min(1)])
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
    frame.render_widget(
        Paragraph::new(vec![
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
                format!("{} tracks  •  Esc/h back", album.tracks.len()),
                Style::default().fg(theme.accent),
            ),
        ]),
        rows[0],
    );

    let tracks = album.tracks.iter().collect::<Vec<_>>();
    render_track_list(frame, rows[1], &tracks, state.content_selection, theme);
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
            CollectionLoadState::NotStarted | CollectionLoadState::Loading { .. } => {
                "Loading Music.app playlists…"
            }
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

    let items = state
        .playlists
        .iter()
        .map(|playlist| {
            let description = playlist.description.as_deref().unwrap_or("No description");
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
            let count = if playlist.tracks_loaded {
                playlist.tracks.len()
            } else {
                playlist.track_count
            };
            ListItem::new(vec![
                Line::from(vec![
                    Span::styled(format!("{source:<6}"), Style::default().fg(theme.accent)),
                    Span::styled(
                        &playlist.name,
                        Style::default()
                            .fg(theme.foreground)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("  {count} tracks"),
                        Style::default().fg(theme.muted),
                    ),
                ]),
                Line::styled(
                    format!("   {description}"),
                    Style::default().fg(theme.muted),
                ),
            ])
        })
        .collect::<Vec<_>>();
    render_selectable_list(frame, area, items, state.content_selection, theme);
}

fn render_playlist_detail(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &AppState,
    playlist: &crate::domain::Playlist,
    theme: Theme,
) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(area);
    let description = playlist.description.as_deref().unwrap_or("No description");
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(
                playlist.name.as_str(),
                Style::default()
                    .fg(theme.foreground)
                    .add_modifier(Modifier::BOLD),
            ),
            Line::styled(description, Style::default().fg(theme.muted)),
            Line::styled(
                format!(
                    "{} tracks  •  P play playlist  •  Esc/h back",
                    playlist.track_count
                ),
                Style::default().fg(theme.accent),
            ),
        ]),
        rows[0],
    );

    if playlist.tracks_loaded {
        let tracks = playlist.tracks.iter().collect::<Vec<_>>();
        render_track_list(frame, rows[1], &tracks, state.content_selection, theme);
    } else {
        frame.render_widget(
            Paragraph::new(format!(
                "Loading playlist tracks… {}/{}",
                playlist.tracks.len(),
                playlist.track_count
            ))
            .style(Style::default().fg(theme.muted))
            .alignment(Alignment::Center),
            rows[1],
        );
    }
}

fn render_track_list(
    frame: &mut Frame<'_>,
    area: Rect,
    tracks: &[&Track],
    selection: usize,
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

    let items = tracks
        .iter()
        .enumerate()
        .map(|(index, track)| {
            let favorite = if track.is_favorite { "♥" } else { " " };
            ListItem::new(vec![
                Line::from(vec![
                    Span::styled(
                        format!("{:>2}", index + 1),
                        Style::default().fg(theme.muted),
                    ),
                    Span::raw("  "),
                    Span::styled(&track.title, Style::default().fg(theme.foreground)),
                    Span::styled(
                        format!("  — {}", track.artist),
                        Style::default().fg(theme.muted),
                    ),
                    Span::styled(
                        format!("  {}", format_time(track.duration.as_secs())),
                        Style::default().fg(theme.muted),
                    ),
                    Span::styled(format!("  {favorite}"), Style::default().fg(theme.accent)),
                ]),
                Line::styled(
                    format!("    {}", track_metadata_summary(track)),
                    Style::default().fg(theme.muted),
                ),
            ])
        })
        .collect::<Vec<_>>();
    let list = List::new(items).highlight_style(
        Style::default()
            .bg(theme.selection)
            .add_modifier(Modifier::BOLD),
    );
    let mut list_state = ListState::default().with_selected(Some(selection));
    frame.render_stateful_widget(list, area, &mut list_state);
}

fn track_metadata_summary(track: &Track) -> String {
    let mut fields = Vec::with_capacity(6);
    if track.album != "Unknown Album" {
        fields.push(format!("Album: {}", track.album));
    }
    if let Some(genre) = &track.metadata.genre {
        fields.push(format!("Genre: {genre}"));
    }
    if let Some(year) = track.metadata.year {
        fields.push(format!("Year: {year}"));
    }
    if let Some(play_count) = track.metadata.play_count {
        fields.push(format!("Plays: {play_count}"));
    }
    if let Some(date_added) = &track.metadata.date_added {
        fields.push(format!("Added: {date_added}"));
    }
    if let Some(rating) = track.metadata.rating {
        fields.push(format!("Rating: {rating}/100"));
    }
    if fields.is_empty() {
        "Metadata unavailable".to_owned()
    } else {
        fields.join(" • ")
    }
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
        CollectionLoadState::Loading { loaded, total } if *total == 0 => {
            "Library: discovering…".to_owned()
        }
        CollectionLoadState::Loading { loaded, total } => {
            format!("Library: {loaded}/{total}")
        }
        CollectionLoadState::Loaded { total } => format!("Library: {total} local items"),
        CollectionLoadState::Error(_) => "Library: unavailable".to_owned(),
    };
    let apple_api = state.auth_status.label();
    Paragraph::new(vec![
        Line::styled(format!("{text}{notification}"), Style::default().fg(color)),
        Line::styled(
            format!("{library}  •  Apple API: {apple_api} (optional)"),
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

fn render_player(frame: &mut Frame<'_>, area: Rect, state: &AppState, theme: Theme) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .style(Style::default().bg(theme.background));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);
    let control_icon = playback_control_icon(state.playback.status);
    let track = state.playback.current_track.as_ref();
    let title = track.map_or("Nothing playing", |track| track.title.as_str());
    let artist = track.map_or("", |track| track.artist.as_str());
    let album = track.map_or("", |track| track.album.as_str());
    let favorite = track.is_some_and(|track| track.is_favorite);
    let repeat = match state.playback.repeat {
        RepeatMode::Off => "off",
        RepeatMode::All => "all",
        RepeatMode::One => "one",
    };
    let metadata = format!("{control_icon}  {title} — {artist}");
    frame.render_widget(
        Paragraph::new(metadata).style(Style::default().fg(theme.foreground)),
        rows[0],
    );

    let controls = format!(
        "Album: {album}    vol {:>3}%{}  shuffle {}  repeat {repeat}{}",
        state.playback.volume,
        if state.playback.muted { " muted" } else { "" },
        if state.playback.shuffle { "on" } else { "off" },
        if favorite { "  ♥" } else { "" }
    );
    frame.render_widget(
        Paragraph::new(controls).style(Style::default().fg(theme.foreground)),
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
        rows[2],
    );
}

const fn playback_control_icon(status: PlaybackStatus) -> &'static str {
    match status {
        PlaybackStatus::Playing => "⏸",
        PlaybackStatus::Paused | PlaybackStatus::Stopped => "▶",
    }
}

fn render_help(frame: &mut Frame<'_>, area: Rect, theme: Theme) {
    let popup = centered_rect(area, 86, 82);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(" Help ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .style(Style::default().bg(theme.background));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    if inner.width >= 68 {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(inner);
        frame.render_widget(
            Paragraph::new(help_lines(
                &[BindingGroup::Navigation, BindingGroup::General],
                theme,
            ))
            .wrap(Wrap { trim: false }),
            columns[0],
        );
        frame.render_widget(
            Paragraph::new(help_lines(&[BindingGroup::Playback], theme)).wrap(Wrap { trim: false }),
            columns[1],
        );
    } else {
        frame.render_widget(
            Paragraph::new(help_lines(
                &[
                    BindingGroup::Navigation,
                    BindingGroup::Playback,
                    BindingGroup::General,
                ],
                theme,
            ))
            .wrap(Wrap { trim: false }),
            inner,
        );
    }
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
                .filter(|binding| binding.group == *group)
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

fn format_time(seconds: u64) -> String {
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use ratatui::{Terminal, backend::TestBackend};

    use crate::{
        app::{
            action::Action,
            reducer::reduce,
            state::{AppState, BackendStatus, NavigationState, Route, Screen, ViewStatus},
        },
        auth::AuthStatus,
        backend::{BackendEvent, MusicBackend, mock::MockMusicBackend},
        domain::{PlaybackSnapshot, PlaybackStatus, Playlist, PlaylistId, Track},
    };

    use super::{playback_control_icon, track_metadata_summary};

    #[test]
    fn playback_control_icon_comes_from_authoritative_playback_status() {
        assert_eq!(playback_control_icon(PlaybackStatus::Playing), "⏸");
        assert_eq!(playback_control_icon(PlaybackStatus::Paused), "▶");
        assert_eq!(playback_control_icon(PlaybackStatus::Stopped), "▶");
    }

    #[test]
    fn local_track_summary_only_renders_available_rich_metadata() {
        let mut track = Track::new(
            "local-track",
            "Local Song",
            "Local Artist",
            "Local Album",
            Duration::from_secs(200),
        );
        track.metadata.genre = Some("Electronic".to_owned());
        track.metadata.year = Some(2025);
        track.metadata.play_count = Some(7);
        track.metadata.date_added = Some("2026-01-02T03:04:05.000Z".to_owned());
        track.metadata.rating = Some(80);

        assert_eq!(
            track_metadata_summary(&track),
            "Album: Local Album • Genre: Electronic • Year: 2025 • Plays: 7 • Added: 2026-01-02T03:04:05.000Z • Rating: 80/100"
        );

        track.metadata.genre = None;
        track.metadata.year = None;
        track.metadata.play_count = None;
        track.metadata.date_added = None;
        track.metadata.rating = None;
        assert_eq!(track_metadata_summary(&track), "Album: Local Album");
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
        assert!(rendered.contains("Mock Playback (no audio)"));
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
            (Screen::Artists, "The Asyncs"),
            (Screen::Albums, "Event Loop"),
            (Screen::Songs, "Midnight Terminal"),
            (Screen::MadeForYou, "Terminal Focus"),
            (Screen::Playlists, "A focused set for building"),
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

        reduce(&mut state, Action::GoTo(Screen::Artists));
        reduce(&mut state, Action::OpenSelected);
        let artist_detail = render_text(&state);
        assert!(artist_detail.contains("Artist Detail"));
        assert!(artist_detail.contains("The Asyncs"));
        assert!(artist_detail.contains("Albums: Event Loop"));
        assert!(artist_detail.contains("top tracks"));

        reduce(&mut state, Action::Back);
        reduce(&mut state, Action::GoTo(Screen::Albums));
        reduce(&mut state, Action::OpenSelected);
        let album_detail = render_text(&state);
        assert!(album_detail.contains("Album Detail"));
        assert!(album_detail.contains("Event Loop"));
        assert!(album_detail.contains("The Asyncs • 2026"));
        assert!(album_detail.contains("Midnight Terminal"));
    }

    fn render_text(state: &AppState) -> String {
        let backend = TestBackend::new(100, 24);
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
