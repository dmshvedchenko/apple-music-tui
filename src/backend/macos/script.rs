#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TrackSelector {
    PersistentId(String),
    DatabaseId(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScriptRequest {
    FullState,
    Poll,
    PollPlaylistTransition {
        playlist_persistent_id: String,
        expected: TrackSelector,
        target: TrackSelector,
        max_wait_ms: u64,
    },
    DiscoverPlaylists,
    LibraryBatch {
        start: usize,
        limit: usize,
        total: Option<usize>,
    },
    #[cfg(test)]
    ProfileLibraryBatch {
        start: usize,
        limit: usize,
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
    Stop,
    PlayPause,
    PlayTrack(TrackSelector),
    PlayTrackOnce(TrackSelector),
    LoadTrackArtwork {
        track: TrackSelector,
        max_bytes: usize,
    },
    PlayPlaylistTrackOnce {
        playlist_persistent_id: String,
        track: TrackSelector,
    },
    PlayPlaylist(String),
    RemovePlaylistTrack {
        playlist_persistent_id: String,
        index: usize,
        expected: TrackSelector,
    },
    Next,
    Previous,
    SeekBy(i64),
    SetVolume(u8),
    ToggleShuffle,
    CycleRepeat,
}

impl ScriptRequest {
    const fn includes_favorite(&self) -> bool {
        !matches!(self, Self::Poll | Self::PollPlaylistTransition { .. })
    }

    fn operation(&self) -> String {
        match self {
            Self::FullState
            | Self::Poll
            | Self::DiscoverPlaylists
            | Self::LibraryBatch { .. }
            | Self::PlaylistBatch { .. } => String::new(),
            Self::PollPlaylistTransition {
                playlist_persistent_id,
                expected,
                target,
                max_wait_ms,
            } => {
                let (expected_property, expected_value) = selector_parts(expected);
                let (target_property, target_value) = selector_parts(target);
                let playlist_persistent_id = js_string(playlist_persistent_id);
                let expected_value = js_string(expected_value);
                let expected_is_persistent = expected_property == "persistentID";
                let target_value = js_string(target_value);
                format!(
                    "const transitionDeadline = Date.now() + {max_wait_ms};\n\
                     const initialTransitionState = String(music.playerState());\n\
                     const initialTransitionTrack = music.currentTrack;\n\
                     const initialPersistentId = readIdentifier(() => initialTransitionTrack.persistentID());\n\
                     const initialDatabaseId = readIdentifier(() => initialTransitionTrack.databaseID());\n\
                     const initialMatches = {expected_is_persistent}\n\
                         ? initialPersistentId === {expected_value}\n\
                         : initialDatabaseId === {expected_value};\n\
                     if (initialMatches && initialTransitionState === 'playing') {{\n\
                         while (Date.now() <= transitionDeadline && String(music.playerState()) === 'playing') {{\n\
                         delay(0.05);\n\
                     }}\n\
                     }}\n\
                     const finalTransitionState = String(music.playerState());\n\
                     const finalTransitionTrack = music.currentTrack;\n\
                     const finalPersistentId = readIdentifier(() => finalTransitionTrack.persistentID());\n\
                     const finalDatabaseId = readIdentifier(() => finalTransitionTrack.databaseID());\n\
                     const finalHasIdentity = finalPersistentId !== null || finalDatabaseId !== null;\n\
                     const finalMatches = !finalHasIdentity || ({expected_is_persistent}\n\
                         ? finalPersistentId === {expected_value}\n\
                         : finalDatabaseId === {expected_value});\n\
                     if (finalTransitionState === 'stopped' && finalMatches) {{\n\
                         const transitionPlaylists = music.playlists.whose({{persistentID: {playlist_persistent_id}}})();\n\
                         if (transitionPlaylists.length === 0) throw new Error('Playlist continuation source is no longer available');\n\
                         const transitionTargets = transitionPlaylists[0].tracks.whose({{{target_property}: {target_value}}})();\n\
                         if (transitionTargets.length === 0) throw new Error('Next playlist track is no longer available');\n\
                         music.play(transitionTargets[0], {{once: true}});\n\
                         result.sessionAdvanced = true;\n\
                     }}"
                )
            }
            #[cfg(test)]
            Self::ProfileLibraryBatch { .. } => String::new(),
            Self::OpenPlayer => "music.activate();".to_owned(),
            Self::Play => "music.play();".to_owned(),
            Self::Pause => "music.pause();".to_owned(),
            Self::Stop => "music.stop();".to_owned(),
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
            Self::PlayTrackOnce(selector) => {
                let (property, value) = match selector {
                    TrackSelector::PersistentId(value) => ("persistentID", value),
                    TrackSelector::DatabaseId(value) => ("databaseID", value),
                };
                let value = js_string(value);
                format!(
                    "const selectedTracks = music.libraryPlaylists()[0].tracks.whose({{{property}: {value}}})();\n\
                     if (selectedTracks.length === 0) throw new Error('Selected album track is no longer available');\n\
                     music.play(selectedTracks[0], {{once: true}});"
                )
            }
            Self::LoadTrackArtwork { track, max_bytes } => {
                let (property, value) = match track {
                    TrackSelector::PersistentId(value) => ("persistentID", value),
                    TrackSelector::DatabaseId(value) => ("databaseID", value),
                };
                let value = js_string(value);
                format!(
                    "const artworkCandidates = [];\n\
                     const artworkAttempts = [];\n\
                     const artworkCurrent = safe(() => music.currentTrack());\n\
                     const artworkCurrentId = artworkCurrent === null ? null : readIdentifier(() => artworkCurrent.{property}());\n\
                     if (artworkCurrentId !== null && artworkCurrentId === {value}) {{ artworkCandidates.push({{ track: artworkCurrent, resolver: 'current_track' }}); artworkAttempts.push('current_track:matched'); }} else artworkAttempts.push('current_track:identity_mismatch');\n\
                     const artworkLibraryTracks = safe(() => music.libraryPlaylists()[0].tracks.whose({{{property}: {value}}})()) || [];\n\
                     if (artworkLibraryTracks.length > 0) {{ artworkCandidates.push({{ track: artworkLibraryTracks[0], resolver: '{property}_library' }}); artworkAttempts.push('{property}_library:matched'); }} else artworkAttempts.push('{property}_library:not_found');\n\
                     let artworkSawNoArtwork = false;\n\
                     let artworkResolved = false;\n\
                     for (const artworkCandidate of artworkCandidates) {{\n\
                         const artworkItems = safe(() => artworkCandidate.track.artworks()) || [];\n\
                         if (artworkItems.length === 0) {{ artworkSawNoArtwork = true; artworkAttempts.push(artworkCandidate.resolver + ':no_artwork'); continue; }}\n\
                         const descriptor = readText(() => artworkItems[0].rawData());\n\
                         const match = descriptor === null ? null : /\\(\\$([0-9A-Fa-f]+)\\$\\)/.exec(descriptor);\n\
                         if (match === null) {{ artworkSawNoArtwork = true; artworkAttempts.push(artworkCandidate.resolver + ':unreadable'); continue; }}\n\
                         const encodedBytes = Math.floor(match[1].length / 2);\n\
                         result.artwork = encodedBytes > {max_bytes}\n\
                             ? {{ tooLarge: true, encodedBytes, resolver: artworkCandidate.resolver, attempts: artworkAttempts }}\n\
                             : {{ rawData: match[1], encodedBytes, resolver: artworkCandidate.resolver, attempts: artworkAttempts }};\n\
                         artworkResolved = true;\n\
                         break;\n\
                     }}\n\
                     if (!artworkResolved) result.artwork = artworkSawNoArtwork\n\
                         ? {{ missing: true, attempts: artworkAttempts }}\n\
                         : {{ transient: true, reason: 'requested artwork track could not be resolved from currentTrack or library', attempts: artworkAttempts }};"
                )
            }
            Self::PlayPlaylistTrackOnce {
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
                     music.play(selectedTracks[0], {{once: true}});"
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
            Self::RemovePlaylistTrack {
                playlist_persistent_id,
                index,
                expected,
            } => {
                let (property, value) = selector_parts(expected);
                let playlist_persistent_id = js_string(playlist_persistent_id);
                let value = js_string(value);
                format!(
                    "const editablePlaylists = music.userPlaylists.whose({{persistentID: {playlist_persistent_id}}})();\n\
                     if (editablePlaylists.length === 0) throw new Error('Editable user playlist is no longer available');\n\
                     const editablePlaylist = editablePlaylists[0];\n\
                     if (Boolean(safe(() => editablePlaylist.smart()))) throw new Error('Smart playlists cannot be edited');\n\
                     const editableTracks = editablePlaylist.tracks();\n\
                     if ({index} >= editableTracks.length) throw new Error('Selected playlist entry is no longer available');\n\
                     const editableTrack = editableTracks[{index}];\n\
                     if (readIdentifier(() => editableTrack.{property}()) !== {value}) throw new Error('Playlist changed; selected entry no longer matches');\n\
                     music.delete(editableTrack);"
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
        #[cfg(test)]
        ScriptRequest::ProfileLibraryBatch { start, limit } => {
            script.push_str(SCRIPT_BATCH_HELPERS);
            script.push_str(&format!(
                "const profileTotal = music.libraryPlaylists()[0].tracks().length;\n\
                 const collectionStarted = Date.now();\n\
                 result.libraryBatch = readTrackBatch(music.libraryPlaylists()[0], {start}, {limit}, profileTotal);\n\
                 const collectionMs = Date.now() - collectionStarted;\n\
                 const serializationStarted = Date.now();\n\
                 JSON.stringify(result);\n\
                 result.profile = {{ collectionMs, serializationMs: Date.now() - serializationStarted }};\n"
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

fn selector_parts(selector: &TrackSelector) -> (&'static str, &str) {
    match selector {
        TrackSelector::PersistentId(value) => ("persistentID", value),
        TrackSelector::DatabaseId(value) => ("databaseID", value),
    }
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

        let artwork = build_script(&ScriptRequest::LoadTrackArtwork {
            track: TrackSelector::DatabaseId("42\"; malicious()".to_owned()),
            max_bytes: 1_024,
        });
        assert!(artwork.contains(r#"databaseID: "42\"; malicious()""#));
        assert!(artwork.contains("encodedBytes > 1024"));
        assert!(!artwork.contains("databaseID: 42"));
        assert!(artwork.contains("music.currentTrack()"));
        assert!(artwork.contains("resolver: 'current_track'"));
        assert!(artwork.contains("databaseID_library"));
        assert!(artwork.contains("artworkCurrentId !== null"));
        assert!(artwork.contains("artworkAttempts"));

        let playlist_track = build_script(&ScriptRequest::PlayPlaylistTrackOnce {
            playlist_persistent_id: "P\"; malicious()".to_owned(),
            track: TrackSelector::PersistentId("T\"; malicious()".to_owned()),
        });
        assert!(playlist_track.contains(r#"persistentID: "P\"; malicious()""#));
        assert!(playlist_track.contains(r#"persistentID: "T\"; malicious()""#));
        assert!(playlist_track.contains("music.play(selectedTracks[0], {once: true})"));

        let transition = build_script(&ScriptRequest::PollPlaylistTransition {
            playlist_persistent_id: "P\"; malicious()".to_owned(),
            expected: TrackSelector::PersistentId("FROM\"; malicious()".to_owned()),
            target: TrackSelector::DatabaseId("42\"; malicious()".to_owned()),
            max_wait_ms: 2_000,
        });
        assert!(transition.contains(r#"initialPersistentId === "FROM\"; malicious()""#));
        assert!(transition.contains(r#"databaseID: "42\"; malicious()""#));
        assert!(transition.contains("Date.now() + 2000"));
        assert!(transition.contains("delay(0.05)"));
        assert!(transition.contains("result.sessionAdvanced = true"));

        let removal = build_script(&ScriptRequest::RemovePlaylistTrack {
            playlist_persistent_id: "PLAYLIST-123".to_owned(),
            index: 2,
            expected: TrackSelector::PersistentId("TRACK-456".to_owned()),
        });
        assert!(removal.contains("music.userPlaylists.whose({persistentID: \"PLAYLIST-123\"})"));
        assert!(removal.contains("editableTracks[2]"));
        assert!(removal.contains("editableTrack.persistentID()) !== \"TRACK-456\""));
        assert!(removal.contains("music.delete(editableTrack);"));
        assert!(!removal.contains("\\\\\n"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn production_removal_script_parses_without_running_it() {
        use std::process::Command;

        let script = build_script(&ScriptRequest::RemovePlaylistTrack {
            playlist_persistent_id: "PLAYLIST-123".to_owned(),
            index: 2,
            expected: TrackSelector::PersistentId("TRACK-456".to_owned()),
        });
        let probe = format!(
            "new Function({});",
            serde_json::to_string(&script).expect("script JSON string")
        );
        let output = Command::new("/usr/bin/osascript")
            .args(["-l", "JavaScript", "-e", &probe])
            .output()
            .expect("run macOS JavaScript parser");
        assert!(
            output.status.success(),
            "generated removal JXA did not parse: {}",
            String::from_utf8_lossy(&output.stderr)
        );
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
