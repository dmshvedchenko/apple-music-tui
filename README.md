# apple-music-tui

`apple-music-tui` is a keyboard-first terminal interface for Apple Music. The project is being built around documented Apple Music API, MusicKit, and macOS Music.app capabilities, with explicit degradation when Apple does not expose a feature.

## Current state

The macOS backend now provides a useful local-first application without paid Apple Developer Program membership. Through Music.app's installed public scripting dictionary it reads real hierarchical playlists and progressively lazy-loaded playlist contents, progressively loads library songs, derives albums/artists/Recently Added/local Recently Played, searches cached typed track/artist/album/playlist entries, lazy-loads artwork, and plays an exact selected track, playlist, or ordered album. Music.app remains the authoritative playback and audio engine.

The mock backend models playback state, position, current track, next/previous, and queue transitions in memory, but it intentionally produces no sound. The TUI labels it `Mock Playback (no audio)`. The macOS backend controls real protected or local playback inside Music.app; it does not decode or stream audio itself.

Mock mode continues to use deterministic demo data. In `--backend macos`, Playlists, Songs, Artists, Albums, Recently Added, Search, and their detail routes use local Music.app data. Listen Now, Browse, Radio, and Made for You remain limited/demo-oriented until a documented provider is implemented.

Phase 0 research is captured in:

- [`docs/FEATURE_MATRIX.md`](docs/FEATURE_MATRIX.md)
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
- [`TODO.md`](TODO.md)
- [`docs/LOCAL_MUSIC_APP.md`](docs/LOCAL_MUSIC_APP.md)

## Using apple-music-tui without Apple Developer Program

Requirements: macOS with Music.app installed, Rust stable 1.88 or newer, and a terminal that supports an alternate screen.

```bash
cargo run -- --backend macos
```

## Installation and quick start

This local Music.app client does **not** require a paid Apple Developer account.
Install from a checked-out release with Rust stable 1.88 or newer:

```bash
cargo install --path .
apple-music-tui --version
apple-music-tui doctor
apple-music-tui --backend macos
```

Alternatively, copy a matching macOS release binary named `apple-music-tui` to
a directory on your `PATH`. The Cargo package version is the single version
source; `apple-music-tui version` and `--version` print it. Use
`apple-music-tui --help` for the complete CLI surface.

The primary daily-use path is `--backend macos`. `--backend mock` is a
deterministic no-audio development/demo mode. Apple API authentication commands
remain optional and are not needed for the local library, playback, cache, or
artwork features.

The TUI does not launch Music.app on startup. If Music.app is closed, the TUI remains open and reports `Music.app is not running`; press `o` to launch it explicitly. Startup performs an immediate Music.app query before 500 ms polling begins, so an already-playing track appears without waiting for a track-change event. Every poll reconciles player state and position and carries a small current-track metadata fingerprint. This is necessary because Music.app can return null or unusable persistent/database IDs for subscription-backed items. On macOS, `--backend auto` selects this backend.

The status line reports `Connecting`, `Connected`, `Music.app is not running`, permission denial, or a synchronization failure from actual query results. Constructing the backend alone is not considered a successful connection. After the first successful local scan, a versioned last-known metadata cache makes Songs, derived library views, playlist hierarchy, and local search available immediately as `Library: Cached`; Music.app then refreshes it in the background and remains authoritative. It reports `Refreshing loaded/total`, `Ready`, or a failed refresh while retaining usable cached data. Press `R` to request one new background scan; repeated presses while a scan is active are safely coalesced. The cache contains no playback/session state or artwork bytes.

Inspect or remove only that local metadata snapshot without touching Music.app, configuration, artwork, or playback state:

```bash
cargo run -- cache-status
cargo run -- cache-clear
```

Paths follow platform conventions:

- Optional Apple configuration: `$XDG_CONFIG_HOME/apple-music-tui/config.toml`, or `~/.config/apple-music-tui/config.toml`.
- Local Music.app metadata cache: `$XDG_CACHE_HOME/apple-music-tui/musicapp-library-v1.json`, or `~/Library/Caches/apple-music-tui/musicapp-library-v1.json` on macOS.
- Logs: none are persisted by default; diagnostics go to stderr only when `RUST_LOG` is set.

`doctor` reports the resolved config/cache paths, cache readability, Music.app and
Automation status, terminal/artwork renderer details, tmux passthrough state,
and optional Apple API configuration without revealing secrets.

## Screens

Screenshots are intentionally pending final visual review in supported terminal
emulators. The release checklist includes the manual Ghostty and tmux artwork
verification required before publishing.

The first control attempt may trigger a macOS Automation consent prompt for the process that launched the TUI. Depending on how it was started, the relevant entry can be Terminal, iTerm, Ghostty, Codex, or another host application—not an abstract `apple-music-tui` entry. Allow that actual host to control Music in **System Settings → Privacy & Security → Automation**. A denied permission is shown in the TUI and does not terminate the application. If permission was previously denied, enable Music under the launching host in that settings pane and retry.

No `[apple]` configuration, Team ID, Key ID, `.p8`, Developer Token, or Music User Token is needed for this mode. The status area identifies `Backend: Music.app`, reports local-library progress, and labels Apple API state as optional.

The Music.app backend supports play, pause, play/pause, next, previous, seek, sound volume, shuffle, repeat, current-track metadata, playback-state polling, explicit Music.app launch, expandable playlist folders and lazy nested-playlist contents, real library Songs, derived Artists/Albums/Recently Added/local Recently Played, cached local search across tracks/artists/albums/playlists, and exact selected track/playlist/album playback. Collection loading is progressive: the TUI renders immediately, discovers playlists, then loads the library in profiled bounded 400-track batches while showing progress. Opening a playlist immediately shows a distinct loading state; every returned batch is navigable/playable, completed contents are retained for the process lifetime, and foreground playlist requests take precedence over the background scan. On the audited 12,997-track debug library the library optimization reduced ready time from roughly 55–60 seconds to roughly 31 seconds. Music.app remains the audio engine, including for protected/cloud tracks that have no local file path.

Whole-album playback is synthesized from exact stable track IDs in disc/track-number order because Music.app does not accept an array of track specifiers. Starting a selected playlist track likewise creates an explicit stable-ID playlist context because exact track-object playback supplies no reliable continuation queue. Both use `once: true`; the backend owns next/previous and repeat progression, while Music.app polling remains authoritative. Playlist shuffle order is synthesized once per start and shown through the player context; changing shuffle cancels the context and asks the user to restart it. An external Music.app track selection cancels either session. Near a playlist end, one bounded JXA request watches authoritative Music.app state and starts the exact next item only after `Stopped`; live three-track runs measured a 0.18–0.32 second post-audio gap on this host. This is not Up Next support.

Artwork is loaded lazily for album detail and Now Playing, never during the initial scan. Raw Music.app bytes are validated and limited to 2 MiB, cached in memory for at most 16 identities, and terminal-inline payloads are limited to 512 KiB. iTerm2 displays supported JPEG/PNG/GIF data inline; Ghostty and Kitty pass PNG through and asynchronously convert bounded JPEG source art to a separately cached PNG display asset. GIF remains a visible Unicode fallback in Kitty mode. Inside tmux, set `set -g allow-passthrough on`; if the outer terminal cannot be detected, use `APPLE_MUSIC_TUI_ARTWORK_RENDERER=kitty` or `unicode` (the default is `auto`). Sixel, unsupported Kitty formats, missing/invalid/large artwork, small terminals, and terminals without image support use the visible Unicode placeholder without affecting navigation or playback.

Run `cargo run -- artwork-test` to send a validated opaque-red 1×1 PNG through the selected renderer without starting Music.app or Ratatui. The diagnostic uses the minimal direct Kitty form (`a=T,f=100`), waits two seconds before its report, and shows renderer, TTY status, framing payload, bytes written, and flush result; `RUST_LOG=debug` also prints the raw detected `TMUX` value and bounded first/last graphics bytes.

`Recently Played` is explicitly local Music.app metadata: it includes only library items with a real played date, sorts newest first, and shows Music.app's play count. It is not Apple cloud Listen Now history.

Music.app 1.6.5 exposes no documented Up Next API, so real queue read/write/reorder, Play Next, and Play Later are disabled. Mute mutation is disabled because changed-value writes returned Music.app error 9038. The sole supported playlist write is `d`: after confirmation, it removes one selected occurrence from a normal editable user playlist, never the library track. It is unavailable for smart, folder, system/library, subscription, and unknown playlists; duplicate occurrences are addressed by their displayed position and stable identity. Create, rename, reorder, ratings, and favorites remain deliberately disabled.

For synchronization diagnostics, enable transition-level tracing and redirect stderr so logs do not interfere with the alternate-screen UI:

```bash
RUST_LOG=apple_music_tui=debug cargo run -- --backend macos 2>apple-music-tui.log
```

Use `apple_music_tui=trace` only for a short investigation when every poll, backend event, and applied position is needed; it is intentionally verbose.

## Run the mock TUI

Requirements: Rust stable 1.88 or newer and a terminal that supports an alternate screen.

```bash
cargo run -- --backend mock
```

## Optional Apple Music API authentication

Local Music.app playback, playlists, library views, and search do not require Apple Developer credentials. The existing authentication path is optional/dormant unless future Apple Music API catalog or personalized-library features are explicitly used. The TUI reports its optional state without disabling local functionality.

If you choose to use the optional Apple API path, you need an Apple Developer Program account with permission to create identifiers and keys:

1. In Apple Developer **Certificates, Identifiers & Profiles**, register a Media ID and enable MusicKit.
2. Create a Media Services private key associated with that Media ID. Record its 10-character Key ID and download the `.p8` file; Apple allows the download only once.
3. Find the 10-character Team ID under Apple Developer membership details.
4. Keep the `.p8` file outside the repository, ideally under `~/.config/apple-music-tui/` with owner-only filesystem permissions.

Create `~/.config/apple-music-tui/config.toml`:

```toml
[apple]
team_id = "XXXXXXXXXX"
key_id = "YYYYYYYYYY"
private_key = "~/.config/apple-music-tui/AuthKey_YYYYYYYYYY.p8"
storefront = "de"
```

The optional `storefront` is a lowercase ISO 3166-1 alpha-2 country code for future pre-auth catalog requests. After user authorization, the account storefront returned by Apple is authoritative; the application never assumes `us`.

Check setup without printing any credential:

```bash
cargo run -- config-path
cargo run -- doctor
```

Then authorize:

```bash
cargo run -- auth
```

The command generates a bounded-lifetime in-memory Developer Token, starts a temporary server on an ephemeral `127.0.0.1` port, and opens a plain MusicKit JS page. The callback is protected by an exact origin/host/path check and a random per-run nonce. No browser token is written to `localStorage`; after Apple consent, the Music User Token is verified against `/v1/me/storefront` and stored as the `music-user-token` account under the `apple-music-tui` macOS Keychain service. The helper closes immediately, Rust-owned secret strings are zeroed when dropped, and the page keeps no persistent copy; close it after success to discard its in-memory state. Raw tokens and key contents are never printed.

Useful follow-up commands:

```bash
cargo run -- auth status
cargo run -- auth logout
```

Apple does not document a direct terminal OAuth endpoint or a dependable fixed Music User Token expiry/refresh schedule. The application therefore treats the user token as opaque, validates it with Apple, and asks you to authorize again after a 401/403 instead of guessing an expiry or refresh protocol.

If authorization fails:

- confirm that the Media Services key is linked to a MusicKit-enabled Media ID;
- confirm the Team ID and Key ID are exactly the values shown in Apple Developer;
- confirm the `.p8` is the original ES256 PKCS#8 private key and is readable;
- allow the browser to load `js-cdn.music.apple.com` and complete the Apple Music consent dialog;
- copy the printed loopback URL if the browser did not open automatically;
- run `cargo run -- doctor` for stage-specific config, signing, Keychain, API, Music.app, and Automation diagnostics.

The foundation keybindings are:

| Key | Action |
|---|---|
| `j` / `k`, `↑` / `↓` | Move selection |
| `gg` / `Home`, `G` / `End` | First / last item |
| `Ctrl-u` / `PgUp`, `Ctrl-d` / `PgDn` | Half-page up / down |
| `h` / `l`, left/right | Change panel focus; `h` returns from a detail view |
| `Enter` | Open the selected screen/detail; play the selected track |
| `Enter` on a playlist folder | Expand/collapse its nested entries |
| `Enter` on a playlist track | Start a synthesized playlist context at that ordered position |
| `P` | Play the selected album or playlist through Music.app |
| `a` | Open actions for the selected track, album, artist, playlist, or folder |
| `d` | In an editable normal user Playlist Detail only, confirm removal of the selected track occurrence (never the library track) |
| `N` | Open full-screen Now Playing; `Esc`, `h`, or `q` returns |
| `R` | Refresh the local Music.app library in the background |
| `S` / `Space` | Open the current Songs, Albums, or Artists sort menu / toggle its direction |
| `F` / `Ctrl-l` | Filter the current Songs, Albums, or Artists view / clear its filter |
| `/` | Open local library search; Enter finishes editing |
| `Esc` / `h` | Return from a detail view; Esc closes Help/search editing |
| `c` / `x` | Play/pause explicitly |
| `Space` | Toggle play/pause |
| `n` / `p` | Next/previous track |
| `[` / `]` | Seek backward/forward |
| `-` / `+` | Volume down/up |
| `m` | Toggle mute when the active backend supports it |
| `s` / `r` | Toggle shuffle/cycle repeat when supported |
| `f` | Favorite the current track |
| `o` | Open the local player when supported |
| `1`–`9` | Open a sidebar destination |
| `?` | Open Help; `?`, `Esc`, or `q` closes Help |
| `q` / `Ctrl-C` | Quit outside overlays, request Music.app Stop (bounded), then restore the terminal; `q` in Help, Actions, confirmation, or Now Playing only closes/returns |

## Capability policy

The Apple Music REST API supplies catalog/library data and supported mutations, but not reusable full-track audio streams. Protected playback is routed through documented Apple-managed surfaces. Music.app automation does not expose a documented Up Next API, and timed lyrics are not exposed by the researched public APIs; those actions remain disabled unless the official capability surface changes. The mock backend intentionally has broader queue capabilities so application behavior remains deterministic and testable.

## Development

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo build --release
```

The release binary is `target/release/apple-music-tui`. CI runs formatter,
Clippy, and ordinary tests on Linux, plus compile/tests on macOS. Live Music.app
tests stay ignored because they require a user library and Automation consent.

## Troubleshooting and known limitations

- If Automation is denied, enable Music for the terminal application that
  launched the TUI in **System Settings → Privacy & Security → Automation**.
- If Music.app is not running, start it manually or press `o` in the TUI.
- In Ghostty through tmux, enable `set -g allow-passthrough on`; use
  `APPLE_MUSIC_TUI_ARTWORK_RENDERER=kitty` only when the outer terminal supports it.
- Music.app does not provide documented Up Next, Play Next/Later, queue reorder,
  safe playlist mutation, timed lyrics, or cloud discovery/recommendations here.
- The optional Apple API path requires a Developer Program account and remains
  separate from the mature local-only client.

See [the release checklist](docs/RELEASE_CHECKLIST.md) before publishing a tag.

## License

Licensed under the [MIT License](LICENSE).
