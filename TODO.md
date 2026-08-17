# Roadmap

The feature matrix is the source of truth for Apple capability assumptions. Items are checked only after implementation exists and relevant quality gates pass.

## Milestone 0 — Research and architecture

- [x] Inspect the initial repository and persistent engineering/product prompts.
- [x] Research current official Apple Music API, MusicKit, MusicKit on the Web, authentication, playback, queue, lyrics, artwork, and DRM boundaries.
- [x] Inspect the installed Music.app scripting definition and macOS Automation requirements.
- [x] Create `docs/FEATURE_MATRIX.md` with explicit support statuses.
- [x] Create `docs/ARCHITECTURE.md` with event, backend, auth, async, cache, configuration, UI, and error boundaries.
- [x] Record significant decisions in `docs/DEVELOPMENT.md`.

## Milestone 1 — Foundation

- [x] Initialize the minimal Rust stable project without overwriting repository files.
- [x] Add `README.md` and `.gitignore`; leave license selection explicit.
- [x] Implement safe terminal initialization/restoration, including panic restoration.
- [x] Implement a Tokio-driven input/backend event loop with no I/O in rendering.
- [x] Add domain track, artist, album, station, playlist, playback, queue, and explicit identifier models.
- [x] Add semantic actions, application state, reducer, commands, and backend events.
- [x] Add an explicit capability set and typed unsupported-feature failures.
- [x] Add a deterministic stateful `MockMusicBackend` with library/artist/album/station/playlist fixtures plus play, pause, selected-track play, toggle, next, previous, position, seek, shuffle, repeat, volume, favorite, and queue operations.
- [x] Add a functional section/detail application shell with explicit history; artist, album, and playlist detail views; responsive sidebar/content/queue layout; player bar; loading/error state; and help overlay.
- [x] Support `cargo run -- --backend mock`; reject unimplemented real backends honestly.
- [x] Add tests for every sidebar route, artist/album/playlist detail and Back navigation, selection validity, selected-song playback, quit state, input mapping, capability reporting, mock backend state, distinct screen rendering, detail metadata, and small terminals.
- [x] Pass formatting, Clippy with warnings denied, and all tests.

## Milestone 2 — macOS playback

- [x] Add isolated `backend/macos` module and Music.app availability detection.
- [x] Add a serialized asynchronous Apple Events worker; never call automation in the render loop.
- [x] Handle Automation consent denied/not determined with actionable diagnostics.
- [x] Poll dynamic playback state and a lightweight current-track fingerprint at 500 ms within a backend-configurable, worker-enforced 250–1000 ms range; reuse cached domain metadata only while robust identity remains unchanged.
- [x] Implement verified play, pause, toggle, next, previous, seek, volume, shuffle, and repeat controls; read mute state but keep mute mutation disabled after live changed-value writes returned Music.app error 9038.
- [x] Add current-track mapping with separate Music.app persistent/database ID namespaces and an explicit ephemeral fallback, without confusing Apple catalog or library IDs.
- [x] Add fixture-backed command/response parsing, conversion, capability, and error tests; keep the live Music.app test opt-in/ignored by default.
- [x] Evaluate ScriptingBridge/persistent helper versus bounded `osascript` execution and record the bounded JXA decision in `docs/DEVELOPMENT.md`.
- [x] Live-audit initial state, external track/state/seek changes, TUI play/pause/next/previous controls, subscription tracks without Music.app IDs, continuous event propagation, and clean shutdown against Music.app.
- [x] Correct Music.app JXA identifier acronym casing and add exact selected library/playlist-context track and playlist playback.

## Milestone 3 — Configuration, authentication, and doctor

- [x] Add typed Apple configuration at the XDG/macOS config path with safe missing defaults, field/path validation, home expansion, and secret redaction.
- [x] Generate and memory-cache bounded-lifetime ES256 Developer Tokens outside the render loop with a refresh safety margin.
- [x] Store, replace, retrieve, and delete Music User Tokens in macOS Keychain through a credential-store abstraction.
- [x] Implement a loopback-only MusicKit JS authorization bridge with an ephemeral port, origin-bound Developer Token, random nonce, bounded requests, callback integrity checks, and no browser persistence.
- [x] Add `auth`, `auth status`, `auth logout`, `doctor`, `config-path`, and `version` commands.
- [x] Diagnose OS, Music.app/Automation availability, config fields, private-key readability/signing, Keychain token presence, and Apple API authentication.
- [ ] Complete live Developer Token + MusicKit consent + Keychain + `/v1/me/storefront` verification with the project owner's Apple Developer credentials; normal automated tests intentionally use no real credentials, browser, Keychain, or network.
- [ ] Add `cache-clear` when the first persistent remote-data cache exists; Milestone 3 creates no cache to clear.

## Local Music.app expansion — no paid developer account

- [x] Reinspect the installed Music.app 1.6.5 scripting definition and live-test playlists, tracks, identifiers, metadata, batching, and selected-item lookup.
- [x] Evaluate ScriptingBridge, MediaLibrary, manual XML export, and Spotlight against the documented JXA/Apple Events surface; reject private databases/frameworks/endpoints.
- [x] Add explicit `LibraryRead`, `PlaylistRead`, and `SelectionPlayback` capabilities plus partial backend updates that do not replace the full library on playback polls.
- [x] Discover real visible Music.app playlists with user/smart/folder/subscription classification, descriptions, and parent IDs where available.
- [x] Lazy-load real playlist contents in bounded batches and handle concrete playlist objects that reject property-array selectors without requesting every property.
- [x] Progressively load real Music.app library tracks in profiled 400-item batches with loaded/total UI state; live-verify all 12,997 local items.
- [x] Map rich local track metadata and correct `persistentID()`/`databaseID()` handling with explicit Music.app namespaces.
- [x] Derive deterministic albums, artists, and Recently Added from Music.app metadata.
- [x] Add a normalized cached local search index for tracks, artists, albums, and playlists; track entries include title, artist, album/album artist, composer, and genre (`/`).
- [x] Add exact identifier-based selected track, playlist-context track, and selected playlist (`P`) playback; live-verify normal user-playlist playback.
- [x] Add parser, derivation, search, batching, reducer, rendering, and opt-in ignored live tests; normal tests use no Music.app.
- [x] Document local source labeling, Automation consent, progressive loading, optional paid authentication, verified mechanisms, and unsupported operations.
- [x] Preserve folder/parent playlist hierarchy by stable ID; render expandable/collapsible folders and lazily open nested playlists without name-based lookup.
- [x] Add exact whole-album playback in disc/track-number order using a synthesized Music.app session over stable track IDs; keep external Music.app state authoritative.
- [x] Add lazy, bounded, identity-deduplicated Music.app artwork retrieval for album detail and Now Playing, 16-entry source/renderable memory caches, iTerm2 inline output plus PNG-Kitty and one-time JPEG-to-PNG Kitty conversion with Ghostty and tmux-passthrough selection, and a Unicode fallback; never fetch artwork during the library scan.
- [x] Add a dedicated local Recently Played view derived from real played-date/count metadata and label it as local Music.app history rather than cloud Listen Now history.
- [x] Profile batch sizes and every major full-load phase; move to 400-track batches, defer derived indexes until completion, window large rendered collections, and live-verify a roughly 31-second full load.
- [x] Research playlist mutations with temporary cleanup probes; keep all write capabilities disabled because JXA creation, smart-playlist rules, atomic rollback, confirmation, and track reordering are not reliable enough as one safe adapter.
- [x] Replace playlist emptiness inference with explicit per-playlist `NotLoaded`/`Loading`/`PartiallyLoaded`/`Loaded`/`Empty`/`Error` states; render partial batches immediately and retain completed contents for the session.
- [x] Give user commands and open-playlist continuation precedence over the background library scan; use a fair serialized priority scheduler, coalesce duplicate background work, and load the first 40 playlist rows before 200-row continuation batches. Repeat live measurements when Automation is available.
- [x] Add synthesized stable-ID playlist playback contexts for selected tracks, including natural continuation, partial-load extension, next/previous, repeat semantics, backend-owned shuffle order, external-change cancellation, and cloud-track live verification.
- [x] Keep one bounded authoritative Music.app transition request alive near a playlist end, avoiding a second post-stop process round trip; three consecutive transitions in a cloud-containing playlist measured a 0.18–0.32 second post-audio gap.
- [x] Persist a versioned, atomic last-known local library/playlist-metadata cache; hydrate derived views/search immediately and reconcile it against each authoritative progressive Music.app scan without persisting playback, sessions, or artwork.
- [ ] Evaluate legitimate plain-lyrics reads without blocking collection loading.
- [x] Implement removal of one selected entry from an editable user playlist with confirmation, stable playlist ID plus occurrence validation, and no library-track deletion.
- [ ] Implement any additional playlist/favorite/rating mutation only after confirmation UI, editability/source checks, rollback/error handling, and dedicated live tests.
- [ ] Keep Up Next, Play Next/Later, queue removal, and reorder unavailable unless a documented public Music.app surface appears.

## Recommended next local-product roadmap — 2026-08-09

This ordered roadmap supersedes the stale local portions of later generic
milestones below. It deliberately leaves playback sessions, artwork, cache
architecture, Apple API/authentication, and playlist mutations unchanged.

### Milestone A — Sort, filter, and scan the local library

- [x] Implemented 2026-08-09: runtime-only stable-ID index views, modal sorting, interactive local filtering, selection preservation, and progressive-refresh reconciliation for Songs, Albums, and Artists.
- **Goal:** make 12,997-track local Songs/Albums/Artists views practical for daily selection.
- **Why:** the data is already available; finding it is the largest remaining local usability gap.
- **Tasks:** stable-ID sort specifications, local text/metadata filters, explicit active-sort/filter status, and bounded derived views without per-frame rebuilding.
- **Dependencies:** existing cached/authoritative collections and visible-row rendering.
- **Risk:** low-to-medium; preserve selection by stable ID through reorder/refresh.
- **Acceptance:** sort/filter Songs, Albums, and Artists while refresh runs; no duplicates, stale selection, or playback regression.

### Milestone B — Library refresh and cache-status controls

- [x] Implemented 2026-08-09: coalesced background `R` refresh, explicit cached/refreshing/ready/failed presentation, CLI cache inspection/clear, and playlist metadata reconciliation that retains completed session-cached playlist contents.
- **Goal:** make last-known versus authoritative library state understandable and controllable.
- **Why:** warm startup is fast, but users need a safe explicit refresh and actionable status.
- **Tasks:** semantic refresh action, coalescing while a scan is running, clear Cached/Refreshing/Ready/error messaging, and a non-destructive cache diagnostic/clear policy only after UX design.
- **Dependencies:** existing progressive scan and atomic cache.
- **Risk:** medium; automation priority must continue to favor playback and foreground playlist loads.
- **Acceptance:** manual refresh never blocks navigation/playback, cannot create parallel scans, and clearly reports the source/state transition.

### Milestone C — Metadata and detail usability

- **Goal:** surface already-read local metadata where it helps decisions.
- **Why:** rating/favorite read state, cloud status, dates, counts, genre, and media kind exist but are selectively visible.
- **Tasks:** compact metadata sections and consistent empty values for song/album/artist/playlist details; keep all values read-only.
- **Dependencies:** parsed metadata and existing detail routes.
- **Risk:** low; avoid dense, flickering lists and do not add mutation controls.
- **Acceptance:** representative local/cloud/smart items present truthful metadata without exposing write actions.

### Milestone D — Dedicated full-screen Now Playing

- [x] Implemented 2026-08-09: history-backed `N` route with responsive artwork, authoritative playback/context/progress presentation, and mini-player suppression.
- **Goal:** provide a focused playback view using existing authoritative state and artwork.
- **Why:** it is a high-value daily-use view that needs no new backend capability.
- **Tasks:** route, responsive layout, keyboard return path, cache/artwork reuse, and explicit unavailable-artwork states.
- **Dependencies:** current player/artwork presentation only.
- **Risk:** medium UI regression risk; no changes to poll/session/render protocols.
- **Acceptance:** external Music.app changes update the view, small terminals degrade safely, and no artwork is re-requested per poll.

### Milestone E — Search workflow refinement

- **Goal:** make the immediate local search index easier to use in large libraries.
- **Why:** search is already fast and cached; scope/result controls are more valuable than remote search.
- **Tasks:** typed result scopes, predictable edit/submit/clear behavior, result counts, and cached-vs-refresh messaging.
- **Dependencies:** existing normalized index and action menu.
- **Risk:** low; preserve current key routing and stable result identities.
- **Acceptance:** filtering works during refresh, keyboard flow is discoverable, and all actions retain their current targets.

### Deferred research — do not expose without new evidence

- Plain local lyrics: research only; expose only if public `lyrics` returns reliably and safely.
- Music videos: classify/read local media kinds first; do not claim playback/video parity.
- Theme, keybinding, mouse, settings, CLI/release packaging: lower value than library control; scope independently.
- Playlist/favorite/rating writes: intentionally deferred for confirmation, editability, rollback, and live-verification design.
- Queue/Up Next: unavailable through local Music.app; revisit only if a documented public surface appears.

## Release readiness

- [x] Add release build verification, package metadata, CI quality gates, doctor cache/path diagnostics, installation guidance, repository secret ignores, and a release checklist.
- [ ] Select a license before publishing; Cargo metadata intentionally has no invented license value.
- [ ] Establish and review a clean tracked Git baseline before tagging or distributing a release.

## Milestone 4 — Catalog and search

- [ ] Add the asynchronous Apple Music HTTP client with typed errors and redacted headers.
- [ ] Keep Apple DTOs separate from domain models and add fixture tests.
- [ ] Implement storefront-aware catalog search for songs, albums, artists, playlists, and stations.
- [ ] Implement returned-link pagination and viewport/lazy loading.
- [ ] Add approximately 300 ms search debounce and generation-based stale response rejection.
- [ ] Add offline/cache-aware search states.

## Milestone 5 — Optional Apple Music API library

- [ ] Implement paginated remote/account library songs, albums, artists, and playlists without replacing the working local Music.app views.
- [ ] Implement Recently Added and Recently Played using documented endpoints.
- [ ] Add sorting/filtering without rebuilding or cloning large collections per frame.
- [ ] Exercise 50,000-track fixtures and measure startup/scroll behavior.

## Milestone 6 — Details and supported mutations

- [x] Add album, artist, and playlist detail screens and stable-ID context menus.
- [ ] Implement playlist creation and append-track operations through Apple Music API.
- [ ] Implement favorites and like/dislike ratings with optimistic rollback.
- [ ] Expose rename/delete/remove only for a backend that reports and verifies those capabilities.
- [ ] Keep arbitrary playlist reorder disabled until Apple documents a reliable route.

## Milestone 7 — Discovery and radio

- [ ] Compose Listen Now from recommendations/history with honest partial-parity labeling.
- [ ] Compose Browse from charts, genres, activities, curators, and catalog playlists.
- [ ] Add charts, genres, live stations, station genres, and personal station metadata.
- [ ] Route station playback through an authorized playback backend.

## Milestone 8 — Queue

- [ ] Add queue and history UI using backend-reported capabilities.
- [ ] Evaluate a documented signed MusicKit `SystemMusicPlayer` helper for queue access.
- [ ] Implement Play Next/Play Later only where supported.
- [ ] Keep Music.app AppleScript Up Next read/reorder/remove unavailable unless its public scripting definition changes.

## Milestone 9 — UI refinement

- [ ] Add configurable keybindings and generate help from the active keymap.
- [ ] Add default, dark, light, and Dracula semantic themes.
- [ ] Add command palette with parsing, validation, and fuzzy matching.
- [x] Add modal policy, status presentation, responsive layout polish, and contextual Help; non-blocking toast work remains separate.

## Milestone 10 — Artwork

- [x] Add bounded artwork cache and async decoding.
- [x] Detect Kitty, iTerm2, and Sixel support behind a renderer abstraction.
- [x] Provide a Unicode/no-image fallback and keep the app fully usable without graphics.

## Milestone 11 — Lyrics

- [ ] Read legitimate plain lyrics from Music.app only when the scripting backend returns them.
- [ ] Keep a provider abstraction for any future official/authorized source.
- [ ] Do not scrape lyric sites or present `hasLyrics` as actual lyric text.
- [ ] Keep synchronized lyrics unavailable until a documented payload API exists.

## Milestone 12 — Production hardening

- [ ] Add bounded retry/backoff, offline behavior, cache invalidation, and migrations.
- [ ] Add structured file logging with mandatory secret redaction.
- [ ] Complete performance, accessibility, terminal compatibility, and panic-safety testing.
- [ ] Add packaging, release artifacts, contributor docs, and platform support policy.
- [ ] Select a project license with the owner before publishing.
