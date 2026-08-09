use std::{
    collections::{BTreeMap, BTreeSet, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    time::{Duration, Instant},
};

use crate::domain::{
    Album, AlbumId, Artist, DataOrigin, MusicAppDatabaseId, MusicAppPersistentId, Playlist,
    PlaylistId, PlaylistKind, RecentlyPlayedEntry, Track, TrackId, TrackMetadata,
};

use super::parser::{RawPlaylist, RawTrack};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DerivedLibrary {
    pub artists: Vec<Artist>,
    pub albums: Vec<Album>,
    pub recently_added: Vec<Album>,
    pub recently_played: Vec<RecentlyPlayedEntry>,
}

pub fn raw_track_to_domain(raw: RawTrack) -> Track {
    let duration = duration_from_seconds(raw.duration.unwrap_or_default());
    let title = nonempty_or(raw.name, "Unknown Track");
    let artist = nonempty_or(raw.artist, "Unknown Artist");
    let album = nonempty_or(raw.album, "Unknown Album");
    let id = raw
        .persistent_id
        .as_deref()
        .and_then(normalize_identifier)
        .map(|id| MusicAppPersistentId::new(id).to_track_id())
        .or_else(|| {
            raw.database_id
                .as_deref()
                .and_then(normalize_identifier)
                .map(|id| MusicAppDatabaseId::new(id).to_track_id())
        })
        .unwrap_or_else(|| ephemeral_track_id(&title, &artist, &album, duration));

    Track {
        id,
        title,
        artist,
        album,
        duration,
        is_favorite: raw.favorited.unwrap_or(false),
        metadata: TrackMetadata {
            origin: DataOrigin::LocalMusicApp,
            album_artist: nonempty(raw.album_artist),
            composer: nonempty(raw.composer),
            genre: nonempty(raw.genre),
            year: raw.year.filter(|year| *year > 0),
            track_number: raw.track_number.filter(|number| *number > 0),
            disc_number: raw.disc_number.filter(|number| *number > 0),
            play_count: raw.played_count,
            skip_count: raw.skipped_count,
            date_added: nonempty(raw.date_added),
            last_played_date: nonempty(raw.played_date),
            last_skipped_date: nonempty(raw.skipped_date),
            modification_date: nonempty(raw.modification_date),
            release_date: nonempty(raw.release_date),
            rating: raw.rating.filter(|rating| *rating <= 100),
            cloud_status: nonempty(raw.cloud_status),
            media_kind: nonempty(raw.media_kind),
            enabled: raw.enabled.unwrap_or(true),
        },
    }
}

pub fn raw_playlist_to_domain(raw: RawPlaylist) -> Playlist {
    let kind = match raw.kind.as_deref().map(normalized_key).as_deref() {
        Some("folderplaylist") => PlaylistKind::Folder,
        Some("subscriptionplaylist") => PlaylistKind::Subscription,
        Some("libraryplaylist") => PlaylistKind::Library,
        Some("userplaylist") if raw.smart => PlaylistKind::Smart,
        Some("userplaylist") => PlaylistKind::User,
        _ => PlaylistKind::Unknown,
    };
    Playlist::unloaded(
        playlist_id(&raw.persistent_id).to_string(),
        raw.name,
        nonempty(raw.description),
        kind,
        raw.parent_persistent_id.map(|id| playlist_id(&id)),
    )
}

#[must_use]
pub fn playlist_id(persistent_id: &str) -> PlaylistId {
    PlaylistId::new(format!("musicapp:playlist:persistent:{persistent_id}"))
}

#[must_use]
pub fn persistent_track_selector(id: &TrackId) -> Option<(&'static str, &str)> {
    id.as_str()
        .strip_prefix("musicapp:persistent:")
        .map(|value| ("persistentID", value))
        .or_else(|| {
            id.as_str()
                .strip_prefix("musicapp:database:")
                .map(|value| ("databaseID", value))
        })
}

#[must_use]
pub fn persistent_playlist_selector(id: &PlaylistId) -> Option<&str> {
    id.as_str().strip_prefix("musicapp:playlist:persistent:")
}

#[must_use]
pub fn derive_library(tracks: &[Track]) -> DerivedLibrary {
    let albums_started = Instant::now();
    let mut grouped_albums: BTreeMap<String, Vec<Track>> = BTreeMap::new();
    for track in tracks {
        let album_artist = track
            .metadata
            .album_artist
            .as_deref()
            .unwrap_or(&track.artist);
        let key = format!(
            "{}\u{1f}{}\u{1f}{}",
            normalized_key(album_artist),
            normalized_key(&track.album),
            track.metadata.year.unwrap_or_default()
        );
        grouped_albums.entry(key).or_default().push(track.clone());
    }

    let mut albums = grouped_albums
        .into_iter()
        .map(|(key, mut tracks)| {
            tracks.sort_by(|left, right| {
                (
                    left.metadata.disc_number.unwrap_or(1),
                    left.metadata.track_number.unwrap_or(u16::MAX),
                    &left.title,
                    &left.id,
                )
                    .cmp(&(
                        right.metadata.disc_number.unwrap_or(1),
                        right.metadata.track_number.unwrap_or(u16::MAX),
                        &right.title,
                        &right.id,
                    ))
            });
            let first = &tracks[0];
            let artist = first
                .metadata
                .album_artist
                .clone()
                .unwrap_or_else(|| first.artist.clone());
            let title = first.album.clone();
            let year = tracks
                .iter()
                .filter_map(|track| track.metadata.year)
                .min()
                .unwrap_or_default();
            let added_date = tracks
                .iter()
                .filter_map(|track| track.metadata.date_added.as_deref())
                .max()
                .unwrap_or_default()
                .to_owned();
            Album::new(
                stable_id("musicapp:album", &key),
                title,
                artist,
                year,
                added_date,
                tracks,
            )
        })
        .collect::<Vec<_>>();
    albums.sort_by(|left, right| {
        normalized_key(&left.artist)
            .cmp(&normalized_key(&right.artist))
            .then_with(|| normalized_key(&left.title).cmp(&normalized_key(&right.title)))
    });
    let albums_elapsed = albums_started.elapsed();

    let artists_started = Instant::now();
    let mut albums_by_artist: BTreeMap<String, BTreeSet<AlbumId>> = BTreeMap::new();
    let mut tracks_by_artist: BTreeMap<String, Vec<TrackId>> = BTreeMap::new();
    let mut display_names: BTreeMap<String, String> = BTreeMap::new();
    for album in &albums {
        let key = normalized_key(&album.artist);
        display_names
            .entry(key.clone())
            .or_insert_with(|| album.artist.clone());
        albums_by_artist
            .entry(key)
            .or_default()
            .insert(album.id.clone());
    }
    for track in tracks {
        let name = track
            .metadata
            .album_artist
            .as_deref()
            .unwrap_or(&track.artist);
        let key = normalized_key(name);
        display_names
            .entry(key.clone())
            .or_insert_with(|| name.to_owned());
        tracks_by_artist
            .entry(key)
            .or_default()
            .push(track.id.clone());
    }
    let artists = display_names
        .into_iter()
        .map(|(key, name)| {
            let album_ids = albums_by_artist
                .remove(&key)
                .unwrap_or_default()
                .into_iter()
                .collect();
            let top_track_ids = tracks_by_artist.remove(&key).unwrap_or_default();
            Artist::new(
                stable_id("musicapp:artist", &key),
                name,
                album_ids,
                top_track_ids,
            )
        })
        .collect();
    let artists_elapsed = artists_started.elapsed();

    let recent_started = Instant::now();
    let mut recently_added = albums.clone();
    recently_added.sort_by(|left, right| {
        right
            .added_date
            .cmp(&left.added_date)
            .then_with(|| left.title.cmp(&right.title))
    });

    let mut recent_by_track = BTreeMap::<TrackId, RecentlyPlayedEntry>::new();
    for track in tracks {
        let Some(played_at) = track.metadata.last_played_date.clone() else {
            continue;
        };
        let candidate = RecentlyPlayedEntry {
            track_id: track.id.clone(),
            title: track.title.clone(),
            artist: track.artist.clone(),
            play_count: track.metadata.play_count,
            played_at,
        };
        let should_replace = recent_by_track
            .get(&track.id)
            .is_none_or(|existing| candidate.played_at > existing.played_at);
        if should_replace {
            recent_by_track.insert(track.id.clone(), candidate);
        }
    }
    let mut recently_played = recent_by_track.into_values().collect::<Vec<_>>();
    recently_played.sort_by(|left, right| {
        right
            .played_at
            .cmp(&left.played_at)
            .then_with(|| left.title.cmp(&right.title))
            .then_with(|| left.track_id.cmp(&right.track_id))
    });
    let recent_elapsed = recent_started.elapsed();
    tracing::debug!(
        tracks = tracks.len(),
        albums_ms = albums_elapsed.as_secs_f64() * 1_000.0,
        artists_ms = artists_elapsed.as_secs_f64() * 1_000.0,
        recent_views_ms = recent_elapsed.as_secs_f64() * 1_000.0,
        "local library grouping timing"
    );

    DerivedLibrary {
        artists,
        albums,
        recently_added,
        recently_played,
    }
}

#[must_use]
pub fn search_track_ids(tracks: &[Track], query: &str, limit: usize) -> Vec<TrackId> {
    crate::domain::search_track_ids(tracks, query, limit)
}

fn duration_from_seconds(seconds: f64) -> Duration {
    if seconds.is_finite() && seconds > 0.0 {
        Duration::from_secs_f64(seconds)
    } else {
        Duration::ZERO
    }
}

fn nonempty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
}

fn nonempty_or(value: Option<String>, fallback: &str) -> String {
    nonempty(value).unwrap_or_else(|| fallback.to_owned())
}

pub(super) fn normalize_identifier(value: &str) -> Option<&str> {
    let value = value.trim();
    if value.is_empty()
        || matches!(
            value.to_ascii_lowercase().as_str(),
            "null" | "undefined" | "missing value" | "0" | "-1"
        )
    {
        None
    } else {
        Some(value)
    }
}

fn normalized_key(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn stable_id(namespace: &str, value: &str) -> String {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    format!("{namespace}:{:016x}", hasher.finish())
}

fn ephemeral_track_id(name: &str, artist: &str, album: &str, duration: Duration) -> TrackId {
    let value = format!(
        "{name}\u{1f}{artist}\u{1f}{album}\u{1f}{}",
        duration.as_millis()
    );
    TrackId::new(stable_id("musicapp:ephemeral", &value))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(id: &str, title: &str, artist: &str, album: &str, added: &str) -> RawTrack {
        RawTrack {
            persistent_id: Some(id.to_owned()),
            database_id: None,
            name: Some(title.to_owned()),
            artist: Some(artist.to_owned()),
            album: Some(album.to_owned()),
            album_artist: Some(artist.to_owned()),
            composer: Some("Composer".to_owned()),
            genre: Some("Ambient".to_owned()),
            duration: Some(180.25),
            favorited: Some(true),
            disliked: Some(false),
            year: Some(2025),
            track_number: Some(1),
            disc_number: Some(1),
            played_count: Some(7),
            skipped_count: Some(2),
            date_added: Some(added.to_owned()),
            played_date: None,
            skipped_date: None,
            modification_date: None,
            release_date: None,
            rating: Some(80),
            enabled: Some(true),
            cloud_status: Some("purchased".to_owned()),
            media_kind: Some("sharedTrack".to_owned()),
        }
    }

    #[test]
    fn converts_rich_metadata_and_keeps_stable_music_identifier() {
        let track = raw_track_to_domain(raw("ABC", "Title", "Artist", "Album", "2025-01-01"));
        assert_eq!(track.id.as_str(), "musicapp:persistent:ABC");
        assert_eq!(track.metadata.genre.as_deref(), Some("Ambient"));
        assert_eq!(track.metadata.play_count, Some(7));
        assert_eq!(track.metadata.origin, DataOrigin::LocalMusicApp);
    }

    #[test]
    fn derives_albums_artists_and_recently_added_deterministically() {
        let tracks = vec![
            raw_track_to_domain(raw("1", "One", "Artist", "Older", "2025-01-01")),
            raw_track_to_domain(raw("2", "Two", "Artist", "Newer", "2026-01-01")),
        ];
        let first = derive_library(&tracks);
        let second = derive_library(&tracks);

        assert_eq!(first, second);
        assert_eq!(first.artists.len(), 1);
        assert_eq!(first.albums.len(), 2);
        assert_eq!(first.recently_added[0].title, "Newer");
    }

    #[test]
    fn album_identity_separates_reissues_and_orders_multiple_discs() {
        let mut disc_two = raw("d2t1", "Disc Two", "Artist", "Shared Title", "2025-01-01");
        disc_two.disc_number = Some(2);
        disc_two.track_number = Some(1);
        let mut disc_one_two = raw("d1t2", "Second", "Artist", "Shared Title", "2025-01-01");
        disc_one_two.disc_number = Some(1);
        disc_one_two.track_number = Some(2);
        let mut disc_one_one = raw("d1t1", "First", "Artist", "Shared Title", "2025-01-01");
        disc_one_one.disc_number = Some(1);
        disc_one_one.track_number = Some(1);
        let mut reissue = raw("reissue", "Reissue", "Artist", "Shared Title", "2026-01-01");
        reissue.year = Some(2026);

        let derived = derive_library(
            &[disc_two, disc_one_two, disc_one_one, reissue]
                .into_iter()
                .map(raw_track_to_domain)
                .collect::<Vec<_>>(),
        );

        assert_eq!(derived.albums.len(), 2);
        let original = derived
            .albums
            .iter()
            .find(|album| album.year == 2025)
            .expect("original album");
        assert_eq!(
            original
                .tracks
                .iter()
                .map(|track| track.title.as_str())
                .collect::<Vec<_>>(),
            vec!["First", "Second", "Disc Two"]
        );
    }

    #[test]
    fn recently_played_uses_latest_real_date_and_deduplicates_track_identity() {
        let mut older = raw("same", "Song", "Artist", "Album", "2025-01-01");
        older.played_date = Some("2026-08-01T10:00:00.000Z".to_owned());
        let mut newer = older.clone();
        newer.played_date = Some("2026-08-03T10:00:00.000Z".to_owned());
        let mut second = raw("second", "Second", "Artist", "Album", "2025-01-01");
        second.played_date = Some("2026-08-02T10:00:00.000Z".to_owned());
        let missing = raw("missing", "Never", "Artist", "Album", "2025-01-01");

        let derived = derive_library(
            &[older, newer, second, missing]
                .into_iter()
                .map(raw_track_to_domain)
                .collect::<Vec<_>>(),
        );

        assert_eq!(derived.recently_played.len(), 2);
        assert_eq!(derived.recently_played[0].title, "Song");
        assert_eq!(
            derived.recently_played[0].played_at,
            "2026-08-03T10:00:00.000Z"
        );
        assert_eq!(derived.recently_played[1].title, "Second");
    }

    #[test]
    fn local_search_matches_across_metadata_fields() {
        let tracks = vec![raw_track_to_domain(raw(
            "1",
            "Quiet Terminal",
            "The Asyncs",
            "Event Loop",
            "2025-01-01",
        ))];
        assert_eq!(
            search_track_ids(&tracks, "asyncs ambient", 10),
            vec![tracks[0].id.clone()]
        );
        assert!(search_track_ids(&tracks, "missing", 10).is_empty());
    }
}
