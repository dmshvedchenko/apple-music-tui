use std::{
    env,
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::domain::{
    DataOrigin, Playlist, PlaylistId, PlaylistKind, PlaylistLoadState, Track, TrackId,
    TrackMetadata,
};

const SCHEMA_VERSION: u32 = 1;
const CACHE_DIRECTORY: &str = "apple-music-tui";
const CACHE_FILENAME: &str = "musicapp-library-v1.json";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct CachedLibrary {
    pub library: Vec<Track>,
    pub playlists: Vec<Playlist>,
}

#[derive(Deserialize, Serialize)]
struct CacheFile {
    schema_version: u32,
    library: Vec<CachedTrack>,
    playlists: Vec<CachedPlaylist>,
}

#[derive(Deserialize, Serialize)]
struct CachedTrack {
    id: String,
    title: String,
    artist: String,
    album: String,
    duration_ms: u64,
    is_favorite: bool,
    metadata: CachedTrackMetadata,
}

#[derive(Deserialize, Serialize)]
struct CachedTrackMetadata {
    album_artist: Option<String>,
    composer: Option<String>,
    genre: Option<String>,
    year: Option<u16>,
    track_number: Option<u16>,
    disc_number: Option<u16>,
    play_count: Option<u64>,
    skip_count: Option<u64>,
    date_added: Option<String>,
    last_played_date: Option<String>,
    last_skipped_date: Option<String>,
    modification_date: Option<String>,
    release_date: Option<String>,
    rating: Option<u8>,
    cloud_status: Option<String>,
    media_kind: Option<String>,
    enabled: bool,
}

#[derive(Deserialize, Serialize)]
struct CachedPlaylist {
    id: String,
    name: String,
    description: Option<String>,
    track_count: usize,
    kind: CachedPlaylistKind,
    parent_id: Option<String>,
}

#[derive(Deserialize, Serialize)]
enum CachedPlaylistKind {
    User,
    Smart,
    Folder,
    Subscription,
    Library,
    Unknown,
}

pub(super) fn default_path() -> Option<PathBuf> {
    let root = env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(|home| {
                    let mut path = PathBuf::from(home);
                    if cfg!(target_os = "macos") {
                        path.push("Library/Caches");
                    } else {
                        path.push(".cache");
                    }
                    path
                })
        })?;
    Some(root.join(CACHE_DIRECTORY).join(CACHE_FILENAME))
}

pub(super) fn load(path: &Path) -> Option<CachedLibrary> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return None,
        Err(error) => {
            tracing::debug!(path = %path.display(), %error, "could not read local Music.app cache");
            return None;
        }
    };
    let file = match serde_json::from_slice::<CacheFile>(&bytes) {
        Ok(file) if file.schema_version == SCHEMA_VERSION => file,
        Ok(file) => {
            tracing::debug!(
                found = file.schema_version,
                expected = SCHEMA_VERSION,
                "ignored incompatible local Music.app cache"
            );
            return None;
        }
        Err(error) => {
            tracing::debug!(path = %path.display(), %error, "ignored corrupt local Music.app cache");
            return None;
        }
    };
    Some(CachedLibrary {
        library: file.library.into_iter().map(Into::into).collect(),
        playlists: file.playlists.into_iter().map(Into::into).collect(),
    })
}

pub(super) fn inspect(path: Option<PathBuf>) -> super::LocalCacheStatus {
    let Some(path) = path else {
        return super::LocalCacheStatus {
            path: None,
            schema_version: None,
            tracks: None,
            playlists: None,
            last_updated_unix_seconds: None,
            readable: false,
        };
    };
    let metadata = fs::metadata(&path).ok();
    let last_updated_unix_seconds = metadata.and_then(|metadata| {
        metadata
            .modified()
            .ok()?
            .duration_since(UNIX_EPOCH)
            .ok()
            .map(|duration| duration.as_secs())
    });
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(_) => {
            return super::LocalCacheStatus {
                path: Some(path),
                schema_version: None,
                tracks: None,
                playlists: None,
                last_updated_unix_seconds,
                readable: false,
            };
        }
    };
    let parsed = serde_json::from_slice::<CacheFile>(&bytes).ok();
    super::LocalCacheStatus {
        path: Some(path),
        schema_version: parsed.as_ref().map(|file| file.schema_version),
        tracks: parsed.as_ref().map(|file| file.library.len()),
        playlists: parsed.as_ref().map(|file| file.playlists.len()),
        last_updated_unix_seconds,
        readable: parsed.is_some_and(|file| file.schema_version == SCHEMA_VERSION),
    }
}

pub(super) fn clear(path: Option<PathBuf>) -> io::Result<super::LocalCacheClearResult> {
    let Some(path) = path else {
        return Ok(super::LocalCacheClearResult::Unavailable);
    };
    match fs::remove_file(path) {
        Ok(()) => Ok(super::LocalCacheClearResult::Removed),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Ok(super::LocalCacheClearResult::NotFound)
        }
        Err(error) => Err(error),
    }
}

pub(super) fn store(path: &Path, library: &[Track], playlists: &[Playlist]) -> io::Result<()> {
    let file = CacheFile {
        schema_version: SCHEMA_VERSION,
        library: library.iter().cloned().map(Into::into).collect(),
        playlists: playlists.iter().cloned().map(Into::into).collect(),
    };
    let bytes = serde_json::to_vec(&file).map_err(io::Error::other)?;
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("cache path has no parent"))?;
    fs::create_dir_all(parent)?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = parent.join(format!(
        ".{CACHE_FILENAME}.{}.{}.tmp",
        std::process::id(),
        nonce
    ));
    let result = (|| {
        let mut file = File::create(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

impl From<Track> for CachedTrack {
    fn from(track: Track) -> Self {
        Self {
            id: track.id.to_string(),
            title: track.title,
            artist: track.artist,
            album: track.album,
            duration_ms: u64::try_from(track.duration.as_millis()).unwrap_or(u64::MAX),
            is_favorite: track.is_favorite,
            metadata: track.metadata.into(),
        }
    }
}

impl From<CachedTrack> for Track {
    fn from(track: CachedTrack) -> Self {
        Self {
            id: TrackId::new(track.id),
            title: track.title,
            artist: track.artist,
            album: track.album,
            duration: std::time::Duration::from_millis(track.duration_ms),
            is_favorite: track.is_favorite,
            metadata: track.metadata.into(),
        }
    }
}

impl From<TrackMetadata> for CachedTrackMetadata {
    fn from(metadata: TrackMetadata) -> Self {
        Self {
            album_artist: metadata.album_artist,
            composer: metadata.composer,
            genre: metadata.genre,
            year: metadata.year,
            track_number: metadata.track_number,
            disc_number: metadata.disc_number,
            play_count: metadata.play_count,
            skip_count: metadata.skip_count,
            date_added: metadata.date_added,
            last_played_date: metadata.last_played_date,
            last_skipped_date: metadata.last_skipped_date,
            modification_date: metadata.modification_date,
            release_date: metadata.release_date,
            rating: metadata.rating,
            cloud_status: metadata.cloud_status,
            media_kind: metadata.media_kind,
            enabled: metadata.enabled,
        }
    }
}

impl From<CachedTrackMetadata> for TrackMetadata {
    fn from(metadata: CachedTrackMetadata) -> Self {
        Self {
            origin: DataOrigin::LocalMusicApp,
            album_artist: metadata.album_artist,
            composer: metadata.composer,
            genre: metadata.genre,
            year: metadata.year,
            track_number: metadata.track_number,
            disc_number: metadata.disc_number,
            play_count: metadata.play_count,
            skip_count: metadata.skip_count,
            date_added: metadata.date_added,
            last_played_date: metadata.last_played_date,
            last_skipped_date: metadata.last_skipped_date,
            modification_date: metadata.modification_date,
            release_date: metadata.release_date,
            rating: metadata.rating,
            cloud_status: metadata.cloud_status,
            media_kind: metadata.media_kind,
            enabled: metadata.enabled,
        }
    }
}

impl From<Playlist> for CachedPlaylist {
    fn from(playlist: Playlist) -> Self {
        Self {
            id: playlist.id.to_string(),
            name: playlist.name,
            description: playlist.description,
            track_count: playlist.track_count,
            kind: playlist.kind.into(),
            parent_id: playlist.parent_id.map(|id| id.to_string()),
        }
    }
}

impl From<CachedPlaylist> for Playlist {
    fn from(playlist: CachedPlaylist) -> Self {
        Playlist {
            id: PlaylistId::new(playlist.id),
            name: playlist.name,
            description: playlist.description,
            tracks: Vec::new(),
            track_count: playlist.track_count,
            contents_state: PlaylistLoadState::NotLoaded,
            kind: playlist.kind.into(),
            parent_id: playlist.parent_id.map(PlaylistId::new),
            origin: DataOrigin::LocalMusicApp,
        }
    }
}

impl From<PlaylistKind> for CachedPlaylistKind {
    fn from(kind: PlaylistKind) -> Self {
        match kind {
            PlaylistKind::User => Self::User,
            PlaylistKind::Smart => Self::Smart,
            PlaylistKind::Folder => Self::Folder,
            PlaylistKind::Subscription => Self::Subscription,
            PlaylistKind::Library => Self::Library,
            PlaylistKind::Unknown => Self::Unknown,
        }
    }
}
impl From<CachedPlaylistKind> for PlaylistKind {
    fn from(kind: CachedPlaylistKind) -> Self {
        match kind {
            CachedPlaylistKind::User => Self::User,
            CachedPlaylistKind::Smart => Self::Smart,
            CachedPlaylistKind::Folder => Self::Folder,
            CachedPlaylistKind::Subscription => Self::Subscription,
            CachedPlaylistKind::Library => Self::Library,
            CachedPlaylistKind::Unknown => Self::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn path(name: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!(
                "apple-music-tui-{name}-{}",
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            ))
            .join("cache.json")
    }
    fn sample() -> CachedLibrary {
        let mut track = Track::new(
            "musicapp:persistent:T1",
            "Song",
            "Artist",
            "Album",
            std::time::Duration::from_secs(60),
        );
        track.metadata.origin = DataOrigin::LocalMusicApp;
        CachedLibrary {
            library: vec![track],
            playlists: vec![Playlist::unloaded(
                "musicapp:playlist:persistent:P1",
                "Playlist",
                None,
                PlaylistKind::User,
                None,
            )],
        }
    }

    #[test]
    fn round_trips_atomically_without_runtime_playlist_contents() {
        let path = path("round-trip");
        let sample = sample();
        store(&path, &sample.library, &sample.playlists).expect("store cache");
        let loaded = load(&path).expect("load cache");
        assert_eq!(loaded.library, sample.library);
        assert_eq!(loaded.playlists[0].tracks.len(), 0);
        assert_eq!(
            loaded.playlists[0].contents_state,
            PlaylistLoadState::NotLoaded
        );
        let _ = fs::remove_dir_all(path.parent().expect("parent"));
    }

    #[test]
    fn ignores_corrupt_and_unknown_schema_cache() {
        let corrupt = path("corrupt");
        fs::create_dir_all(corrupt.parent().expect("parent")).expect("directory");
        fs::write(&corrupt, b"not json").expect("corrupt file");
        assert!(load(&corrupt).is_none());
        let mismatch = path("schema");
        fs::create_dir_all(mismatch.parent().expect("parent")).expect("directory");
        fs::write(
            &mismatch,
            br#"{"schema_version":99,"library":[],"playlists":[]}"#,
        )
        .expect("schema file");
        assert!(load(&mismatch).is_none());
        let _ = fs::remove_dir_all(corrupt.parent().expect("parent"));
        let _ = fs::remove_dir_all(mismatch.parent().expect("parent"));
    }

    #[test]
    fn failed_write_leaves_the_previous_valid_cache_readable() {
        let path = path("failed-write");
        let sample = sample();
        store(&path, &sample.library, &sample.playlists).expect("initial cache");
        let blocked_parent = path.parent().expect("parent").join("blocked");
        fs::write(&blocked_parent, b"not a directory").expect("blocking file");
        let failed_target = blocked_parent.join("cache.json");
        assert!(store(&failed_target, &sample.library, &sample.playlists).is_err());
        assert_eq!(load(&path).expect("previous cache").library, sample.library);
        let _ = fs::remove_dir_all(path.parent().expect("parent"));
    }

    #[test]
    fn inspection_and_clear_handle_existing_and_missing_cache() {
        let path = path("inspection");
        let sample = sample();
        store(&path, &sample.library, &sample.playlists).expect("store cache");
        let inspection = inspect(Some(path.clone()));
        assert!(inspection.readable);
        assert_eq!(inspection.schema_version, Some(SCHEMA_VERSION));
        assert_eq!(inspection.tracks, Some(1));
        assert_eq!(inspection.playlists, Some(1));
        assert_eq!(
            clear(Some(path.clone())).expect("clear cache"),
            super::super::LocalCacheClearResult::Removed
        );
        assert_eq!(
            clear(Some(path.clone())).expect("clear missing cache"),
            super::super::LocalCacheClearResult::NotFound
        );
        assert!(!inspect(Some(path.clone())).readable);
        let _ = fs::remove_dir_all(path.parent().expect("parent"));
    }
}
