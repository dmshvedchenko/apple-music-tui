# apple-music-tui

`apple-music-tui` is a keyboard-first terminal interface for Apple Music. The project is being built around documented Apple Music API, MusicKit, and macOS Music.app capabilities, with explicit degradation when Apple does not expose a feature.

## Current state

The macOS backend now provides a useful local-first application without paid Apple Developer Program membership. Through Music.app's installed public scripting dictionary it reads real playlists and playlist contents, progressively loads library songs, derives albums/artists/Recently Added, searches cached typed track/artist/album/playlist entries, and plays an exact selected track or playlist. Music.app remains the authoritative playback and audio engine.

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

The TUI does not launch Music.app on startup. If Music.app is closed, the TUI remains open and reports `Music.app is not running`; press `o` to launch it explicitly. Startup performs an immediate Music.app query before 500 ms polling begins, so an already-playing track appears without waiting for a track-change event. Every poll reconciles player state and position and carries a small current-track metadata fingerprint. This is necessary because Music.app can return null or unusable persistent/database IDs for subscription-backed items. On macOS, `--backend auto` selects this backend.

The status line reports `Connecting`, `Connected`, `Music.app is not running`, permission denial, or a synchronization failure from actual query results. Constructing the backend alone is not considered a successful connection.

The first control attempt may trigger a macOS Automation consent prompt for the process that launched the TUI. Depending on how it was started, the relevant entry can be Terminal, iTerm, Ghostty, Codex, or another host application—not an abstract `apple-music-tui` entry. Allow that actual host to control Music in **System Settings → Privacy & Security → Automation**. A denied permission is shown in the TUI and does not terminate the application. If permission was previously denied, enable Music under the launching host in that settings pane and retry.

No `[apple]` configuration, Team ID, Key ID, `.p8`, Developer Token, or Music User Token is needed for this mode. The status area identifies `Backend: Music.app`, reports local-library progress, and labels Apple API state as optional.

The Music.app backend supports play, pause, play/pause, next, previous, seek, sound volume, shuffle, repeat, current-track metadata, playback-state polling, explicit Music.app launch, real playlists and lazy playlist contents, real library Songs, derived Artists/Albums/Recently Added, cached local search across tracks/artists/albums/playlists, and exact selected track/playlist playback. Collection loading is progressive: the TUI renders immediately, discovers playlists, then loads the library in bounded 200-track batches while showing progress. Music.app remains the audio engine, including for protected/cloud tracks that have no local file path.

Music.app 1.6.5 exposes no documented Up Next API, so real queue read/write/reorder, Play Next, and Play Later are disabled. Whole-album playback, artwork rendering, and a dedicated Recently Played view remain partial/deferred. Mute mutation is disabled because changed-value writes returned Music.app error 9038. Playlist, rating, and favorite mutations remain deliberately disabled because their safe source/editability/rollback behavior has not been live-qualified.

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
| `j` / `k`, arrows | Move selection |
| `h` / `l`, left/right | Change panel focus; `h` returns from a detail view |
| `Enter` | Open the selected screen/detail; play the selected track |
| `P` | Play the selected playlist through Music.app |
| `/` | Open local library search; Enter finishes editing |
| `Esc` | Return from a detail view or close Help |
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
| `?` | Help |
| `q` / `Ctrl-C` | Quit |

## Capability policy

The Apple Music REST API supplies catalog/library data and supported mutations, but not reusable full-track audio streams. Protected playback is routed through documented Apple-managed surfaces. Music.app automation does not expose a documented Up Next API, and timed lyrics are not exposed by the researched public APIs; those actions remain disabled unless the official capability surface changes. The mock backend intentionally has broader queue capabilities so application behavior remains deterministic and testable.

## Development

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

The project license has not yet been selected.
