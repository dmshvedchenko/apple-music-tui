# apple-music-tui

`apple-music-tui` is a keyboard-first terminal interface for Apple Music
on macOS.

Built with Rust and Ratatui, it provides a local-first Apple Music
client using Music.app automation without requiring an Apple Developer
Program membership.

![apple-music-tui](docs/screenshots/player.png)

## Features

- 🎵 Native Music.app playback
- 📚 Browse local Apple Music library
- 🖼 Terminal album artwork
- 🎧 Full-screen Now Playing
- 🔀 Session-based shuffle
- 🔎 Search, sorting and filtering
- 📂 Playlist and album playback
- ⌨ Keyboard-driven workflow
- 🍺 Homebrew installation

Music.app remains the audio engine. The application does not decode or
stream Apple Music audio itself.

## Installation

### Homebrew (recommended)

```bash
brew install dmshvedchenko/tap/apple-music-tui
```

Run:

```bash
apple-music-tui --backend macos
```

### From source

Requirements:

- macOS
- Rust stable 1.88+
- Music.app installed
- Terminal with alternate screen support

```bash
cargo install --path .
apple-music-tui --version
apple-music-tui doctor
apple-music-tui --backend macos
```

## Screenshots

### Playlist browsing and playback

![Playlist view](docs/screenshots/player.png)

### Full-screen Now Playing

![Now Playing](docs/screenshots/now-playing.png)

## Links

- Source code: https://github.com/dmshvedchenko/apple-music-tui

- Releases: https://github.com/dmshvedchenko/apple-music-tui/releases

- Homebrew tap: https://github.com/dmshvedchenko/homebrew-tap

## Local Music.app backend

The macOS backend provides a local-first Apple Music experience without
paid Apple Developer Program membership.

Supported:

- real Music.app playlists and folders;
- lazy-loaded playlist contents;
- local library Songs;
- derived Albums and Artists;
- local Recently Added and Recently Played;
- cached search;
- artwork loading;
- exact track playback;
- playlist and album playback sessions;
- playback state synchronization.

Music.app remains authoritative for:

- current track;
- playback state;
- audio output.

## Cache and refresh

After the first successful scan, metadata cache allows faster startup.

Startup:

```text
Library: Cached
```

Music.app refreshes data in the background:

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

The cache does not contain playback state, session state, or artwork
bytes.

## Artwork

Artwork is loaded lazily and cached.

Supported:

- Kitty graphics protocol;
- iTerm2 inline images;
- Unicode fallback.

For tmux:

```tmux
set -g allow-passthrough on
```

Unsupported terminals automatically fall back without affecting playback
or navigation.

## Playback sessions

Playlist and album playback use synthesized stable-ID sessions.

This provides:

- exact track continuation;
- previous/next handling;
- session shuffle;
- repeat support;
- Music.app synchronization.

The application does not implement:

- Up Next;
- Play Next;
- Play Later;
- queue editing.

These features require APIs that Music.app does not currently expose.

## Playlist editing

The only supported playlist mutation is removing a track occurrence from
an editable normal user playlist.

Press:

```text
d
```

After confirmation:

- removes one selected playlist entry;
- never deletes the library track;
- safely handles duplicate tracks.

Unsupported:

- smart playlists;
- system playlists;
- playlist creation;
- playlist reorder;
- ratings;
- favorites writes.

## Keyboard shortcuts

Key Action

---

`j/k` Navigate
`gg/G` First/last item
`Enter` Open or play
`P` Play playlist/album
`N` Full-screen Now Playing
`n/p` Next/previous track
`Space` Play/pause
`s` Toggle shuffle
`r` Toggle repeat
`R` Refresh library
`/` Search
`S` Sort
`F` Filter
`?` Help
`q` Quit

## Optional Apple Music API

Apple Developer credentials are not required for local Music.app
playback.

The optional Apple Music API path exists for future catalog and
personalized features.

## Development

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo build --release
```

## Repository

Source:

https://github.com/dmshvedchenko/apple-music-tui

License:

MIT

## Troubleshooting

### Music.app permission

Enable Automation permission for the application that launches the
terminal:

System Settings → Privacy & Security → Automation

Possible hosts:

- Terminal
- iTerm2
- Ghostty
- Codex

### Music.app is not running

The TUI does not automatically launch Music.app.

Use:

```text
o
```

to open it.

### Artwork in tmux

Enable:

```tmux
set -g allow-passthrough on
```

or force:

```bash
APPLE_MUSIC_TUI_ARTWORK_RENDERER=kitty
```

## License

MIT License.
