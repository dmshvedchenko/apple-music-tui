# Architecture

## Goals

`apple-music-tui` is a keyboard-first terminal client that combines only documented Apple integrations. It must start and remain useful when a network, Apple account, or platform-specific backend is unavailable. It must compile on non-macOS systems with reduced capabilities.

The architecture separates UI, application state, domain models, backend routing, Apple integrations, and infrastructure. No Ratatui widget performs I/O and no backend renders terminal content.

## Component boundaries

```text
Terminal input
    │
    ▼
semantic Action ───────► reducer ───────► AppState ───────► Ratatui render
                            │
                            ▼
                         Command
                            │
                            ▼
                    async backend worker
                            │
                            ▼
                       BackendEvent
                            │
                            └────────────► reducer
```

The dependency direction is:

```text
UI ─► Application ─► Domain
          │             ▲
          ▼             │
       Backend ─► Infrastructure
```

- `domain`: stable product models and explicit identifier types; no Ratatui, HTTP, AppleScript, or persistence types.
- `app`: state, semantic actions, reducer, commands, and backend events.
- `ui`: pure rendering from immutable application state.
- `input`: terminal events to semantic actions using the active keymap.
- `backend`: capability contracts, concrete backends, and composite routing.
- `infrastructure`: HTTP DTOs/client, Apple Events worker, auth, Keychain, cache, configuration, and logging.

## Application state

Application state is data only. It includes navigation, selection, active screen, paginated view data, playback, queue, loading/error states, backend/auth/network status, notifications, modal state, and semantic theme values.

Navigation uses typed routes and an explicit history stack. Detail routes carry stable domain identifiers (for example, a playlist ID), while history entries preserve the prior route and content selection so Back restores the previous view deterministically.

Long collections are stored once and rendered by viewport. Widgets and terminal frame objects never live in state. Loading views use explicit `NotStarted`, `Loading { loaded, total }`, `Loaded { total }`, and `Error` collection states alongside the general view state. `BackendUpdate` distinguishes playback-only updates from playlist replacement, library batches, and playlist-track batches; a 500 ms playback poll therefore never replaces the full library.

## Actions, commands, and events

Actions express intent without naming a backend:

```text
MoveDown
OpenSelected
PlayPause
SearchChanged(query)
OpenQueue
Backend(event)
```

The reducer is deterministic: `(AppState, Action) -> (AppState, [Command])`. It may update state optimistically, but it never performs I/O. Commands carry asynchronous work to services. Services return typed backend events which re-enter the reducer.

Request generations or cancellation tokens protect asynchronous search and paginated views from stale responses. The reducer accepts a response only if its generation matches the active request.

## Domain model and identifiers

Core types include `Track`, `Album`, `Artist`, `Playlist`, `PlaybackSnapshot`, `QueueItem`, `SearchResults`, and paginated collections.

Identifiers remain explicit because Apple uses different namespaces:

```text
CatalogTrackId
LibraryTrackId
MusicAppPersistentId
MusicAppDatabaseId
QueueEntryId
```

Apple Music API DTOs are decoded under the API module and converted into domain models. DTO fields never leak into rendering code.

## Backend contracts

Backends expose narrow capability-oriented traits rather than one assumed-complete provider. Planned contracts include playback, volume, search, catalog, library, playlist, favorites/ratings, recommendations, radio, queue, and lyrics.

Each backend publishes a `Capabilities` set. Unsupported operations return a typed `UnsupportedFeature` error and are disabled or hidden in the UI. A mock backend is a production-quality development backend, not a fake-success layer: state-changing methods mutate deterministic state and emit observable updates.

## CompositeMusicBackend

`CompositeMusicBackend` selects a provider per capability and records the route in diagnostics. The expected macOS route is:

| Capability | Preferred provider | Fallback |
|---|---|---|
| Playback, position, volume, shuffle, repeat | Music.app integration | Mock in demo/test only |
| Current item metadata | Music.app integration | Apple API lookup by known catalog ID |
| Catalog search | Apple Music API when optionally configured | Unsupported/cached data |
| Local library search | In-memory Music.app-derived index | Mock data in demo/test |
| Library and playlists | Music.app documented local reads | Optional Apple Music API for remote/account semantics |
| Recommendations, charts, genres, stations | Apple Music API | Cached data |
| Queue | Documented MusicKit helper if adopted | Unsupported for Music.app AppleScript; mock for tests |
| Plain lyrics | Music.app when it returns legitimate text | Provider abstraction; otherwise unsupported |

Routing is decided at startup and may degrade when authorization or connectivity changes. Backend failures update status and notifications; they do not terminate unrelated capabilities.

## macOS Music.app integration

macOS-specific implementation belongs under `src/backend/macos/` (or an equivalent isolated crate/module) and is compiled behind a single platform boundary.

The implemented production approach is the generic serialized backend worker plus bounded `/usr/bin/osascript -l JavaScript` subprocesses using the public Music.app scripting dictionary. Blocking process work runs through `spawn_blocking`; scripts return JSON, use fixed source plus typed values, and are invoked without a shell. Each process has a ten-second timeout. Stdout and stderr are drained concurrently while the timeout is enforced so a rich collection response cannot deadlock on an OS pipe buffer. This keeps Apple Events and process waits out of both the renderer and async executor threads while preserving the existing backend trait boundary.

The backend performs a playback state query immediately at startup, then polls at 500 ms; the worker clamps backend intervals to 250–1000 ms. It bulk-discovers playlist metadata, then reads the Music.app library in bounded 200-track property-array batches. Opening an unloaded playlist queues a lazy playlist-track batch ahead of the background library scan. Collection responses carry the current playback snapshot so Music.app remains authoritative throughout the scan. After the final library batch, deterministic grouping derives albums, artists, and Recently Added. Application state maintains normalized search entries as collection batches arrive, then performs one final rebuild after derivation; keypresses filter this cache instead of rescanning raw Music.app objects. Results retain typed track, artist, album, or playlist identities and route through the normal detail/playback actions.

Polls always read state, position, volume/mute, shuffle/repeat, and lightweight current-track identity metadata: title, artist, album, duration, persistent ID, and database ID. Position is applied independently on every update. The optional favorite property is omitted from hot polling. JXA acronym spelling follows the installed dictionary exactly: `persistentID()` and `databaseID()`.

Music.app persistent and database IDs are normalized before use, and placeholder values such as null, `missing value`, zero, and minus one are rejected. Valid IDs use distinct `musicapp:persistent:` and `musicapp:database:` domain namespaces. When neither ID is usable, identity falls back to a composite of title, artist, album, and rounded duration—never playback position. This fingerprint is included in every poll because real subscription-backed tracks can lack both IDs. An unchanged fingerprint reuses the cached domain `Track`; a change immediately rebuilds it from the poll metadata, preventing stale title/artist/album state without scanning the static library.

Installation absence, Music.app not running, Automation denial, unknown player states, worker/channel failure, and other Apple Event failures are represented as backend availability states. They update the status line rather than pretending the backend remains connected. Polling continues after recoverable Music.app query failures, so a later explicit `OpenPlayer` action, permission correction, or manual Music.app launch can recover without restarting the TUI.

The installed Music.app scripting definition documents play, pause, next, previous, seek, volume/mute, shuffle, repeat, current track, playlist/library objects, ratings, favorites, and plain lyrics. Read capabilities now include real playlists, hierarchy/classification, playlist contents, library tracks, rich metadata, and exact identifier-based track/playlist playback. Playlist-context commands are distinct because some playlist track objects are addressable inside their playlist but not through the main library playlist. It does not expose the Up Next queue. Changed-value mute writes returned error 9038 locally, so macOS mute mutation is disabled despite the dictionary declaration. Favorites, ratings, and destructive playlist/library mutations remain disabled until they receive confirmation UX and dedicated integration coverage. The backend does not represent `current playlist` as the Up Next queue.

macOS Automation consent denial is a normal capability failure with recovery guidance. Packaged hardened binaries require appropriate Apple Events usage metadata/entitlements. ScriptingBridge or a small signed Swift helper can later replace subprocess execution without changing application or UI layers.

`SystemMusicPlayer` is a documented MusicKit surface that controls Music.app and exposes a queue. A native helper is a future evaluation item because it adds signing, entitlement, build-toolchain, and IPC requirements; it is not assumed available in the Rust foundation.

## Apple Music API integration

The HTTP backend uses `reqwest` asynchronously. HTTP response DTOs are separate from domain models. Requests use pagination links returned by Apple rather than synthesizing offsets. Catalog requests require a developer token; personalized `/v1/me` requests also require a Music User Token.

The API backend covers catalog/search, library reads, recently added/played, recommendations, charts, genres, station metadata, favorites/ratings, playlist creation, and appending tracks. Missing documented mutations—such as arbitrary playlist reordering—remain unsupported.

The REST API does not supply reusable full-track audio streams. Play parameters are handed only to documented Apple playback surfaces.

## Authentication and secrets

Configuration identifies a Team ID, Key ID, optional storefront, and a path to a `.p8` file. The Media ID association is configured in Apple Developer rather than duplicated locally. Private key contents and tokens never appear in logs, diagnostics output, cache keys, panic messages, or repository files. Secret tokens use a wrapper whose `Debug` and `Display` implementations always redact and whose owned bytes are zeroed on drop.

Developer tokens are ES256 JWTs with `alg`, `kid`, `iss`, `iat`, and `exp`, generated outside the render loop with a 30-day lifetime and refreshed five minutes before expiry. They remain in memory. Browser tokens additionally carry Apple's `origin` claim for the exact ephemeral loopback origin.

Music User Token consent uses MusicKit on the Web because Apple provides no documented direct terminal sign-in endpoint. `auth` binds an ephemeral server to `127.0.0.1`, creates a 256-bit nonce, opens a minimal non-persistent MusicKit JS page, and accepts a token only when the callback's loopback peer, host, origin, path, content type, bounded size, and nonce match. The helper closes after success or a five-minute timeout. A signed native MusicKit helper remains a future packaging alternative.

The Music User Token is verified with `GET /v1/me/storefront` and stored under macOS Keychain service `apple-music-tui`, account `music-user-token`. Apple publishes no dependable fixed Music User Token lifetime or refresh protocol, so the token is opaque: a 401/403 transitions to reauthorization instead of a guessed refresh. Other platforms use the same credential-store abstraction but report Keychain as unsupported. Authorization failure disables only personalized capabilities; catalog access may remain available with a valid Developer Token, and local Music.app playback remains available independently.

## Async runtime and workers

Tokio owns application tasks. Separate bounded channels carry input, backend events, commands, and shutdown signals. Blocking Apple Events, Keychain, SQLite, filesystem, JWT signing, and image decoding run in dedicated workers or `spawn_blocking` tasks.

The UI loop selects over input, backend events, timers, and shutdown. Rendering remains synchronous and cheap. Backpressure is explicit: coalescible playback ticks may be dropped/replaced, while user commands and mutations are not silently discarded.

## Caching and persistence

Persistent cache is introduced only when remote data exists. SQLite is the likely metadata index; artwork uses a bounded filesystem cache. Entries carry source, storefront, fetch time, and invalidation metadata. Successful mutations update local state/cache or invalidate affected resources. Transient failures retain stale-but-usable cached data and show offline state.

The initial frame never waits for cache migration, network access, full library scans, or artwork decoding. Music.app collection state is in memory only and is rebuilt on launch; the optional paid-auth path is not consulted by the local backend.

## Configuration

The default configuration path is `$XDG_CONFIG_HOME/apple-music-tui/config.toml` when `XDG_CONFIG_HOME` is absolute, otherwise `~/.config/apple-music-tui/config.toml`. Missing configuration uses safe defaults. `~` expansion is limited to the current user's `~` and `~/...`; `~user` is rejected. Malformed or partial Apple configuration returns field-level diagnostics and never prints secrets.

Keybindings are parsed into semantic actions. Help is generated from the same active keymap, so remapping a command updates help automatically. Themes use semantic color roles rather than raw colors in widgets.

## Terminal rendering and safety

The terminal is initialized with raw mode, alternate screen, hidden cursor, and bracketed/paste-related modes only as needed. A guard restores all changed state on normal return and error. A panic hook performs best-effort restoration before forwarding to the prior hook.

Rendering is viewport-based, handles resize events, and targets 80×24 as the minimum comfortable layout. Narrow terminals degrade to fewer panels and truncated metadata rather than panicking.

## Error model

Reusable typed errors distinguish configuration, authentication, network, API, backend, cache, playback, terminal, and unsupported-feature failures. `anyhow` is reserved for an outer application boundary if later justified. Recoverable errors become non-blocking notifications or view error states; required user actions may use a modal.

Retries use bounded exponential backoff with jitter only for transient failures. Authentication/permission failures are not retried forever.

## Observability

Structured logs use `tracing` and write outside the active terminal surface. Sensitive headers, tokens, key material, cookies, and user secrets are redacted by construction. The future `doctor` command reports capability routes and statuses without revealing credential values.

## Portability

The mock and Apple Music HTTP layers compile on supported non-macOS targets. All Music.app integration is behind `cfg(target_os = "macos")` at the backend boundary. On other platforms the capability set simply omits Music.app features; the UI and reducer remain unchanged.

## Testing strategy

Tests require no network, credentials, Music.app, or Apple account.

- Reducer tests cover navigation, progressive collection updates, lazy playlist loading, contextual selection playback, loading/error transitions, and search input.
- Mock backend tests cover deterministic playback, seek, queue, and favorite state changes.
- Capability/router tests cover route selection and unsupported operations.
- Input tests cover configurable key parsing and semantic mapping.
- DTO fixture tests cover API-to-domain conversion without live HTTP.
- Terminal rendering tests exercise small dimensions using Ratatui's test backend.

Opt-in ignored tests exercise the installed Music.app for playlist discovery, one bounded library batch, real playlist metadata, exact user-playlist track playback, and playlist playback. They are never part of normal `cargo test` and briefly mutate playback only in the explicitly named playback test.

Quality gates are `cargo fmt --check`, Clippy for all targets/features with warnings denied, and `cargo test`.
