use std::collections::{BTreeMap, BTreeSet};

use crate::{
    auth::AuthStatus,
    backend::capabilities::Capabilities,
    domain::{
        Album, AlbumId, Artist, ArtistId, Artwork, ArtworkKey, BackendAvailability,
        CollectionLoadState, PlaybackSnapshot, Playlist, PlaylistHierarchy, PlaylistId, QueueItem,
        RecentlyPlayedEntry, Station, Track, TrackId, VisiblePlaylistEntry,
    },
};

pub const QUEUE_PANE_MIN_WIDTH: u16 = 108;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Screen {
    ListenNow,
    Browse,
    Radio,
    RecentlyAdded,
    RecentlyPlayed,
    Artists,
    Albums,
    Songs,
    MadeForYou,
    Search,
    Playlists,
}

impl Screen {
    pub const ALL: [Self; 11] = [
        Self::ListenNow,
        Self::Browse,
        Self::Radio,
        Self::RecentlyAdded,
        Self::RecentlyPlayed,
        Self::Artists,
        Self::Albums,
        Self::Songs,
        Self::MadeForYou,
        Self::Playlists,
        Self::Search,
    ];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::ListenNow => "Listen Now",
            Self::Browse => "Browse",
            Self::Radio => "Radio",
            Self::RecentlyAdded => "Recently Added",
            Self::RecentlyPlayed => "Recently Played",
            Self::Artists => "Artists",
            Self::Albums => "Albums",
            Self::Songs => "Songs",
            Self::MadeForYou => "Made for You",
            Self::Search => "Search",
            Self::Playlists => "Playlists",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Route {
    Section(Screen),
    NowPlaying,
    ArtistDetail { artist_id: ArtistId },
    AlbumDetail { album_id: AlbumId },
    PlaylistDetail { playlist_id: PlaylistId },
}

impl Route {
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Section(screen) => screen.label(),
            Self::NowPlaying => "Now Playing",
            Self::ArtistDetail { .. } => "Artist Detail",
            Self::AlbumDetail { .. } => "Album Detail",
            Self::PlaylistDetail { .. } => "Playlist Detail",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NavigationEntry {
    pub route: Route,
    pub content_selection: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NavigationState {
    pub active: Route,
    pub history: Vec<NavigationEntry>,
}

impl Default for NavigationState {
    fn default() -> Self {
        Self {
            active: Route::Section(Screen::ListenNow),
            history: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Focus {
    #[default]
    Sidebar,
    Content,
    Queue,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BackendStatus {
    Initializing,
    Ready { name: String },
    Error { message: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ViewStatus {
    Loading,
    Loaded,
    Empty,
    Error(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LocalSearchResult {
    Track(crate::domain::TrackId),
    Artist(ArtistId),
    Album(AlbumId),
    Playlist(PlaylistId),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CollectionKind {
    Songs,
    Albums,
    Artists,
}

impl CollectionKind {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Songs => "Songs",
            Self::Albums => "Albums",
            Self::Artists => "Artists",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CollectionSort {
    SongTitle,
    SongArtist,
    SongAlbum,
    SongDateAdded,
    SongYear,
    SongPlayCount,
    AlbumTitle,
    AlbumArtist,
    AlbumYear,
    AlbumRecentlyAdded,
    ArtistName,
    ArtistAlbumCount,
    ArtistTrackCount,
}

impl CollectionSort {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::SongTitle | Self::AlbumTitle => "Title",
            Self::SongArtist | Self::AlbumArtist => "Artist",
            Self::SongAlbum => "Album",
            Self::SongDateAdded => "Date Added",
            Self::SongYear | Self::AlbumYear => "Year",
            Self::SongPlayCount => "Play Count",
            Self::AlbumRecentlyAdded => "Recently Added",
            Self::ArtistName => "Name",
            Self::ArtistAlbumCount => "Album Count",
            Self::ArtistTrackCount => "Track Count",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CollectionViewState {
    pub sort: CollectionSort,
    pub descending: bool,
    pub filter: String,
    pub indices: Vec<usize>,
    pub normalized_filter_keys: Vec<String>,
    pub source_len: Option<usize>,
    pub rebuild_count: u64,
}

impl CollectionViewState {
    #[must_use]
    pub fn new(sort: CollectionSort) -> Self {
        Self {
            sort,
            descending: false,
            filter: String::new(),
            indices: Vec::new(),
            normalized_filter_keys: Vec::new(),
            source_len: None,
            rebuild_count: 0,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LibraryViews {
    pub songs: CollectionViewState,
    pub albums: CollectionViewState,
    pub artists: CollectionViewState,
}

impl Default for LibraryViews {
    fn default() -> Self {
        Self {
            songs: CollectionViewState::new(CollectionSort::SongTitle),
            albums: CollectionViewState::new(CollectionSort::AlbumTitle),
            artists: CollectionViewState::new(CollectionSort::ArtistName),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SortMenuState {
    pub collection: CollectionKind,
    pub selection: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FilterEditorState {
    pub collection: CollectionKind,
    pub original: String,
    pub draft: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContextTarget {
    Track(TrackId),
    PlaylistTrack {
        playlist_id: PlaylistId,
        track_id: TrackId,
        index: usize,
    },
    Album(AlbumId),
    Artist(ArtistId),
    Playlist(PlaylistId),
    Folder(PlaylistId),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextAction {
    PlayTrack,
    OpenAlbum,
    OpenArtist,
    PlayAlbum,
    OpenPlaylist,
    PlayPlaylist,
    ExpandFolder,
    CollapseFolder,
    RemoveFromPlaylist,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionMenuState {
    pub target: ContextTarget,
    pub actions: Vec<ContextAction>,
    pub selection: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlaylistTrackRemovalConfirmation {
    pub playlist_id: PlaylistId,
    pub index: usize,
    pub track_id: TrackId,
    pub track_title: String,
    pub playlist_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalSearchIndexEntry {
    pub result: LocalSearchResult,
    pub normalized_text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ArtworkCacheEntry {
    Loading,
    Ready(Artwork),
    /// A fresh Music.app object was not available yet. This is deliberately
    /// not a permanent negative cache entry and may be requested again.
    Transient(String),
    Unavailable(String),
}

/// A display asset derived from source artwork only when a terminal protocol needs it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RenderableArtworkCacheEntry {
    Loading {
        source_fingerprint: u64,
    },
    Ready {
        source_fingerprint: u64,
        artwork: Artwork,
    },
    Unavailable {
        source_fingerprint: u64,
        message: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppState {
    pub should_quit: bool,
    pub stop_playback_on_exit: bool,
    pub navigation: NavigationState,
    pub sidebar_selection: usize,
    pub content_selection: usize,
    pub queue_selection: usize,
    pub focus: Focus,
    pub playback: PlaybackSnapshot,
    pub queue: Vec<QueueItem>,
    pub library: Vec<Track>,
    pub artists: Vec<Artist>,
    pub albums: Vec<Album>,
    pub recently_added: Vec<Album>,
    pub recently_played: Vec<RecentlyPlayedEntry>,
    pub stations: Vec<Station>,
    pub playlists: Vec<Playlist>,
    pub playlist_hierarchy: PlaylistHierarchy,
    pub expanded_playlist_folders: BTreeSet<PlaylistId>,
    pub artwork_cache: BTreeMap<ArtworkKey, ArtworkCacheEntry>,
    pub artwork_cache_order: Vec<ArtworkKey>,
    /// Stable track identities retained only while an artwork request may retry.
    pub artwork_request_tracks: BTreeMap<ArtworkKey, crate::domain::TrackId>,
    pub artwork_retry_attempts: BTreeMap<ArtworkKey, u8>,
    pub renderable_artwork_cache: BTreeMap<ArtworkKey, RenderableArtworkCacheEntry>,
    pub renderable_artwork_cache_order: Vec<ArtworkKey>,
    pub search_query: String,
    pub search_results: Vec<LocalSearchResult>,
    pub search_index: Vec<LocalSearchIndexEntry>,
    pub search_input_active: bool,
    pub library_views: LibraryViews,
    pub sort_menu: Option<SortMenuState>,
    pub filter_editor: Option<FilterEditorState>,
    pub capabilities: Capabilities,
    pub backend_availability: BackendAvailability,
    pub backend_status: BackendStatus,
    pub auth_status: AuthStatus,
    pub view_status: ViewStatus,
    pub library_status: CollectionLoadState,
    pub playlist_status: CollectionLoadState,
    pub notification: Option<String>,
    pub help_open: bool,
    pub help_scroll: usize,
    pub action_menu: Option<ActionMenuState>,
    pub playlist_track_removal_confirmation: Option<PlaylistTrackRemovalConfirmation>,
    pub playlist_track_removal_in_flight: bool,
    pub terminal_size: (u16, u16),
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            should_quit: false,
            stop_playback_on_exit: false,
            navigation: NavigationState::default(),
            sidebar_selection: 0,
            content_selection: 0,
            queue_selection: 0,
            focus: Focus::Sidebar,
            playback: PlaybackSnapshot::default(),
            queue: Vec::new(),
            library: Vec::new(),
            artists: Vec::new(),
            albums: Vec::new(),
            recently_added: Vec::new(),
            recently_played: Vec::new(),
            stations: Vec::new(),
            playlists: Vec::new(),
            playlist_hierarchy: PlaylistHierarchy::default(),
            expanded_playlist_folders: BTreeSet::new(),
            artwork_cache: BTreeMap::new(),
            artwork_cache_order: Vec::new(),
            artwork_request_tracks: BTreeMap::new(),
            artwork_retry_attempts: BTreeMap::new(),
            renderable_artwork_cache: BTreeMap::new(),
            renderable_artwork_cache_order: Vec::new(),
            search_query: String::new(),
            search_results: Vec::new(),
            search_index: Vec::new(),
            search_input_active: false,
            library_views: LibraryViews::default(),
            sort_menu: None,
            filter_editor: None,
            capabilities: Capabilities::default(),
            backend_availability: BackendAvailability::Available,
            backend_status: BackendStatus::Initializing,
            auth_status: AuthStatus::NotConfigured,
            view_status: ViewStatus::Loading,
            library_status: CollectionLoadState::NotStarted,
            playlist_status: CollectionLoadState::NotStarted,
            notification: None,
            help_open: false,
            help_scroll: 0,
            action_menu: None,
            playlist_track_removal_confirmation: None,
            playlist_track_removal_in_flight: false,
            terminal_size: (80, 24),
        }
    }
}

impl AppState {
    /// Prefer an album artwork identity when the current track is known to belong to one.
    #[must_use]
    pub fn artwork_key_for_track(&self, track_id: &crate::domain::TrackId) -> ArtworkKey {
        self.albums
            .iter()
            .find(|album| album.tracks.iter().any(|track| track.id == *track_id))
            .map(|album| ArtworkKey::Album(album.id.clone()))
            .unwrap_or_else(|| ArtworkKey::Track(track_id.clone()))
    }

    #[must_use]
    pub fn visible_playlist_entries(&self) -> Vec<VisiblePlaylistEntry> {
        if self.playlist_hierarchy.roots.is_empty() && !self.playlists.is_empty() {
            PlaylistHierarchy::from_playlists(&self.playlists)
                .visible_entries(&self.expanded_playlist_folders)
        } else {
            self.playlist_hierarchy
                .visible_entries(&self.expanded_playlist_folders)
        }
    }
}
