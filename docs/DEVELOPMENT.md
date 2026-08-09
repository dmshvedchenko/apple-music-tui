# Development Decisions

This file records durable engineering decisions and their rationale. It is not a session log.

## D001 — Capability-oriented composite backend

**Decision:** Model backend support as explicit capabilities and route each capability independently through `CompositeMusicBackend`.

**Rationale:** No documented Apple surface provides desktop parity. Music.app is strong for local playback, while Apple Music API is strong for catalog and personalized metadata. Explicit routing prevents fake success and keeps reduced-capability builds honest.

## D002 — Music.app through documented automation only

**Decision:** Use the public Music.app scripting definition/Apple Events for the first macOS playback backend. Keep MusicKit `SystemMusicPlayer` behind a future signed-helper evaluation. Do not depend on MediaRemote or private frameworks.

**Rationale:** The scripting definition directly exposes the needed foundation controls and is available with the operating system. A native MusicKit helper may improve queue access but adds Swift build, code-signing, entitlement, and IPC complexity.

## D003 — REST for data, Apple-managed surfaces for protected playback

**Decision:** Never treat Apple Music API play parameters or preview URLs as general full-track streams. Route subscription playback through Music.app or another documented MusicKit player.

**Rationale:** The REST API exposes metadata and playback parameters, while full playback is authorization-, subscription-, and DRM-managed by Apple playback surfaces.

## D004 — Browser or native bridge for Music User Token consent

**Decision:** Personalized HTTP features use a documented MusicKit consent flow implemented as a localhost MusicKit-on-the-Web bootstrap, with a signed native helper retained as a future alternative. Store the resulting secret in Keychain.

**Rationale:** Apple automatically manages Music User Tokens on Apple platforms and the web; a pure terminal process has no documented direct sign-in endpoint. The bridge preserves explicit consent without scraping cookies or private endpoints.

## D005 — Mock backend is a stateful reference implementation

**Decision:** Build Milestone 1 against deterministic stateful mock data and run it through the same command/event boundary used by production backends.

**Rationale:** This makes the TUI testable in CI and prevents architectural shortcuts that would couple rendering to AppleScript or HTTP.

## D006 — License remains undecided

**Decision:** Do not create a `LICENSE` file until the project owner selects one.

**Rationale:** License selection is a product/legal decision and should not be silently inferred.

## D007 — Bounded JXA subprocesses for the first Music.app backend

**Decision:** Implement Music.app Apple Events with serialized, bounded `/usr/bin/osascript -l JavaScript` calls behind `src/backend/macos/`. Use structured JSON responses, fixed script source with typed numeric parameters, a ten-second process timeout, and `spawn_blocking`. Query full state immediately, then poll dynamic state and a lightweight track fingerprint at 500 ms. Reuse the cached domain track only while that fingerprint is unchanged.

**Rationale:** This uses the locally installed public scripting definition, requires no private frameworks or signed helper, and keeps blocking automation outside rendering and Tokio executor threads. The automation-runner abstraction and backend trait allow a persistent ScriptingBridge or MusicKit helper to replace the process transport later without changing the reducer or UI.

## D008 — Music.app capabilities follow changed-value verification

**Decision:** The macOS backend exposes launch, playback transport, seek, volume, shuffle, repeat, local library/playlist/history reads, lazy artwork, local search, and identifier-based selected track/playlist/album playback. It does not expose queue, favorite mutation, playlist mutation, or mute mutation. Persistent and database Music.app IDs occupy distinct `musicapp:persistent:` and `musicapp:database:` namespaces; an explicitly ephemeral metadata-derived identity is used when streamed items fail to return both.

**Rationale:** The local Music.app 1.6.5 dictionary and live tests verified the exposed controls, collection reads, lazy artwork, and exact playback commands. It contains no Up Next surface; contained playlist write probes did not establish a consistent safe adapter; favorite writes were not exercised; and changed-value mute writes returned Music.app error 9038. Capability flags represent observed behavior, not merely declarations in the scripting dictionary.

## D009 — Music.app identity must tolerate absent and placeholder IDs

**Decision:** Normalize Music.app identifiers before choosing an identity. Reject null-like strings, zero, and minus one; use persistent ID first, then database ID, then a composite of title, artist, album, and rounded duration. Carry those metadata fields on every hot poll, but never include playback position. Seed the backend's identity/cache from the initial full query.

**Rationale:** Live subscription-backed and local current-track queries returned no usable persistent or database ID. The earlier JXA path could stringify missing values into stable placeholder IDs, while its fallback omitted artist and album. That made distinct tracks appear identical and left cached metadata stale even as dynamic state continued to poll. The complete metadata fallback and cache seeding keep startup, track changes, and position reconciliation independent and observable.

## D010 — Generate Developer Tokens in memory

**Decision:** Generate Apple Developer Tokens from the configured Team ID, Key ID, and external `.p8` using maintained ES256 primitives. Use a 30-day lifetime, cache per optional origin in process memory, and refresh five minutes before expiry. Never persist or log a signed token.

**Rationale:** Apple permits ES256 tokens up to six months, but a shorter lifetime limits exposure while avoiding per-request signing. Origin-bound tokens reduce the scope of the browser authorization helper without coupling API request tokens to a web origin.

## D011 — MusicKit JS loopback consent and Keychain storage

**Decision:** Implement `auth` as a temporary, framework-free MusicKit JS page on an ephemeral `127.0.0.1` port. Require an exact loopback peer, host, origin, nonce-bearing path/header, JSON content type, and bounded payload. Do not use browser persistence. Verify the returned token with `/v1/me/storefront` before replacing the macOS Keychain item.

**Rationale:** Official MusicKit automatically obtains Music User Tokens only within supported Apple-platform or web consent surfaces; Apple documents no direct CLI OAuth/token endpoint. The loopback bridge retains explicit Apple consent and keeps the bearer credential out of config, shell history, logs, and remote callback hosts.

## D012 — Treat Music User Tokens as opaque

**Decision:** Do not assign a locally guessed expiry or refresh scheme to a Music User Token. Retrieve it from Keychain, validate it when an authenticated session is needed, and require a new `auth` flow after Apple returns an authorization rejection.

**Rationale:** Apple's current public documentation does not provide a stable lifetime or refresh-token contract for applications to reproduce. Inventing one would create false authentication state and potentially retain revoked credentials.

## D013 — Local Music.app is the primary no-paid-account data backend

**Decision:** Use documented Music.app Apple Events for local playlists, lazy playlist contents, progressively batched library songs, rich metadata, derived albums/artists/Recently Added, local search, and exact selected-item playback. Keep the paid Apple API authentication path optional/dormant and do not gate local features on it.

**Rationale:** The installed Music.app dictionary and live Music.app 1.6.5 probes expose enough real local/cloud-aware library state to make the TUI useful without Apple Developer Program membership. ScriptingBridge exposes the same dictionary rather than a richer surface; MediaLibrary and Spotlight are file-oriented and omit protected/cloud playlist semantics; XML export is manual and stale. Private databases, private frameworks, and undocumented endpoints are prohibited.

## D014 — Progressive collection updates are separate from playback snapshots

**Decision:** Emit typed partial backend updates for playback, playlist discovery, library batches, playlist batches, and artwork. Load 400 library tracks per JXA property-array request; lazy-load playlist tracks when their detail route opens. Derive grouping once after the final library batch. Maintain normalized typed search-index entries incrementally during loading and rebuild once after artist/album derivation. Drain subprocess pipes concurrently.

**Rationale:** Replacing or cloning a 12,997-track snapshot every 500 ms would make playback polling scale with library size. Partial events keep rendering responsive and preserve Music.app as the playback source of truth. Cached normalized search avoids rescanning raw collection fields on every input event. Concurrent pipe draining avoids the deadlock found when rich JSON exceeded the OS stdout pipe capacity.

## D015 — Selected playback requires exact context and identifiers

**Decision:** Use Music.app persistent/database IDs, never names, for playback selection. Commands from playlist detail carry the playlist ID, currently ordered stable track IDs, selected index, and whether loading is complete; library/album/artist/search commands use the main library context. Whole-playlist playback uses its persistent ID.

**Rationale:** Live testing found valid user-playlist tracks that resolve within their playlist but not through the Music library playlist. Explicit context makes playback safe and deterministic without private IDs or ambiguous metadata matching.

## D016 — Playlist hierarchy is an ID tree, not a formatted flat list

**Decision:** Preserve `PlaylistKind::Folder` and `parent_id`, derive a `PlaylistHierarchy` keyed only by `PlaylistId`, and store expanded folder IDs separately in `AppState`. Flatten only the currently visible rows for selection/rendering. Folder Enter toggles expansion; non-folder Enter uses the normal playlist detail route and lazy-load command.

**Rationale:** Display names are neither unique nor stable. Keeping structure and expansion independent makes nested duplicates safe, preserves selection by ID across expansion/collapse, and keeps playlist detail rendering outside the sidebar.

## D017 — Whole-album playback is an exact synthesized session

**Decision:** Advertise `AlbumPlayback` for mock and macOS backends. Album grouping includes normalized album artist, title, and year, and orders tracks by disc number, track number, title, and stable ID. Music.app playback uses one exact stable track selector at a time with `once: true`; a backend session advances after the expected track stops and owns TUI next/previous until an external track change cancels it.

**Rationale:** Live Music.app rejected an array of track specifiers, while exact single-track `once` playback worked. The synthesized session avoids temporary playlists and title interpolation, supports multi-disc ordering, and lets every poll reconcile with Music.app as the source of truth. Transition latency includes Apple Event/process transport in addition to the 500 ms scheduling interval and is documented from live measurement rather than inferred from the timer alone.

## D018 — Artwork is lazy, bounded, identity-deduplicated, and optional

**Decision:** Read the first Music.app artwork descriptor lazily for album detail and Now Playing. Decode validated hexadecimal bytes with a 2 MiB extraction limit, cache at most 16 entries by stable identity, and cap terminal-inline payloads at 512 KiB. Emit supported formats through iTerm2 and PNG through Kitty/ Ghostty. Direct Ghostty is identified by `TERM_PROGRAM=ghostty`; known or explicitly forced Kitty output inside tmux is wrapped in tmux passthrough. When tmux hides the outer terminal, `APPLE_MUSIC_TUI_ARTWORK_RENDERER=auto|kitty|unicode` is an explicit safe selection. Use a Unicode placeholder for Sixel, unsupported Kitty formats, missing/invalid/oversized artwork, or small terminals.

**Rationale:** Live extraction was reliable (a representative JPEG was roughly 388 KiB and returned in tens to hundreds of milliseconds), but eager collection-wide reads would multiply Apple Events and memory. Raw terminal protocols do not uniformly accept JPEG/GIF, and adding synchronous image transcoding would increase startup/runtime cost. The fallback keeps navigation and playback independent of image support.

## D019 — Recently Played means local played-date metadata only

**Decision:** Derive a dedicated local view only from tracks with a real Music.app played date, deduplicate by stable `TrackId`, keep the newest metadata entry, and sort descending by played date. Retain compact display data rather than another full `Track` clone and label every row `Local Music.app`.

**Rationale:** `playedDate` and `playedCount` were reliable enough for a recent local-library view, but they are not an event log and do not equal Apple cloud Listen Now history. Explicit naming avoids overstating parity while preserving useful local history.

## D020 — Optimize the measured Apple Event and render bottlenecks

**Decision:** Use 400-track batches, defer album/artist/recent/search derivation to the final batch, instrument Apple Event/process time, parsing, conversion, reducer merge, grouping, search indexing, and frames, and build Ratatui list rows only for the visible window.

**Rationale:** Warm 100/200/400/500 probes showed JXA collection around 417–439 ms and JSON serialization around 0–1 ms, but wall time around 1.19–1.22 s: Apple Event/process transport dominates, not Rust parsing. On 12,997 real items, 400-track loading reached ready state in about 31.3 seconds instead of 55–60 seconds. Final grouping was about 113 ms, search indexing about 85 ms, reducer work about 204 ms total, and visible-window debug frames averaged about 7.5 ms. A 500 batch gave no consistent transport advantage and increases per-event work.

## D021 — No persistent local cache or playlist mutation capabilities yet

**Decision:** Keep Music.app metadata and artwork in memory for this milestone. Do not add SQLite or expose create/rename/delete/add/remove/reorder capabilities. Record mutation probes, but require a future dedicated adapter with explicit editable-source checks, confirmation, idempotency/compensating cleanup, and integration tests before capability publication.

**Rationale:** Music.app exposes no cheap authoritative whole-library revision/change feed, so a persistent cache cannot yet meet safe invalidation requirements. The optimized progressive scan is roughly 31 seconds and immediately usable while loading. Mutation probes were inconsistent across terminology: AppleScript create/rename/delete and duplicate/remove worked on temporary playlists, JXA `make` returned `-1708`, smart creation silently produced a regular playlist, delete showed no native confirmation, writes are not transactional, and a track `move` command reported success without changing order.

## D026 — Last-known local library cache is reconciled, never trusted

**Decision:** Persist only complete authoritative source tracks and playlist metadata in versioned JSON, atomically via a synced same-directory temporary file and rename. On startup, rebuild derived views and search from that cache off the render path, retain it while a 400-item authoritative refresh runs, upsert incoming tracks by stable ID, and replace the complete set only at refresh completion. Do not persist artwork, current playback, queue, playlist contents, or synthesized playback sessions.

**Rationale:** The measured 31-second scan makes a last-known UI valuable even without a Music.app collection revision feed. Stable-ID reconciliation makes changed/new entries usable as they arrive and removes deleted entries once the authoritative result is complete, without presenting duplicates. JSON is sufficient for one bounded metadata snapshot; corruption, an unsupported schema, and write failures safely fall back to normal live loading rather than blocking Music.app operation.

## D027 — Prioritize local library control before optional remote parity

**Decision:** The next product milestones focus on local sorting/filtering,
manual refresh/status, richer read-only metadata, full-screen Now Playing, and
search workflow refinement. Do not start Apple Music API discovery, playlist
mutation, queue emulation, or playback/session rewrites as part of that work.

**Rationale:** The local backend already supplies authoritative playback and a
large real library, while the highest daily-use gaps are finding and
understanding items. Browse, Listen Now, recommendations, charts, catalog
search, quality variants, and cloud history require an optional Apple API
surface. Playlist/rating/favorite writes and Up Next are either unsafe or absent
from the installed public Music.app dictionary, so treating them as routine UI
polish would create a misleading product contract.

## D028 — Collection sort/filter state is runtime-only index data

**Decision:** Keep Songs, Albums, and Artists in authoritative source vectors.
Build cached ordered index views only when collection data, a sort choice, or a
filter changes; preserve selection by `TrackId`, `AlbumId`, or `ArtistId`.
Do not persist sort/filter preferences or add them to the library metadata cache.

**Rationale:** This avoids cloning a large library or sorting it during render,
keeps cache reconciliation authoritative, and makes warm cached collections
immediately usable. Preference persistence needs a separate UI-preferences
boundary; storing it in the last-known library cache would conflate user UI
state with Music.app metadata.

## D029 — Refresh is one coalesced background scan over retained state

**Decision:** `R` starts the existing phase-driven Music.app discovery/library
scan only when it is ready. The UI receives explicit refresh-start/failure
events, retains its cached/visible source collections and runtime sort/filter
views, and accepts progressive stable-ID reconciliation exactly as at startup.
Repeated requests while discovery or scanning is active return a notice instead
of launching a second scan. Completed playlist contents remain session-cached
when playlist metadata/hierarchy is reconciled.

**Rationale:** A second full scan would compete with bounded Apple Events and
would make status misleading. Reusing the established background phase keeps
playback, playback sessions, artwork, and foreground playlist loads untouched.
The CLI cache controls inspect or delete only the atomic metadata snapshot, so
they cannot alter Music.app or runtime playback state.

## D030 — Full-screen Now Playing is a history route over existing state

**Decision:** Model full-screen Now Playing as a non-sidebar `Route::NowPlaying`
opened by `N`, not a new backend mode or playback model. It renders the current
authoritative snapshot and shared artwork placement after Ratatui has allocated
its final rectangle, while hiding the mini-player. Closing restores the saved
navigation entry.

**Rationale:** This keeps external Music.app changes, synthesized session
context, progress, and existing artwork deduplication as single sources of
truth. It avoids duplicate artwork requests, a second progress timer, and any
new automation work in the render path.

## D022 — Playlist contents have explicit progressive state and foreground priority

**Decision:** Store `PlaylistLoadState` independently from the track vector with `NotLoaded`, `Loading`, `PartiallyLoaded`, `Loaded`, `Empty`, and `Error`. Opening changes state before I/O, the first batch executes as a foreground command, subsequent batches stay ahead of library scanning, and terminal completed contents remain session-cached. The backend worker uses a biased command branch; it does not attempt to cancel an Apple Event already in flight.

**Rationale:** An empty vector cannot distinguish an unopened playlist, a slow request, a genuinely empty result, or failure. Live tests also showed two transport regimes: property-array playlists return 400 tracks quickly, while some concrete playlists require a bounded 20-track selected-property fallback. Explicit progress keeps both interactive and makes the scheduling guarantee testable.

## D023 — Selected playlist-track playback is a synthesized stable-ID session

**Decision:** Generalize album playback into a backend `PlaybackSession` whose domain projection is `PlaybackContext::Playlist` or `Album`. A playlist session records ordered IDs, selected index, completeness, expected transition, and optional backend-owned shuffled order. Exact playlist tracks play with `once: true`. Natural completion requires an expected previous Playing track near its duration followed by Stopped; a missing stopped `currentTrack` is accepted only through that correlation. Near the end, one typed JXA request verifies the source identity, watches `playerState` for a bounded 1–3 seconds, and starts the preselected exact next item only after `Stopped`. Later playlist batches extend an incomplete session. Repeat One/All/Off are applied to the synthesized order; changing shuffle cancels the context and requests a restart.

**Rationale:** Live selected-track playback stopped after one item because Music.app received only one exact track object and the backend retained no playlist order. Music.app also clears `currentTrack` after some exact `once` completions. The explicit session fixes both without pretending the scripting dictionary exposes Up Next. A one-shot near-end check could return `Playing` just before completion, forcing another full `osascript`/Apple Event request after audio ended. The bounded watcher avoids that second request without truncating audio or treating time as completion. On 2026-08-09, three real transitions in a cloud-containing playlist measured 178 ms, 283 ms, and 317 ms after the authoritative end; a 20 ms watcher cadence did not improve on the 50 ms cadence, so this host's practical lower bound is roughly 0.2–0.3 seconds.
