use serde::{Deserialize, Deserializer};

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RawMusicState {
    pub running: bool,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub position: Option<f64>,
    #[serde(default)]
    pub volume: Option<f64>,
    #[serde(default)]
    pub muted: Option<bool>,
    #[serde(default)]
    pub shuffle: Option<bool>,
    #[serde(default, rename = "repeat")]
    pub repeat_mode: Option<String>,
    #[serde(default)]
    pub track: Option<RawTrack>,
    #[serde(default)]
    pub playlists: Option<Vec<RawPlaylist>>,
    #[serde(default)]
    pub library_batch: Option<RawTrackBatch>,
    #[serde(default)]
    pub playlist_batch: Option<RawPlaylistTrackBatch>,
    #[serde(default)]
    pub artwork: Option<RawArtwork>,
    #[serde(default)]
    pub profile: Option<RawProfileMetrics>,
    #[serde(default)]
    pub session_advanced: bool,
    #[serde(default)]
    pub error: Option<RawScriptError>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RawProfileMetrics {
    pub collection_ms: f64,
    pub serialization_ms: f64,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RawArtwork {
    #[serde(default)]
    pub raw_data: Option<String>,
    #[serde(default)]
    pub missing: bool,
    #[serde(default)]
    pub too_large: bool,
    #[serde(default)]
    pub encoded_bytes: Option<usize>,
    #[serde(default)]
    pub resolver: Option<String>,
    #[serde(default)]
    pub attempts: Vec<String>,
    #[serde(default)]
    pub transient: bool,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RawTrack {
    #[serde(default, deserialize_with = "optional_string")]
    pub persistent_id: Option<String>,
    #[serde(default, deserialize_with = "optional_string")]
    pub database_id: Option<String>,
    pub name: Option<String>,
    #[serde(default)]
    pub artist: Option<String>,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub album_artist: Option<String>,
    #[serde(default)]
    pub composer: Option<String>,
    #[serde(default)]
    pub genre: Option<String>,
    #[serde(default)]
    pub duration: Option<f64>,
    #[serde(default)]
    pub favorited: Option<bool>,
    #[serde(default)]
    pub disliked: Option<bool>,
    #[serde(default)]
    pub year: Option<u16>,
    #[serde(default)]
    pub track_number: Option<u16>,
    #[serde(default)]
    pub disc_number: Option<u16>,
    #[serde(default)]
    pub played_count: Option<u64>,
    #[serde(default)]
    pub skipped_count: Option<u64>,
    #[serde(default)]
    pub date_added: Option<String>,
    #[serde(default)]
    pub played_date: Option<String>,
    #[serde(default)]
    pub skipped_date: Option<String>,
    #[serde(default)]
    pub modification_date: Option<String>,
    #[serde(default)]
    pub release_date: Option<String>,
    #[serde(default)]
    pub rating: Option<u8>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub cloud_status: Option<String>,
    #[serde(default)]
    pub media_kind: Option<String>,
}

fn optional_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match value {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(value)) => Some(value),
        Some(serde_json::Value::Number(value)) => Some(value.to_string()),
        Some(serde_json::Value::Bool(value)) => Some(value.to_string()),
        Some(_) => None,
    })
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RawPlaylist {
    pub persistent_id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub smart: bool,
    #[serde(default)]
    pub parent_persistent_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RawTrackBatch {
    pub start: usize,
    pub total: usize,
    #[serde(default)]
    pub tracks: Vec<RawTrack>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RawPlaylistTrackBatch {
    pub playlist_persistent_id: String,
    pub start: usize,
    pub total: usize,
    #[serde(default)]
    pub tracks: Vec<RawTrack>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct RawScriptError {
    #[serde(default)]
    pub number: Option<i64>,
    pub message: String,
}

pub fn parse_output(output: &str) -> Result<RawMusicState, serde_json::Error> {
    serde_json::from_str(output.trim())
}

#[cfg(test)]
mod tests {
    use super::parse_output;

    #[test]
    fn parses_unicode_and_embedded_newlines_without_delimiters() {
        let output = include_str!("../../../tests/fixtures/music_state_playing.json");

        let parsed = parse_output(output).expect("valid structured output");
        let track = parsed.track.expect("track");
        assert_eq!(track.name.as_deref(), Some("Quoted \"song\"\n第二行"));
        assert_eq!(track.artist.as_deref(), Some("Björk"));
        assert_eq!(parsed.repeat_mode.as_deref(), Some("all"));
    }

    #[test]
    fn parses_not_running_without_track_fields() {
        let parsed = parse_output(r#"{"running":false}"#).expect("valid state");
        assert!(!parsed.running);
        assert!(parsed.track.is_none());
        assert!(parsed.error.is_none());
    }

    #[test]
    fn parses_streamed_track_with_null_identifiers_and_optional_metadata() {
        let parsed = parse_output(
            r#"{
                "running": true,
                "state": "paused",
                "position": 3,
                "track": {
                    "persistentId": null,
                    "databaseId": null,
                    "name": "Так же как все",
                    "artist": "A'Studio",
                    "album": "Волны",
                    "duration": 242.4,
                    "favorited": null
                }
            }"#,
        )
        .expect("valid streamed state");

        let track = parsed.track.expect("track");
        assert!(track.persistent_id.is_none());
        assert!(track.database_id.is_none());
        assert_eq!(track.name.as_deref(), Some("Так же как все"));
        assert_eq!(track.artist.as_deref(), Some("A'Studio"));
        assert_eq!(track.album.as_deref(), Some("Волны"));
        assert_eq!(track.favorited, None);
    }

    #[test]
    fn parses_real_playlist_hierarchy_and_rich_batched_track_metadata() {
        let parsed = parse_output(
            r#"{
                "running": true,
                "state": "paused",
                "playlists": [{
                    "persistentId": "CHILD",
                    "name": "Smart Mix",
                    "description": "Local description",
                    "kind": "userPlaylist",
                    "smart": true,
                    "parentPersistentId": "FOLDER"
                }],
                "libraryBatch": {
                    "start": 200,
                    "total": 12997,
                    "tracks": [{
                        "persistentId": "ABC",
                        "databaseId": 19443,
                        "name": "Track",
                        "artist": "Artist",
                        "album": "Album",
                        "albumArtist": "Album Artist",
                        "composer": "Composer",
                        "genre": "Electronic",
                        "duration": 242.4,
                        "year": 2025,
                        "trackNumber": 3,
                        "discNumber": 1,
                        "playedCount": 7,
                        "skippedCount": 2,
                        "dateAdded": "2026-01-02T03:04:05.000Z",
                        "rating": 80,
                        "favorited": true,
                        "enabled": true,
                        "cloudStatus": "purchased",
                        "mediaKind": "sharedTrack"
                    }]
                }
            }"#,
        )
        .expect("rich response");

        let playlist = &parsed.playlists.expect("playlists")[0];
        assert!(playlist.smart);
        assert_eq!(playlist.parent_persistent_id.as_deref(), Some("FOLDER"));
        let batch = parsed.library_batch.expect("batch");
        assert_eq!((batch.start, batch.total), (200, 12_997));
        let track = &batch.tracks[0];
        assert_eq!(track.database_id.as_deref(), Some("19443"));
        assert_eq!(track.album_artist.as_deref(), Some("Album Artist"));
        assert_eq!(track.played_count, Some(7));
        assert_eq!(track.cloud_status.as_deref(), Some("purchased"));
    }
}
