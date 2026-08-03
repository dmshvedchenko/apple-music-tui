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

**Decision:** The macOS backend exposes launch, playback transport, seek, volume, shuffle, repeat, local library/playlist reads, local search, and identifier-based selected track/playlist playback. It does not expose queue, favorite mutation, destructive playlist mutation, or mute mutation. Persistent and database Music.app IDs occupy distinct `musicapp:persistent:` and `musicapp:database:` namespaces; an explicitly ephemeral metadata-derived identity is used when streamed items fail to return both.

**Rationale:** The local Music.app 1.6.5 dictionary and live tests verified the exposed controls, collection reads, and exact playback commands. It contains no Up Next surface, favorite/destructive mutations were not exercised, and changed-value mute writes returned Music.app error 9038. Capability flags represent observed behavior, not merely declarations in the scripting dictionary.

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

**Decision:** Emit typed partial backend updates for playback, playlist discovery, library batches, and playlist batches. Load 200 library tracks per JXA property-array request; lazy-load playlist tracks when their detail route opens. Derive grouping once after the final library batch. Maintain normalized typed search-index entries incrementally during loading and rebuild once after artist/album derivation. Drain subprocess pipes concurrently.

**Rationale:** Replacing or cloning a 12,997-track snapshot every 500 ms would make playback polling scale with library size. Partial events keep rendering responsive and preserve Music.app as the playback source of truth. Cached normalized search avoids rescanning raw collection fields on every input event. Concurrent pipe draining avoids the deadlock found when rich JSON exceeded the OS stdout pipe capacity.

## D015 — Selected playback requires exact context and identifiers

**Decision:** Use Music.app persistent/database IDs, never names, for playback selection. Commands from playlist detail carry both playlist ID and track ID; library/album/artist/search commands use the main library context. Playlist playback uses its persistent ID.

**Rationale:** Live testing found valid user-playlist tracks that resolve within their playlist but not through the Music library playlist. Explicit context makes playback safe and deterministic without private IDs or ambiguous metadata matching.
