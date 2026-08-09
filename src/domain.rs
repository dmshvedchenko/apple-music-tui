use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    hash::{Hash, Hasher},
    time::Duration,
};

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TrackId(String);

impl TrackId {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TrackId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct MusicAppPersistentId(String);

impl MusicAppPersistentId {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn to_track_id(&self) -> TrackId {
        TrackId::new(format!("musicapp:persistent:{}", self.0))
    }
}

impl fmt::Display for MusicAppPersistentId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct MusicAppDatabaseId(String);

impl MusicAppDatabaseId {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn to_track_id(&self) -> TrackId {
        TrackId::new(format!("musicapp:database:{}", self.0))
    }
}

impl fmt::Display for MusicAppDatabaseId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PlaylistId(String);

impl PlaylistId {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PlaylistId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AlbumId(String);

impl AlbumId {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

impl fmt::Display for AlbumId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ArtistId(String);

impl ArtistId {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ArtworkKey {
    Album(AlbumId),
    Track(TrackId),
    Playlist(PlaylistId),
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ArtworkMediaType {
    Jpeg,
    Png,
    Gif,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Artwork {
    pub media_type: ArtworkMediaType,
    pub bytes: Vec<u8>,
}

impl Artwork {
    /// Identifies a source asset while an asynchronous renderable conversion is in flight.
    #[must_use]
    pub fn fingerprint(&self) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.media_type.hash(&mut hasher);
        self.bytes.hash(&mut hasher);
        hasher.finish()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ArtworkResult {
    Ready(Artwork),
    Missing,
    Transient(String),
    TooLarge { encoded_bytes: usize },
    Invalid(String),
}

impl fmt::Display for ArtistId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct StationId(String);

impl StationId {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

impl fmt::Display for StationId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct QueueEntryId(String);

impl QueueEntryId {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

impl fmt::Display for QueueEntryId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DataOrigin {
    #[default]
    Demo,
    LocalMusicApp,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TrackMetadata {
    pub origin: DataOrigin,
    pub album_artist: Option<String>,
    pub composer: Option<String>,
    pub genre: Option<String>,
    pub year: Option<u16>,
    pub track_number: Option<u16>,
    pub disc_number: Option<u16>,
    pub play_count: Option<u64>,
    pub skip_count: Option<u64>,
    pub date_added: Option<String>,
    pub last_played_date: Option<String>,
    pub last_skipped_date: Option<String>,
    pub modification_date: Option<String>,
    pub release_date: Option<String>,
    pub rating: Option<u8>,
    pub cloud_status: Option<String>,
    pub media_kind: Option<String>,
    pub enabled: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Track {
    pub id: TrackId,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: Duration,
    pub is_favorite: bool,
    pub metadata: TrackMetadata,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecentlyPlayedEntry {
    pub track_id: TrackId,
    pub title: String,
    pub artist: String,
    pub play_count: Option<u64>,
    pub played_at: String,
}

impl Track {
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        title: impl Into<String>,
        artist: impl Into<String>,
        album: impl Into<String>,
        duration: Duration,
    ) -> Self {
        Self {
            id: TrackId::new(id),
            title: title.into(),
            artist: artist.into(),
            album: album.into(),
            duration,
            is_favorite: false,
            metadata: TrackMetadata {
                enabled: true,
                ..TrackMetadata::default()
            },
        }
    }
}

#[must_use]
pub fn search_track_ids(tracks: &[Track], query: &str, limit: usize) -> Vec<TrackId> {
    let terms = normalize_search_text(query)
        .split_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if terms.is_empty() || limit == 0 {
        return Vec::new();
    }
    tracks
        .iter()
        .filter(|track| {
            let metadata = format!(
                "{} {} {} {} {} {}",
                track.title,
                track.artist,
                track.album,
                track.metadata.album_artist.as_deref().unwrap_or_default(),
                track.metadata.composer.as_deref().unwrap_or_default(),
                track.metadata.genre.as_deref().unwrap_or_default()
            );
            let haystack = normalize_search_text(&metadata);
            terms.iter().all(|term| haystack.contains(term))
        })
        .take(limit)
        .map(|track| track.id.clone())
        .collect()
}

fn normalize_search_text(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PlaylistKind {
    #[default]
    User,
    Smart,
    Folder,
    Subscription,
    Library,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Playlist {
    pub id: PlaylistId,
    pub name: String,
    pub description: Option<String>,
    pub tracks: Vec<Track>,
    pub track_count: usize,
    pub contents_state: PlaylistLoadState,
    pub kind: PlaylistKind,
    pub parent_id: Option<PlaylistId>,
    pub origin: DataOrigin,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum PlaylistLoadState {
    #[default]
    NotLoaded,
    Loading {
        loaded: usize,
        total: Option<usize>,
    },
    PartiallyLoaded {
        loaded: usize,
        total: usize,
    },
    Loaded {
        total: usize,
    },
    Empty,
    Error(String),
}

impl PlaylistLoadState {
    #[must_use]
    pub const fn should_request_load(&self) -> bool {
        matches!(self, Self::NotLoaded | Self::Error(_))
    }

    #[must_use]
    pub const fn is_complete(&self) -> bool {
        matches!(self, Self::Loaded { .. } | Self::Empty)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlaylistTreeNode {
    pub playlist_id: PlaylistId,
    pub children: Vec<Self>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PlaylistHierarchy {
    pub roots: Vec<PlaylistTreeNode>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VisiblePlaylistEntry {
    pub playlist_id: PlaylistId,
    pub depth: usize,
    pub has_children: bool,
}

impl PlaylistHierarchy {
    #[must_use]
    pub fn from_playlists(playlists: &[Playlist]) -> Self {
        let known_ids = playlists
            .iter()
            .map(|playlist| playlist.id.clone())
            .collect::<BTreeSet<_>>();
        let mut root_ids = Vec::new();
        let mut children_by_parent = BTreeMap::<PlaylistId, Vec<PlaylistId>>::new();
        for playlist in playlists {
            match playlist.parent_id.as_ref() {
                Some(parent_id) if parent_id != &playlist.id && known_ids.contains(parent_id) => {
                    children_by_parent
                        .entry(parent_id.clone())
                        .or_default()
                        .push(playlist.id.clone());
                }
                _ => root_ids.push(playlist.id.clone()),
            }
        }

        let mut emitted = BTreeSet::new();
        let mut roots = root_ids
            .iter()
            .filter_map(|id| {
                build_playlist_node(id, &children_by_parent, &mut BTreeSet::new(), &mut emitted)
            })
            .collect::<Vec<_>>();
        for playlist in playlists {
            if !emitted.contains(&playlist.id)
                && let Some(node) = build_playlist_node(
                    &playlist.id,
                    &children_by_parent,
                    &mut BTreeSet::new(),
                    &mut emitted,
                )
            {
                roots.push(node);
            }
        }
        Self { roots }
    }

    #[must_use]
    pub fn visible_entries(
        &self,
        expanded_folders: &BTreeSet<PlaylistId>,
    ) -> Vec<VisiblePlaylistEntry> {
        let mut entries = Vec::new();
        for root in &self.roots {
            append_visible_playlist_entries(root, 0, expanded_folders, &mut entries);
        }
        entries
    }
}

fn build_playlist_node(
    id: &PlaylistId,
    children_by_parent: &BTreeMap<PlaylistId, Vec<PlaylistId>>,
    visiting: &mut BTreeSet<PlaylistId>,
    emitted: &mut BTreeSet<PlaylistId>,
) -> Option<PlaylistTreeNode> {
    if emitted.contains(id) || !visiting.insert(id.clone()) {
        return None;
    }
    let children = children_by_parent
        .get(id)
        .into_iter()
        .flatten()
        .filter_map(|child_id| build_playlist_node(child_id, children_by_parent, visiting, emitted))
        .collect();
    visiting.remove(id);
    emitted.insert(id.clone());
    Some(PlaylistTreeNode {
        playlist_id: id.clone(),
        children,
    })
}

fn append_visible_playlist_entries(
    node: &PlaylistTreeNode,
    depth: usize,
    expanded_folders: &BTreeSet<PlaylistId>,
    entries: &mut Vec<VisiblePlaylistEntry>,
) {
    entries.push(VisiblePlaylistEntry {
        playlist_id: node.playlist_id.clone(),
        depth,
        has_children: !node.children.is_empty(),
    });
    if expanded_folders.contains(&node.playlist_id) {
        for child in &node.children {
            append_visible_playlist_entries(child, depth + 1, expanded_folders, entries);
        }
    }
}

impl Playlist {
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        description: Option<String>,
        tracks: Vec<Track>,
    ) -> Self {
        let track_count = tracks.len();
        Self {
            id: PlaylistId::new(id),
            name: name.into(),
            description,
            tracks,
            track_count,
            contents_state: if track_count == 0 {
                PlaylistLoadState::Empty
            } else {
                PlaylistLoadState::Loaded { total: track_count }
            },
            kind: PlaylistKind::User,
            parent_id: None,
            origin: DataOrigin::Demo,
        }
    }

    #[must_use]
    pub fn unloaded(
        id: impl Into<String>,
        name: impl Into<String>,
        description: Option<String>,
        kind: PlaylistKind,
        parent_id: Option<PlaylistId>,
    ) -> Self {
        Self {
            id: PlaylistId::new(id),
            name: name.into(),
            description,
            tracks: Vec::new(),
            track_count: 0,
            contents_state: PlaylistLoadState::NotLoaded,
            kind,
            parent_id,
            origin: DataOrigin::LocalMusicApp,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Album {
    pub id: AlbumId,
    pub title: String,
    pub artist: String,
    pub year: u16,
    pub added_date: String,
    pub tracks: Vec<Track>,
}

impl Album {
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        title: impl Into<String>,
        artist: impl Into<String>,
        year: u16,
        added_date: impl Into<String>,
        tracks: Vec<Track>,
    ) -> Self {
        Self {
            id: AlbumId::new(id),
            title: title.into(),
            artist: artist.into(),
            year,
            added_date: added_date.into(),
            tracks,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Artist {
    pub id: ArtistId,
    pub name: String,
    pub album_ids: Vec<AlbumId>,
    pub top_track_ids: Vec<TrackId>,
}

impl Artist {
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        album_ids: Vec<AlbumId>,
        top_track_ids: Vec<TrackId>,
    ) -> Self {
        Self {
            id: ArtistId::new(id),
            name: name.into(),
            album_ids,
            top_track_ids,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Station {
    pub id: StationId,
    pub name: String,
    pub description: String,
}

impl Station {
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            id: StationId::new(id),
            name: name.into(),
            description: description.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PlaybackStatus {
    #[default]
    Stopped,
    Paused,
    Playing,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RepeatMode {
    #[default]
    Off,
    All,
    One,
}

impl RepeatMode {
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Off => Self::All,
            Self::All => Self::One,
            Self::One => Self::Off,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlaybackSnapshot {
    pub status: PlaybackStatus,
    pub current_entry_id: Option<QueueEntryId>,
    pub current_track: Option<Track>,
    pub position: Duration,
    pub volume: u8,
    pub muted: bool,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub context: PlaybackContext,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum PlaybackContext {
    #[default]
    NoContext,
    Playlist {
        playlist_id: PlaylistId,
        ordered_track_ids: Vec<TrackId>,
        current_index: usize,
        complete: bool,
    },
    Album {
        album_id: AlbumId,
        ordered_track_ids: Vec<TrackId>,
        current_index: usize,
    },
}

impl Default for PlaybackSnapshot {
    fn default() -> Self {
        Self {
            status: PlaybackStatus::Stopped,
            current_entry_id: None,
            current_track: None,
            position: Duration::ZERO,
            volume: 50,
            muted: false,
            shuffle: false,
            repeat: RepeatMode::Off,
            context: PlaybackContext::NoContext,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueueItem {
    pub id: QueueEntryId,
    pub track: Track,
}

impl QueueItem {
    #[must_use]
    pub fn new(id: impl Into<String>, track: Track) -> Self {
        Self {
            id: QueueEntryId::new(id),
            track,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum BackendAvailability {
    #[default]
    Available,
    NotRunning,
    Unavailable,
    PermissionDenied,
    Error(String),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum CollectionLoadState {
    #[default]
    NotStarted,
    Cached {
        total: usize,
    },
    Refreshing {
        loaded: usize,
        total: usize,
    },
    Loading {
        loaded: usize,
        total: usize,
    },
    Loaded {
        total: usize,
    },
    Error(String),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BackendSnapshot {
    pub availability: BackendAvailability,
    pub playback: PlaybackSnapshot,
    pub queue: Vec<QueueItem>,
    pub library: Vec<Track>,
    pub artists: Vec<Artist>,
    pub albums: Vec<Album>,
    pub recently_added: Vec<Album>,
    pub recently_played: Vec<RecentlyPlayedEntry>,
    pub stations: Vec<Station>,
    pub playlists: Vec<Playlist>,
    pub library_status: CollectionLoadState,
    pub playlist_status: CollectionLoadState,
}
