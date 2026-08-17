# apple-music-tui

![GitHub release](https://img.shields.io/github/v/release/dmshvedchenko/apple-music-tui)
![License](https://img.shields.io/github/license/dmshvedchenko/apple-music-tui)
![Rust](https://img.shields.io/badge/Rust-1.88+-orange)
![Platform](https://img.shields.io/badge/macOS-15+-blue)

`apple-music-tui` is a keyboard-first terminal interface for Apple Music on macOS.

Built with Rust and Ratatui, it provides a local-first Apple Music client using Music.app automation without requiring an Apple Developer Program membership.

![apple-music-tui](docs/screenshots/player.png)

## Why?

Apple Music has an excellent catalog, but very few keyboard-first desktop clients.

`apple-music-tui` focuses on fast navigation, efficient keyboard workflows, and native Music.app integration while leaving playback, DRM, and audio output to Apple's own application.

If you enjoy tools like **lazygit**, **k9s**, **btop**, or **yazi**, the interface should feel immediately familiar.

## Features

- 🎵 Native Music.app playback
- 📚 Browse local Apple Music library
- 🖼 Terminal album artwork
- 🎧 Full-screen Now Playing
- 🔀 Session-based shuffle
- 🔎 Search, sorting and filtering
- 📂 Playlist and album playback
- ⚡ Fast metadata cache with background refresh
- ⌨ Keyboard-driven workflow
- 🍺 Homebrew installation

Music.app remains the audio engine. `apple-music-tui` never decodes or streams Apple Music audio itself.

---

## Installation

### Homebrew (recommended)

```bash
brew install dmshvedchenko/tap/apple-music-tui
```

Run:

```bash
apple-music-tui --backend macos
```

### Build from source

Requirements:

- macOS
- Music.app
- Rust 1.88+
- Terminal with alternate screen support

```bash
cargo install --path .

apple-music-tui --version
apple-music-tui doctor
apple-music-tui --backend macos
```

---

## Screenshots

### Playlist browsing

![Playlist view](docs/screenshots/player.png)

### Full-screen Now Playing

![Now Playing](docs/screenshots/now-playing.png)

---

## Links

- **GitHub**  
  https://github.com/dmshvedchenko/apple-music-tui

- **Releases**  
  https://github.com/dmshvedchenko/apple-music-tui/releases

- **Homebrew Tap**  
  https://github.com/dmshvedchenko/homebrew-tap

---

## Local Music.app backend

The macOS backend provides a complete local-first Apple Music experience without a paid Apple Developer Program membership.

Supported:

- hierarchical playlists and folders
- progressive playlist loading
- local library
- Artists / Albums / Recently Added / Recently Played
- search
- artwork
- exact track playback
- playlist and album playback
- playback synchronization with Music.app

Music.app remains authoritative for:

- playback
- DRM
- current track
- audio output

---

## Cache and refresh

After the first successful scan the application stores a local metadata cache.

Startup:

```text
Library: Cached
```

Background refresh:

```text
Refreshing loaded/total
Ready
```

Manual refresh:

```text
R
```

Inspect or clear metadata cache:

```bash
apple-music-tui cache-status
apple-music-tui cache-clear
```

The cache contains metadata only.

It never stores:

- playback state
- playback sessions
- artwork

---

## Artwork

Artwork is loaded lazily and cached.

Supported terminals:

- Kitty graphics protocol
- Ghostty
- iTerm2
- Unicode fallback

For tmux:

```tmux
set -g allow-passthrough on
```

---

## Playback sessions

Playlist and album playback use synthesized stable-ID sessions.

This enables:

- exact continuation
- previous / next
- repeat
- shuffle
- playback synchronization

### Intentionally not supported

These features are unavailable because Music.app does not expose public APIs for them:

- Up Next editing
- Play Next
- Play Later
- queue editing
- timed lyrics
- recommendations

---

## Playlist editing

The only supported playlist mutation is removing a track occurrence from an editable user playlist.

Key:

```text
d
```

After confirmation it removes only the selected playlist entry.

The underlying library track is never deleted.

---

## Keyboard shortcuts

| Key     | Action                        |
| ------- | ----------------------------- |
| `j/k`   | Navigate                      |
| `gg/G`  | First / last item             |
| `Enter` | Open / play                   |
| `P`     | Play playlist or album        |
| `.`     | Jump to current playing track |
| `N`     | Full-screen Now Playing       |
| `Space` | Play / Pause                  |
| `n/p`   | Next / Previous               |
| `s`     | Toggle shuffle                |
| `r`     | Toggle repeat                 |
| `R`     | Refresh library               |
| `/`     | Search                        |
| `S`     | Sort                          |
| `F`     | Filter                        |
| `?`     | Help                          |
| `q`     | Quit                          |

---

## Optional Apple Music API

Local playback does **not** require Apple Developer credentials.

The optional Apple Music API path is reserved for future cloud features.

---

## Development

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo build --release
```

---

## Troubleshooting

### Music.app permission

Enable Automation permission for the application launching `apple-music-tui`.

System Settings → Privacy & Security → Automation

Typical hosts:

- Terminal
- iTerm2
- Ghostty

### Music.app is not running

Press:

```text
o
```

to launch Music.app.

### Artwork inside tmux

Enable:

```tmux
set -g allow-passthrough on
```

or force the renderer:

```bash
APPLE_MUSIC_TUI_ARTWORK_RENDERER=kitty
```

---

## Feedback

Bug reports, feature requests and pull requests are welcome.

If you enjoy the project, consider giving it a ⭐ on GitHub.

---

## License

MIT License.
