# Release checklist

## Automated / repository checks

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] `cargo test`
- [ ] `cargo build --release`
- [ ] `target/release/apple-music-tui --version`, `doctor`, and `cache-status`
- [ ] `cargo install --path . --root <temporary-directory>` and installed `--version`
- [ ] Review `git status` and `git diff --check`; confirm no secrets, config, cache, log, or build artifacts are staged.
- [ ] Confirm the Cargo version, license decision, and intended tag before publishing.

## Manual owner checks (macOS, Music.app, Automation consent)

- [ ] Verify Music.app synchronization; play/pause, next/previous, seek, volume, shuffle, and repeat.
- [ ] Verify continuous playlist playback and whole-album playback.
- [ ] Verify playlist lazy loading, warm-cache startup, manual `R` refresh, and local sort/filter.
- [ ] Verify Help, context actions, and full-screen Now Playing.
- [ ] Verify direct-Ghostty artwork; verify tmux artwork passthrough where supported.
- [ ] In a disposable normal user playlist, verify `d` confirmation/cancel and removal of exactly one selected occurrence; confirm its library track remains. Do not use an important playlist.
- [ ] Verify `q` in Help, Actions, confirmation, and Now Playing does not quit or stop Music.app.
- [ ] Verify normal `q` / Ctrl-C stops Music.app, clears artwork, restores the terminal, and returns to the shell.
- [ ] Verify `--backend mock` starts and exits cleanly.

Live checks require the release owner's library, terminal, and macOS Automation
consent. CI intentionally keeps live Music.app tests opt-in.
