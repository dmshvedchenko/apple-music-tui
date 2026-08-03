use crate::{
    auth::AuthStatus,
    backend::capabilities::Capabilities,
    domain::{
        Album, AlbumId, Artist, ArtistId, BackendAvailability, CollectionLoadState,
        PlaybackSnapshot, Playlist, PlaylistId, QueueItem, Station, Track,
    },
};

pub const QUEUE_PANE_MIN_WIDTH: u16 = 108;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Screen {
    ListenNow,
    Browse,
    Radio,
    RecentlyAdded,
    Artists,
    Albums,
    Songs,
    MadeForYou,
    Search,
    Playlists,
}

impl Screen {
    pub const ALL: [Self; 10] = [
        Self::ListenNow,
        Self::Browse,
        Self::Radio,
        Self::RecentlyAdded,
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
    ArtistDetail { artist_id: ArtistId },
    AlbumDetail { album_id: AlbumId },
    PlaylistDetail { playlist_id: PlaylistId },
}

impl Route {
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Section(screen) => screen.label(),
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalSearchIndexEntry {
    pub result: LocalSearchResult,
    pub normalized_text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppState {
    pub should_quit: bool,
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
    pub stations: Vec<Station>,
    pub playlists: Vec<Playlist>,
    pub search_query: String,
    pub search_results: Vec<LocalSearchResult>,
    pub search_index: Vec<LocalSearchIndexEntry>,
    pub search_input_active: bool,
    pub capabilities: Capabilities,
    pub backend_availability: BackendAvailability,
    pub backend_status: BackendStatus,
    pub auth_status: AuthStatus,
    pub view_status: ViewStatus,
    pub library_status: CollectionLoadState,
    pub playlist_status: CollectionLoadState,
    pub notification: Option<String>,
    pub help_open: bool,
    pub terminal_size: (u16, u16),
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            should_quit: false,
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
            stations: Vec::new(),
            playlists: Vec::new(),
            search_query: String::new(),
            search_results: Vec::new(),
            search_index: Vec::new(),
            search_input_active: false,
            capabilities: Capabilities::default(),
            backend_availability: BackendAvailability::Available,
            backend_status: BackendStatus::Initializing,
            auth_status: AuthStatus::NotConfigured,
            view_status: ViewStatus::Loading,
            library_status: CollectionLoadState::NotStarted,
            playlist_status: CollectionLoadState::NotStarted,
            notification: None,
            help_open: false,
            terminal_size: (80, 24),
        }
    }
}
