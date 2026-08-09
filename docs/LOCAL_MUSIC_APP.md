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

The definition does not expose the Music.app Up Next queue, Play Next, Play Later, queue removal, or queue reordering. The generic `add` command adds files. Generic make/delete/duplicate/move and writable playlist properties exist, but their behavior is not uniform between JXA and AppleScript and writes have no transaction or confirmation boundary. The contained mutation findings are recorded below; every mutation capability remains disabled.

## Implemented local-only surface

| Feature | Implementation and live result |
|---|---|
| Playback/current item | Existing 500 ms authoritative Music.app reconciliation retained |
| Real playlists | Bulk playlist/property discovery; user, smart, folder, subscription, parent, and description metadata mapped by stable ID; folders expand/collapse and nested playlists remain lazy-loadable |
| Playlist contents | Immediate explicit loading state; lazy foreground batches render/play progressively; property-array fast path plus a bounded 20-track selected-property fallback; terminal states distinguish loaded empty and error |
| Library songs | Profiled 400-track batches with explicit `Loading { loaded, total }` progress; 12,997 local Music.app items loaded successfully in the live audit |
| Metadata | Stable IDs, title, artist, album/album artist, composer, genre, duration, year, track/disc numbers, play/skip data, dates, rating, favorite, enabled, cloud status, and media kind |
| Albums/artists | Deterministically derived after the library scan; album identity includes album artist, title, and year, and tracks sort by disc/track number |
| Recently Added | Derived from Music.app `dateAdded` metadata and sorted newest first |
| Recently Played | Compact stable-ID entries derived only from real played dates, deduplicated and sorted newest first; labeled local Music.app history |
| Local search | Typed, normalized cached entries for tracks, artists, albums, and playlists. Track entries include title, artist, album/album artist, composer, and genre; `/` opens the search view |
| Library sorting/filtering | Runtime-only index views for Songs (title/artist/album/date/year/play count), Albums (title/artist/year/recently added), and Artists (name/album count/track count); case-insensitive filtering never queries Music.app |
| Selected track playback | Exact Music.app ID lookup; playlist detail creates an ordered stable-ID synthesized session because exact playlist track objects provide no reliable continuation queue |
| Selected playlist playback | Exact persistent-ID lookup; `P` plays the selected playlist |
| Whole-album playback | Synthesized exact-ID session: `P` starts the first derived disc/track-ordered item with `once: true`, polling advances after stop, and next/previous stay in the album until an external Music.app track change |
| Artwork | Lazy bounded track-artwork extraction for album detail and Now Playing; strict hex/type validation, 2 MiB read limit, 16-entry identity-keyed source cache, separately bounded Kitty renderable cache, 512 KiB inline limit, iTerm2 and PNG-Kitty output, and visible Unicode fallback. Kitty passes PNG through and converts JPEG to a bounded PNG off the render and Music.app automation paths. The installed public Music.app scripting definition exposes artwork on tracks, not a dependable artwork property on playlist objects, so Playlist Detail shows an explicit unavailable placeholder rather than fabricating playlist art. |

The initial frame and playback state do not wait for collection loading. Playback polls and collection batches share the serialized backend worker and every collection response also reconciles the authoritative Music.app playback snapshot. Ready user commands are selected before background ticks; first and continuation playlist batches remain ahead of the library scan, while an already running Apple Event remains non-preemptible. Backend events distinguish playback-only updates, playlist replacement, library batches, playlist batches/failures, playback-context failures, and artwork, so the reducer never replaces or clones the entire 12,997-track collection on every 500 ms poll. Search-index entries are appended with progressive track batches and rebuilt once after derived artists/albums are installed, rather than rebuilding the whole index per keypress or batch. Subprocess stdout/stderr are drained concurrently to prevent large structured responses from deadlocking on an OS pipe buffer.

## Verified limits

- Music.app remains the audio engine; the Rust process never decodes protected audio.
- Folder playlists are navigable metadata containers, not assumed playable track lists.
- Synthesized sessions create no temporary Music.app playlist and do not expose Up Next. Near an expected playlist end, one bounded JXA watcher starts the exact next item only after authoritative Music.app `Stopped`; three transitions in a cloud-containing playlist measured a 0.18–0.32 second post-audio Apple Event/process gap on this host.
- Smart-playlist track persistent IDs can alias the current playable object's identifier. Playback remains exact, but strict ID-equality assertions are limited to normal user playlists in the live integration test.
- Some concrete user playlist objects reject property-array selectors even though their individual track objects are readable. The fallback reads only selected documented properties and is capped at 20 tracks per event-loop update.
- iTerm2 inline images accept the Music.app JPEG/PNG/GIF bytes directly. Kitty inline output uses source PNG directly and converts bounded JPEG source art to PNG once in a separate renderable cache; GIF remains a Unicode fallback. Sixel has detection but no encoder.
- Plain lyrics and favorites/ratings mutations are not implemented in this task. One guarded playlist operation is supported: after confirmation, remove the selected entry from an editable normal user playlist. It never deletes the library track; smart, folder, system/library, subscription, and unknown playlists remain read-only. The request carries the playlist ID, displayed entry index, and expected stable track ID so concurrent reordering fails safely instead of removing a different occurrence.
- Up Next and queue mutation remain unavailable because the installed public definition has no such surface.
- Optional Apple API authentication remains dormant and is not required for `--backend macos`.

## Live verification

The audit exercised the real installed Music.app, not only fixtures:

- 107 playlists discovered (user, smart, folder, and system classes present in the raw surface).
- 12,997 library tracks loaded progressively without blocking initial rendering.
- Real folder parent relationships were retained; `Country` expanded to indented child playlists, and a nested playlist opened through the standard detail/lazy-load route.
- Real playlist detail displayed non-placeholder titles, artists, durations, descriptions where present, and track counts.
- Opening an uncached 38-track playlist during the library scan displayed `Loading playlist…` immediately, rendered the first 20 tracks at `20 / 38`, then completed; leaving and reopening used retained contents immediately.
- A selected real playlist track published `1 / 38`; TUI Next changed to `2 / 38` without losing context, and an unrelated external Music.app selection removed the context while following the external track.
- Two consecutive accelerated natural ends advanced first→second→third without input. The selected real playlist contained cloud/subscription tracks and required no local path. Repeat Off was exercised live; Repeat All/One and race semantics are fixture-tested.
- Exact two-track album order passed the ignored live backend test and the TUI `P` action started the visible album's first derived track. Multi-disc ordering is fixture-tested; no suitable multi-disc album was manually selected during the final TUI run.
- Live album artwork extraction succeeded. The host terminal reported no supported image protocol, so the TUI displayed the bounded cached JPEG size and Unicode placeholder; iTerm2/Kitty command generation is covered by pure tests. A separate bounded read-only scan found a real no-artwork track among the first 500 items, confirming that Music.app's empty-artwork result reaches the missing/placeholder path.
- Local Recently Played displayed real played dates/counts in descending order and was visibly labeled `Local Music.app`.
- `j/k`, Enter, Escape/back, `/` search routing, and clean `q` shutdown were exercised in the TUI.
- Exact selected user-playlist track playback and exact playlist playback succeeded, then the test restored the captured track and playing/paused state.
- Normal `cargo test` does not contact Music.app; opt-in live tests are marked ignored.

## Practical loading measurements

Measurements are from the real 12,997-item library on 2026-08-03 using the unoptimized debug build. The prior 200-track implementation took approximately 55–60 seconds across 65 requests, with roughly 97 MiB resident after the complete derived library/search index.

Two warm isolated profiling passes produced the following ranges. `collection` is measured inside JXA; total includes process launch, Apple Events, stdout transport, and response return.

| Batch | Tracks | Total wall time | JXA collection | JSON serialization | Rust parse |
|---:|---:|---:|---:|---:|---:|
| 100 | 100 | 1.19–1.52 s | 421–424 ms | 0–1 ms | 1.3–2.2 ms |
| 200 | 200 | 1.22–1.64 s | 417–484 ms | 0–1 ms | 2.7–5.1 ms |
| 400 | 400 | 1.21–1.52 s | 438 ms | 1 ms | 5.4–8.5 ms |
| 500 | 500 | 1.19–2.48 s | 425–1,152 ms | 1 ms | 6.3–11.9 ms |

The production batch is 400: it halves the request count without the 500-item outlier and preserves progressive UI updates. In the final full run, backend start to completed reducer state was about **31.3 seconds** (33 library queries plus initial state and 107-playlist discovery), a roughly 43–48% reduction from 55–60 seconds. The 33 library queries spent 27.52 seconds in the automation/process path and 287.65 ms parsing. Final derivation took 112.94 ms: albums 64.84 ms, artists 33.58 ms, and recent views 14.44 ms. Search-index construction took 84.67 ms, and reducer merge/index work totaled 203.60 ms.

Playlist-specific live timings were measured twice. The historical implementation did not instrument Enter-to-first-track latency and showed an empty-looking detail until the first response; it could also let one ready background library query win scheduling (about 1.24 seconds in the same audit). The new loading indicator is reducer-local and appears on the next frame. Actual Music.app data latency remains transport-dependent:

| Playlist path | Tracks | First visible batch | Complete |
|---|---:|---:|---:|
| Property-array large | 12,997 | 1.25–1.68 s (400 tracks) | 29.17–37.23 s |
| Direct small | 7 | 3.80–3.81 s | 3.80–3.81 s |
| Selected-property fallback | 61 | 9.21–9.53 s (20 tracks) | 28.91–29.78 s |

The fallback is slower because it performs bounded per-track documented property reads, but partial rows are usable immediately and each continuation remains foreground while that playlist is open. For automatic playback, the original two-query implementation measured about 3.08–3.10 seconds from seeking to one second before the end until the next track appeared. Its one-shot successor could still return while Music.app was Playing and need another process after the end. The production watcher now starts near the expected end, checks the stable source identity once, observes `playerState` every 50 ms for a bounded 1–3 seconds, rechecks identity, and plays only after `Stopped`. The 2026-08-09 calibrated run against a cloud-containing playlist measured 178 ms, 283 ms, and 317 ms after the authoritative end. A 20 ms cadence did not improve those results, making approximately 0.2–0.3 seconds the observed lower bound on this host.

Visible-window rendering avoids constructing thousands of off-screen Ratatui rows. In the instrumented Songs-screen run, 120 debug frames averaged 7.46 ms and peaked at 13.52 ms while keyboard input remained responsive. Steady RSS after completion was approximately 93.1 MiB; `/usr/bin/time` recorded an instrumented maximum resident set size of 109.7 MiB and a 89.8 MiB peak-memory-footprint counter. No artwork was requested during the scan.

## Persistent cache

The backend stores a versioned JSON last-known metadata cache in the platform cache directory after a complete authoritative scan. It stores source library tracks plus playlist names, descriptions, kinds, counts, and parent IDs—not playlist contents, playback/queue state, synthesized sessions, or artwork bytes. On the next start it immediately rebuilds Songs, Artists, Albums, Recently Added, Recently Played, search, and the playlist hierarchy from that cache, then performs the same progressive Music.app refresh. Fresh batches update cached items by stable ID; only completion replaces the full cached track set, which removes deleted items without intermediate duplicates. Corrupt/unsupported cache files and failed writes are debug-logged and never stop local Music.app operation. The cache is last-known state only; no Music.app revision feed is assumed, and the completed scan remains authoritative.

## Manual refresh and cache controls

The status area distinguishes cached startup data, an authoritative refresh with
bounded batch progress, ready data, and a failed refresh that leaves the last
usable library visible. `R` requests a background refresh only when the current
scan is ready; a second request is coalesced with a concise notification.
Playback commands, playback sessions, playlist lazy loads, and artwork keep
their existing priority and are not reset by a refresh.

`apple-music-tui cache-status` reports the metadata cache path, schema, track
and playlist counts, modification timestamp, and readability. `cache-clear`
removes only that metadata file; a missing file is successful and no Music.app,
configuration, artwork, or session data is touched.

Playlist discovery remains authoritative. New/deleted playlists and folder
relationships reconcile with each scan, while already completed playlist
contents are retained for the running session rather than eagerly reloaded.

## Full-screen Now Playing

`N` opens a history-backed full-screen Now Playing route. It uses the existing
authoritative playback snapshot, synthesized context, and shared artwork cache;
it makes no additional Music.app query unless the current track has not already
started an artwork request. The regular mini-player is hidden while this route
is active. `Esc`, `h`, or `q` returns to the precise prior route and selection.

## Product boundary and next local work

The local backend is a strong playback-and-owned-library client, not a local
replacement for Apple Music's network product. It supports real Songs, derived
Artists/Albums/Recently Added, local Recently Played, playlist hierarchy and
contents, stable-ID playback, local search, contextual actions, and Now Playing.
Browse, Listen Now, Made for You, charts, genres, catalog search, and
recommendation parity require the Apple Music API; the local dictionary does
not expose them.

The recommended next work is local sorting/filtering, user-controlled refresh
and cache status, richer read-only metadata presentation, a full-screen Now
Playing route, and search workflow refinement. Playlist writes, ratings/favorite
writes, and Up Next remain deliberately absent: the first class lacks a safe
transactional/editability model, and the latter has no documented public
Music.app scripting surface. Plain lyrics deserve a separate read-only probe;
timed lyrics are not publicly available.

## Playlist mutation research (not exposed)

All probes used uniquely named temporary playlists and deleted them in success/error cleanup. No permanent user playlist was changed.

- JXA `make` returned Apple Event `-1708` (`Message not understood`) both with and without an explicit insertion location. Native AppleScript `make new user playlist` succeeded.
- Rename preserved the temporary playlist's persistent ID. Two same-name temporary playlists coexisted with distinct persistent IDs, confirming that names cannot be keys.
- Scripted delete removed the temporary playlist immediately with no Music.app confirmation. Any future TUI delete must therefore supply its own explicit confirmation.
- `duplicate` appended both an uploaded track and a subscription track by exact identity. Deleting track 1 from that temporary playlist reduced only its playlist count from two to one.
- Attempting to create a playlist with `smart:true` produced a normal playlist with `smart=false`; smart rule creation/editing is not available through this surface.
- `move track 2 ... before track 1` returned success but left the exact ID order unchanged. Reordering is unreliable and remains unavailable.
- Apple Events provide no transaction. A later failure can leave earlier writes applied; safe exposure requires editability/source checks, idempotent operations or compensating cleanup, stale-ID handling, and dedicated live tests. Smart/system/subscription playlists remain read-only.
