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
- [x] Progressively load real Music.app library tracks in 200-item batches with loaded/total UI state; live-verify all 12,997 local items.
- [x] Map rich local track metadata and correct `persistentID()`/`databaseID()` handling with explicit Music.app namespaces.
- [x] Derive deterministic albums, artists, and Recently Added from Music.app metadata.
- [x] Add a normalized cached local search index for tracks, artists, albums, and playlists; track entries include title, artist, album/album artist, composer, and genre (`/`).
- [x] Add exact identifier-based selected track, playlist-context track, and selected playlist (`P`) playback; live-verify normal user-playlist playback.
- [x] Add parser, derivation, search, batching, reducer, rendering, and opt-in ignored live tests; normal tests use no Music.app.
- [x] Document local source labeling, Automation consent, progressive loading, optional paid authentication, verified mechanisms, and unsupported operations.
- [ ] Add a dedicated Recently Played view from played-date/count metadata; the fields are parsed but no history semantics are claimed yet.
- [ ] Evaluate lazy artwork and legitimate plain-lyrics reads without blocking collection loading.
- [ ] Implement any playlist/favorite/rating mutation only after confirmation UI, editability/source checks, rollback/error handling, and dedicated live tests.
- [ ] Keep Up Next, Play Next/Later, queue removal, and reorder unavailable unless a documented public Music.app surface appears.

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

- [ ] Add album, artist, and playlist detail screens and context menus.
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
- [ ] Add non-blocking toasts, modal policy, status bar, and responsive layout polish.

## Milestone 10 — Artwork

- [ ] Add bounded artwork cache and async decoding.
- [ ] Detect Kitty, iTerm2, and Sixel support behind a renderer abstraction.
- [ ] Provide a Unicode/no-image fallback and keep the app fully usable without graphics.

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
