#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TrackSelector {
    PersistentId(String),
    DatabaseId(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScriptRequest {
    FullState,
    Poll,
    DiscoverPlaylists,
    LibraryBatch {
        start: usize,
        limit: usize,
        total: Option<usize>,
    },
    PlaylistBatch {
        playlist_persistent_id: String,
        start: usize,
        limit: usize,
        total: Option<usize>,
    },
    OpenPlayer,
    Play,
    Pause,
    PlayPause,
    PlayTrack(TrackSelector),
    PlayPlaylistTrack {
        playlist_persistent_id: String,
        track: TrackSelector,
    },
    PlayPlaylist(String),
    Next,
    Previous,
    SeekBy(i64),
    SetVolume(u8),
    ToggleShuffle,
    CycleRepeat,
}

impl ScriptRequest {
    const fn includes_favorite(&self) -> bool {
        !matches!(self, Self::Poll)
    }

    fn operation(&self) -> String {
        match self {
            Self::FullState
            | Self::Poll
            | Self::DiscoverPlaylists
            | Self::LibraryBatch { .. }
            | Self::PlaylistBatch { .. } => String::new(),
            Self::OpenPlayer => "music.activate();".to_owned(),
            Self::Play => "music.play();".to_owned(),
            Self::Pause => "music.pause();".to_owned(),
            Self::PlayPause => "music.playpause();".to_owned(),
            Self::PlayTrack(selector) => {
                let (property, value) = match selector {
                    TrackSelector::PersistentId(value) => ("persistentID", value),
                    TrackSelector::DatabaseId(value) => ("databaseID", value),
                };
                let value = js_string(value);
                format!(
                    "const selectedTracks = music.libraryPlaylists()[0].tracks.whose({{{property}: {value}}})();\n\
                     if (selectedTracks.length === 0) throw new Error('Selected track is no longer available');\n\
                     music.play(selectedTracks[0]);"
                )
            }
            Self::PlayPlaylistTrack {
                playlist_persistent_id,
                track,
            } => {
                let (property, value) = match track {
                    TrackSelector::PersistentId(value) => ("persistentID", value),
                    TrackSelector::DatabaseId(value) => ("databaseID", value),
                };
                let playlist_persistent_id = js_string(playlist_persistent_id);
                let value = js_string(value);
                format!(
                    "const selectedPlaylists = music.playlists.whose({{persistentID: {playlist_persistent_id}}})();\n\
                     if (selectedPlaylists.length === 0) throw new Error('Selected playlist is no longer available');\n\
                     const selectedTracks = selectedPlaylists[0].tracks.whose({{{property}: {value}}})();\n\
                     if (selectedTracks.length === 0) throw new Error('Selected track is no longer available in this playlist');\n\
                     music.play(selectedTracks[0]);"
                )
            }
            Self::PlayPlaylist(persistent_id) => {
                let persistent_id = js_string(persistent_id);
                format!(
                    "const selectedPlaylists = music.playlists.whose({{persistentID: {persistent_id}}})();\n\
                     if (selectedPlaylists.length === 0) throw new Error('Selected playlist is no longer available');\n\
                     music.play(selectedPlaylists[0]);"
                )
            }
            Self::Next => "music.nextTrack();".to_owned(),
            Self::Previous => "music.previousTrack();".to_owned(),
            Self::SeekBy(seconds) => format!(
                "music.playerPosition = Math.max(0, Number(music.playerPosition()) + {seconds});"
            ),
            Self::SetVolume(volume) => {
                format!("music.soundVolume = Math.max(0, Math.min(100, {volume}));")
            }
            Self::ToggleShuffle => {
                "music.shuffleEnabled = !Boolean(music.shuffleEnabled());".to_owned()
            }
            Self::CycleRepeat => concat!(
                "const currentRepeat = String(music.songRepeat());",
                "music.songRepeat = currentRepeat === 'off' ? 'all' : ",
                "(currentRepeat === 'all' ? 'one' : 'off');"
            )
            .to_owned(),
        }
    }
}

#[must_use]
pub fn build_script(request: &ScriptRequest) -> String {
    let mut script = String::from(SCRIPT_PREFIX);
    if matches!(request, ScriptRequest::OpenPlayer) {
        script.push_str(&request.operation());
        script.push_str(SCRIPT_RUNNING_CHECK);
    } else {
        script.push_str(SCRIPT_RUNNING_CHECK);
        script.push_str(&request.operation());
    }
    script.push_str(SCRIPT_STATE);
    script.push_str("const includeFavorite = ");
    script.push_str(if request.includes_favorite() {
        "true"
    } else {
        "false"
    });
    script.push_str(";\n");
    script.push_str(SCRIPT_CURRENT_TRACK);

    match request {
        ScriptRequest::DiscoverPlaylists => script.push_str(SCRIPT_PLAYLISTS),
        ScriptRequest::LibraryBatch {
            start,
            limit,
            total,
        } => {
            script.push_str(SCRIPT_BATCH_HELPERS);
            let total = total.map_or_else(
                || "music.libraryPlaylists()[0].tracks().length".to_owned(),
                |total| total.to_string(),
            );
            script.push_str(&format!(
                "result.libraryBatch = readTrackBatch(music.libraryPlaylists()[0], {start}, {limit}, {total});\n"
            ));
        }
        ScriptRequest::PlaylistBatch {
            playlist_persistent_id,
            start,
            limit,
            total,
        } => {
            script.push_str(SCRIPT_BATCH_HELPERS);
            let persistent_id = js_string(playlist_persistent_id);
            script.push_str(&format!(
                "const batchPlaylists = music.playlists.whose({{persistentID: {persistent_id}}})();\n\
                 if (batchPlaylists.length === 0) throw new Error('Selected playlist is no longer available');\n"
            ));
            let total = total.map_or_else(
                || "batchPlaylists[0].tracks().length".to_owned(),
                |total| total.to_string(),
            );
            script.push_str(&format!(
                "const playlistBatch = readTrackBatch(batchPlaylists[0], {start}, {limit}, {total});\n\
                 playlistBatch.playlistPersistentId = {persistent_id};\n\
                 result.playlistBatch = playlistBatch;\n"
            ));
        }
        _ => {}
    }
    script.push_str(SCRIPT_SUFFIX);
    script
}

fn js_string(value: &str) -> String {
    serde_json::to_string(value).expect("serializing a Rust string to JSON cannot fail")
}

const SCRIPT_PREFIX: &str = r#"(() => {
const music = Application("com.apple.Music");
const result = { running: false };
const safe = (read) => {
    try {
        const value = read();
        return value === undefined ? null : value;
    } catch (_) {
        return null;
    }
};
const readText = (read) => {
    const value = safe(read);
    if (value === null || value === undefined) return null;
    const text = String(value).trim();
    return text.length === 0 ? null : text;
};
const readIdentifier = (read) => {
    const text = readText(read);
    if (text === null) return null;
    const normalized = text.toLowerCase();
    return ["null", "undefined", "missing value", "0", "-1"].includes(normalized)
        ? null
        : text;
};
const errorInfo = (error) => ({
    number: Number.isFinite(error.errorNumber) ? error.errorNumber : null,
    message: String(error.message || error)
});
try {
"#;

const SCRIPT_RUNNING_CHECK: &str = r#"
    result.running = Boolean(music.running());
    if (!result.running) return JSON.stringify(result);
"#;

const SCRIPT_STATE: &str = r#"
    result.state = String(music.playerState());
    result.position = Number(music.playerPosition());
    result.volume = Number(music.soundVolume());
    result.muted = Boolean(music.mute());
    result.shuffle = Boolean(music.shuffleEnabled());
    result.repeat = String(music.songRepeat());
"#;

const SCRIPT_CURRENT_TRACK: &str = r#"
    const track = music.currentTrack;
    const persistentId = readIdentifier(() => track.persistentID());
    const databaseId = readIdentifier(() => track.databaseID());
    const name = readText(() => track.name());
    const artist = readText(() => track.artist());
    const album = readText(() => track.album());
    const rawDuration = safe(() => Number(track.duration()));
    const duration = Number.isFinite(rawDuration) && rawDuration >= 0 ? rawDuration : null;
    const hasTrack = persistentId !== null || databaseId !== null || name !== null
        || artist !== null || album !== null || duration !== null;
    if (hasTrack) {
        result.track = {
            persistentId,
            databaseId,
            name,
            artist,
            album,
            duration,
            favorited: includeFavorite ? safe(() => Boolean(track.favorited())) : null
        };
    }
"#;

const SCRIPT_PLAYLISTS: &str = r#"
    const allPlaylistProperties = safe(() => music.playlists.properties()) || [];
    const userPlaylistProperties = safe(() => music.userPlaylists.properties()) || [];
    const userPlaylistDescriptions = safe(() => music.userPlaylists.description()) || [];
    const userPlaylistReferences = safe(() => music.userPlaylists()) || [];
    const userInfo = {};
    for (let index = 0; index < userPlaylistProperties.length; index += 1) {
        const properties = userPlaylistProperties[index] || {};
        const id = readIdentifier(() => properties.persistentID);
        if (id === null) continue;
        userInfo[id] = {
            smart: Boolean(properties.smart),
            description: readText(() => userPlaylistDescriptions[index]),
            parentPersistentId: readIdentifier(() => userPlaylistReferences[index].parent().persistentID())
        };
    }
    result.playlists = allPlaylistProperties
        .filter((properties) => properties && properties.visible !== false
            && String(properties.class) !== "libraryPlaylist")
        .map((properties) => {
            const id = readIdentifier(() => properties.persistentID);
            const details = id === null ? null : userInfo[id];
            return {
                persistentId: id,
                name: readText(() => properties.name) || "Untitled Playlist",
                description: details ? details.description : null,
                kind: readText(() => properties.class),
                smart: details ? details.smart : false,
                parentPersistentId: details ? details.parentPersistentId : null
            };
        })
        .filter((playlist) => playlist.persistentId !== null);
"#;

const SCRIPT_BATCH_HELPERS: &str = r#"
    const serializable = (value) => {
        if (value === null || value === undefined) return null;
        if (value instanceof Date) return value.toISOString();
        if (["string", "number", "boolean"].includes(typeof value)) return value;
        return String(value);
    };
    const readValues = (read, count) => {
        const values = safe(read);
        if (!Array.isArray(values)) return Array(count).fill(null);
        const normalized = values.map(serializable);
        while (normalized.length < count) normalized.push(null);
        return normalized;
    };
    const readSingleTrack = (track) => ({
        persistentId: serializable(safe(() => track.persistentID())),
        databaseId: serializable(safe(() => track.databaseID())),
        name: serializable(safe(() => track.name())),
        artist: serializable(safe(() => track.artist())),
        album: serializable(safe(() => track.album())),
        albumArtist: serializable(safe(() => track.albumArtist())),
        composer: serializable(safe(() => track.composer())),
        genre: serializable(safe(() => track.genre())),
        duration: serializable(safe(() => track.duration())),
        favorited: serializable(safe(() => track.favorited())),
        disliked: serializable(safe(() => track.disliked())),
        year: serializable(safe(() => track.year())),
        trackNumber: serializable(safe(() => track.trackNumber())),
        discNumber: serializable(safe(() => track.discNumber())),
        playedCount: serializable(safe(() => track.playedCount())),
        skippedCount: serializable(safe(() => track.skippedCount())),
        dateAdded: serializable(safe(() => track.dateAdded())),
        playedDate: serializable(safe(() => track.playedDate())),
        skippedDate: serializable(safe(() => track.skippedDate())),
        modificationDate: serializable(safe(() => track.modificationDate())),
        releaseDate: serializable(safe(() => track.releaseDate())),
        rating: serializable(safe(() => track.rating())),
        enabled: serializable(safe(() => track.enabled())),
        cloudStatus: serializable(safe(() => track.cloudStatus())),
        mediaKind: serializable(safe(() => track.class()))
    });
    const readTrackBatch = (container, start, limit, knownTotal) => {
        const total = Number(knownTotal);
        const count = Math.max(0, Math.min(limit, total - start));
        if (count === 0) return { start, total, tracks: [] };
        const tracks = container.tracks.slice(start, start + count);
        const fields = {
            persistentId: readValues(() => tracks.persistentID(), count),
            databaseId: readValues(() => tracks.databaseID(), count),
            name: readValues(() => tracks.name(), count),
            artist: readValues(() => tracks.artist(), count),
            album: readValues(() => tracks.album(), count),
            albumArtist: readValues(() => tracks.albumArtist(), count),
            composer: readValues(() => tracks.composer(), count),
            genre: readValues(() => tracks.genre(), count),
            duration: readValues(() => tracks.duration(), count),
            favorited: readValues(() => tracks.favorited(), count),
            disliked: readValues(() => tracks.disliked(), count),
            year: readValues(() => tracks.year(), count),
            trackNumber: readValues(() => tracks.trackNumber(), count),
            discNumber: readValues(() => tracks.discNumber(), count),
            playedCount: readValues(() => tracks.playedCount(), count),
            skippedCount: readValues(() => tracks.skippedCount(), count),
            dateAdded: readValues(() => tracks.dateAdded(), count),
            playedDate: readValues(() => tracks.playedDate(), count),
            skippedDate: readValues(() => tracks.skippedDate(), count),
            modificationDate: readValues(() => tracks.modificationDate(), count),
            releaseDate: readValues(() => tracks.releaseDate(), count),
            rating: readValues(() => tracks.rating(), count),
            enabled: readValues(() => tracks.enabled(), count),
            cloudStatus: readValues(() => tracks.cloudStatus(), count),
            mediaKind: readValues(() => tracks.class(), count)
        };
        const primaryUnavailable = fields.name.every((value) => value === null)
            && fields.persistentId.every((value) => value === null)
            && fields.databaseId.every((value) => value === null);
        if (primaryUnavailable) {
            // Some user playlists return concrete track references that reject property-array
            // selectors. Keep the fallback bounded so those Apple Events cannot monopolize the
            // backend worker; subsequent polls continue at the returned offset.
            const references = container.tracks();
            const fallbackCount = Math.min(count, 20);
            const records = references
                .slice(start, start + fallbackCount)
                .map(readSingleTrack);
            return { start, total, tracks: records };
        }
        const records = [];
        for (let index = 0; index < count; index += 1) {
            const record = {};
            Object.keys(fields).forEach((key) => { record[key] = fields[key][index]; });
            records.push(record);
        }
        return { start, total, tracks: records };
    };
"#;

const SCRIPT_SUFFIX: &str = r#"
    return JSON.stringify(result);
} catch (error) {
    result.error = errorInfo(error);
    return JSON.stringify(result);
}
})();
"#;

#[cfg(test)]
mod tests {
    use super::{ScriptRequest, TrackSelector, build_script};

    #[test]
    fn commands_are_constructed_from_typed_values() {
        let seek = build_script(&ScriptRequest::SeekBy(-5));
        let volume = build_script(&ScriptRequest::SetVolume(73));
        let selected = build_script(&ScriptRequest::PlayTrack(TrackSelector::PersistentId(
            "ABC\"; malicious()".to_owned(),
        )));

        assert!(seek.contains("playerPosition()) + -5"));
        assert!(volume.contains("Math.min(100, 73)"));
        assert!(selected.contains(r#"persistentID: "ABC\"; malicious()""#));
        assert!(!selected.contains("persistentID: ABC"));
        assert!(!seek.contains("sh -c"));
    }

    #[test]
    fn polling_uses_correct_music_app_identifier_acronyms() {
        let poll = build_script(&ScriptRequest::Poll);
        assert!(poll.contains("track.persistentID()"));
        assert!(poll.contains("track.databaseID()"));
        assert!(!poll.contains("track.persistentId()"));
        assert!(poll.contains("includeFavorite = false"));
        assert!(build_script(&ScriptRequest::FullState).contains("includeFavorite = true"));
    }

    #[test]
    fn library_requests_are_bounded_and_read_selected_properties() {
        let script = build_script(&ScriptRequest::LibraryBatch {
            start: 200,
            limit: 200,
            total: Some(12_997),
        });
        assert!(script.contains("readTrackBatch(music.libraryPlaylists()[0], 200, 200, 12997)"));
        assert!(script.contains("tracks.albumArtist()"));
        assert!(script.contains("tracks.cloudStatus()"));
        assert!(!script.contains("properties()"));
    }
}
