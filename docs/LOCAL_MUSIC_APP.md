# Local Music.app Capability Audit

Research and live verification date: 2026-08-03. Installed application: Music.app 1.6.5. Installed scripting definition: `/System/Applications/Music.app/Contents/Resources/com.apple.Music.sdef`.

This project treats the documented Music.app Apple Events surface as the primary no-paid-account integration on macOS. It requires the user's macOS Automation consent, but it does not require Apple Developer Program membership, a Developer Token, a Music User Token, or network access.

## Mechanisms evaluated

| Mechanism | What it provides | Decision |
|---|---|---|
| Music.app scripting dictionary through JXA/Apple Events | Live playback, library playlists/tracks, rich track metadata, stable Music.app identifiers, playlist hierarchy/classification, selected-item playback | Implemented as the primary local backend |
| ScriptingBridge | A native Objective-C bridge generated from the same scripting definition | Valid future transport optimization, but no additional Music.app data or commands |
| MediaLibrary framework | Local media sources, groups, objects, and file-oriented metadata | Not adopted: it does not improve protected/cloud track playback or Music.app playlist semantics |
| Music library XML export | User-triggered metadata/playlist export | Explicit manual fallback only; not live and never triggered automatically |
| Spotlight / `NSMetadataQuery` | Indexed local-file metadata | Not a library-membership or cloud-playlist interface; not adopted |
| Private Music database files, MediaRemote/private frameworks, undocumented network endpoints | Potential undocumented data/control | Prohibited and not used |

Official references: [ScriptingBridge](https://developer.apple.com/documentation/scriptingbridge), [MediaLibrary](https://developer.apple.com/documentation/medialibrary/mlmedialibrary), [NSMetadataQuery](https://developer.apple.com/documentation/foundation/nsmetadataquery), and [Apple's manual Music library/playlist export guide](https://support.apple.com/en-au/guide/music/-mus27cd5060f/mac).

## Installed dictionary findings

The installed definition exposes application sources, library/user/folder/subscription playlists, playlist parent relationships, playlist descriptions, smart-playlist classification, playlist tracks, and playback commands. It has read-only application/window/playlist `selection` specifiers, but the backend does not depend on visible UI selection: typed TUI selections resolve through persistent/database identifiers instead. `current playlist` is read-only and means the playlist containing the targeted track; it is not Up Next. Shuffle and repeat are application playback properties, not per-playlist configuration.

Track objects expose persistent and database IDs plus title, artist, album, album artist, composer, genre, duration, year, track/disc number, play/skip counts and dates, date added, modification/release dates, rating, favorite/dislike state, enabled state, cloud status, class/media kind, artwork, and local-file location where applicable. File, URL, and shared track classes are public, and library/user playlists can contain these classes. Live cloud/subscription reads confirmed that `location` is not required; nullable/unsupported fields remain absent rather than failing the batch.

JXA uses the exact acronym spellings `persistentID()` and `databaseID()`. The former implementation used `persistentId()`/`databaseId()`, causing valid current-track identifiers to be discarded. Domain IDs remain namespaced as `musicapp:persistent:*`, `musicapp:database:*`, and `musicapp:playlist:persistent:*`.

The definition does not expose the Music.app Up Next queue, Play Next, Play Later, queue removal, or queue reordering. The generic `add` command adds files; it is not a verified identity-based cloud-track append operation. Generic make/delete/duplicate/move and writable playlist properties exist, but destructive mutations remain disabled until read behavior is stable and each mutation has confirmation UI, editability checks, rollback/error behavior, and dedicated live tests.

## Implemented local-only surface

| Feature | Implementation and live result |
|---|---|
| Playback/current item | Existing 500 ms authoritative Music.app reconciliation retained |
| Real playlists | Bulk playlist/property discovery; user, smart, folder, subscription, parent, and description metadata mapped when available |
| Playlist contents | Lazy, bounded loading when a detail route opens; property-array fast path plus a bounded selected-property fallback for Music.app playlist objects that reject range property selectors |
| Library songs | 200-track batches with explicit `Loading { loaded, total }` progress; 12,997 local Music.app items loaded successfully in the live audit |
| Metadata | Stable IDs, title, artist, album/album artist, composer, genre, duration, year, track/disc numbers, play/skip data, dates, rating, favorite, enabled, cloud status, and media kind |
| Albums/artists | Deterministically derived after the library scan using normalized album-artist/album and artist keys |
| Recently Added | Derived from Music.app `dateAdded` metadata and sorted newest first |
| Local search | Typed, normalized cached entries for tracks, artists, albums, and playlists. Track entries include title, artist, album/album artist, composer, and genre; `/` opens the search view |
| Selected track playback | Exact Music.app ID lookup; playlist detail commands carry playlist context because some playlist tracks are not addressable through the main library playlist |
| Selected playlist playback | Exact persistent-ID lookup; `P` plays the selected playlist |

The initial frame and playback state do not wait for collection loading. Playback polls and collection batches share the serialized backend worker and every collection response also reconciles the authoritative Music.app playback snapshot. Backend events distinguish playback-only updates, playlist replacement, library batches, and playlist batches, so the reducer never replaces or clones the entire 12,997-track collection on every 500 ms poll. Search-index entries are appended with progressive track batches and rebuilt once after derived artists/albums are installed, rather than rebuilding the whole index per keypress or batch. Subprocess stdout/stderr are drained concurrently to prevent large structured responses from deadlocking on an OS pipe buffer.

## Verified limits

- Music.app remains the audio engine; the Rust process never decodes protected audio.
- Folder playlists are navigable metadata containers, not assumed playable track lists.
- Smart-playlist track persistent IDs can alias the current playable object's identifier. Playback remains exact, but strict ID-equality assertions are limited to normal user playlists in the live integration test.
- Some concrete user playlist objects reject property-array selectors even though their individual track objects are readable. The fallback reads only selected documented properties and is capped at 20 tracks per event-loop update.
- Artwork bytes, plain lyrics, playlist mutations, favorites/ratings mutations, and Recently Played UI are not implemented in this task.
- Up Next and queue mutation remain unavailable because the installed public definition has no such surface.
- Optional Apple API authentication remains dormant and is not required for `--backend macos`.

## Live verification

The audit exercised the real installed Music.app, not only fixtures:

- 107 playlists discovered (user, smart, folder, and system classes present in the raw surface).
- 12,997 library tracks loaded progressively without blocking initial rendering.
- Real playlist detail displayed non-placeholder titles, artists, durations, descriptions where present, and track counts.
- `j/k`, Enter, Escape/back, `/` search routing, and clean `q` shutdown were exercised in the TUI.
- Exact selected user-playlist track playback and exact playlist playback succeeded, then the test restored the captured track and playing/paused state.
- Normal `cargo test` does not contact Music.app; opt-in live tests are marked ignored.

## Practical loading measurements

Measurements are from the real 12,997-item library on 2026-08-03 using the unoptimized debug build. A direct isolated 200-track documented-property batch completed in approximately 0.46 seconds. The complete progressive scan took approximately 55–60 seconds across 65 bounded requests, including serialized playback reconciliation and final artist/album/search-index derivation. The initial frame and current playback appeared immediately, playlist discovery arrived before the full scan, library progress advanced every batch, and keyboard/rendering remained responsive while loading.

Observed resident memory was approximately 19 MiB partway through loading and 97 MiB after the full library, derived albums/artists/Recently Added, and normalized search index were resident. These are practical debug-build figures, not a release-build ceiling; the retained domain model intentionally trades memory for offline-fast navigation/search after loading. No artwork bytes or local audio files were loaded.
