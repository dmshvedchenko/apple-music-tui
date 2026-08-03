# Feature Matrix

Research snapshot: 2026-08-03.

This matrix records product capability assumptions. A feature is exposed by the UI only when the active backend reports the corresponding capability.

## Status meanings

- `SUPPORTED`: a documented planned backend can provide the feature reliably.
- `PARTIAL`: only part of the desktop behavior or metadata is available.
- `BACKEND_DEPENDENT`: support varies by platform, authorization, subscription, or selected backend.
- `UNAVAILABLE`: no documented, reliable integration found for the planned architecture.

## Local Music.app only — no Apple Developer account

This table is scoped to `--backend macos` and the installed public Music.app 1.6.5 scripting definition. It does not assume a Developer Token, Music User Token, network access, private database access, or private frameworks.

Local statuses are stricter than dictionary availability:

- `SUPPORTED`: runtime-verified and exposed by the local backend.
- `PARTIAL`: a useful runtime-verified subset exists, or the view is derived rather than a first-class Music.app collection.
- `READ_ONLY`: the local surface can read the value/object, but the application exposes no safe write.
- `WRITE_ONLY`: a stable write exists without a corresponding useful read. No audited feature currently falls in this category.
- `UNAVAILABLE`: absent from the installed public surface, or not reliable/safe enough to expose after the runtime audit.

| Local feature | Public Music.app surface and runtime result | Current local-backend exposure | Status |
|---|---|---|---|
| Playback | `play`, `pause`, `playpause`, next, and previous commands worked live | Real Music.app transport with authoritative polling | `SUPPORTED` |
| Current track | `currentTrack` plus selected documented properties worked for local and cloud-backed items | Title, artist, album, duration, identity, and player bar | `SUPPORTED` |
| Seek | Read/write `playerPosition` worked live | Five-second seek controls, reconciled on the next poll | `SUPPORTED` |
| Volume | Read/write `soundVolume` worked live | Five-point volume controls | `SUPPORTED` |
| Shuffle | Read/write application shuffle state worked live | Toggle control | `SUPPORTED` |
| Repeat | Read/write `songRepeat` worked live, with asynchronous Music.app application | Cycle off/all/one and reconcile | `SUPPORTED` |
| Library songs | Library-playlist track ranges and selected property arrays worked live | Progressive 200-track batches and Songs view | `SUPPORTED` |
| Library albums | No first-class complete album collection is needed; album and album-artist fields worked | Stable composite grouping derived from loaded tracks | `PARTIAL` |
| Library artists | Artist and album-artist fields worked; no efficient canonical artist collection was found | Normalized grouping derived from loaded tracks | `PARTIAL` |
| Library playlists | Playlist classes/properties and bulk discovery worked live | Real user/system/subscription playlists | `SUPPORTED` |
| Playlist folders | Folder class and parent relationships are readable | Kind/parent preserved; flattened display, no expand/collapse yet | `READ_ONLY` |
| Smart playlists | User-playlist `smart` flag and contents are readable | Classified and readable; rules are never reverse-engineered or edited | `READ_ONLY` |
| Playlist contents | Track object specifiers worked; some concrete playlists require selected-property fallback | Lazy bounded batches with a 20-item fallback cap | `SUPPORTED` |
| Create playlist | Generic `make` exists, but source/editability behavior was not safely verified | Disabled | `UNAVAILABLE` |
| Rename playlist | Playlist name is declared writable, but duplicate/stale/editability and rollback behavior was not verified | Disabled | `UNAVAILABLE` |
| Delete playlist | Generic `delete` exists, but destructive runtime behavior was intentionally not exercised | Disabled | `UNAVAILABLE` |
| Add track to playlist | File `add` and object duplication exist, but no reliable audited cloud/library identity append was proven | Disabled | `UNAVAILABLE` |
| Remove track from playlist | Generic deletion is not sufficient proof of safe editable-playlist removal | Disabled | `UNAVAILABLE` |
| Reorder playlist tracks | No explicit reliable track-order command was found or verified | Disabled | `UNAVAILABLE` |
| Play selected track | `play(trackSpecifier)` worked with persistent/database-ID lookup | Exact library or playlist-context playback | `SUPPORTED` |
| Play selected playlist | `play(playlistSpecifier)` worked with persistent-ID lookup | `P` plays the selected real playlist | `SUPPORTED` |
| Play album | No reliable album-context playback operation was verified | Individual album tracks can play; whole-album action is disabled | `PARTIAL` |
| Track rating | Track rating is readable; writes were not exercised | Parsed as optional 0–100 legacy rating metadata | `READ_ONLY` |
| Favorite/love | Current dictionary exposes `favorited`; legacy/current semantic equivalence is not assumed | Parsed from the property using Music.app's own “favorited” name; writes disabled | `READ_ONLY` |
| Play count | `playedCount` worked as a nullable/coercible track field | Parsed and retained as metadata | `READ_ONLY` |
| Date added | `dateAdded` worked, including missing values | Parsed and used for Recently Added | `READ_ONLY` |
| Genre | Track genre worked, including empty/missing values | Parsed, displayed where applicable, and indexed for search | `READ_ONLY` |
| Year | Track year worked as a nullable/coercible numeric field | Parsed and used for album metadata | `READ_ONLY` |
| Composer | Track composer worked, including empty/missing values | Parsed and indexed for search | `READ_ONLY` |
| Artwork | Artwork objects exist, but eager bytes would be expensive and lazy retrieval was not live-qualified | No artwork reads during library loading; renderer deferred | `PARTIAL` |
| Recently added | Real `dateAdded` values worked | Local album view sorted by newest available date | `SUPPORTED` |
| Recently played | Played date/count are readable, but they are not a complete event history | Metadata retained; dedicated history view deferred | `PARTIAL` |
| Queue read | Installed definition exposes current playlist, not Music.app Up Next | Disabled | `UNAVAILABLE` |
| Queue write | No installed public Up Next mutation surface | Disabled | `UNAVAILABLE` |
| Play Next | No installed public command | Disabled | `UNAVAILABLE` |
| Play Later | No installed public command | Disabled | `UNAVAILABLE` |
| Queue reorder | No installed public command | Disabled | `UNAVAILABLE` |
| Search local library | Already-loaded normalized metadata can be indexed without Music.app/API calls per keypress | Cached all-term search across tracks, artists, albums, and playlists | `SUPPORTED` |

## Matrix

| Feature | Desktop Apple Music | Apple Music API | MusicKit | macOS Music.app | Planned backend | Status |
|---|---|---|---|---|---|---|
| Playback | Full protected playback | Play parameters and previews; no raw full-track stream | Protected playback through Apple-managed players | Music.app performs protected playback | Composite routes to Music.app on macOS; possible MusicKit helper later | `BACKEND_DEPENDENT` |
| Play | Yes | No playback transport endpoint | `MusicPlayer.play()` | `play` command | macOS playback / mock | `BACKEND_DEPENDENT` |
| Pause | Yes | No | `MusicPlayer.pause()` | `pause` command | macOS playback / mock | `BACKEND_DEPENDENT` |
| Next | Yes | No | `skipToNextEntry()` | `next track` command | macOS playback / mock | `BACKEND_DEPENDENT` |
| Previous | Yes | No | `skipToPreviousEntry()` | `previous track` and `back track` | macOS playback / mock | `BACKEND_DEPENDENT` |
| Seek | Yes | No | Read/write playback time and seek methods | Read/write `player position` | macOS playback / mock | `BACKEND_DEPENDENT` |
| Volume | Yes | No | Web player volume; no uniform Swift player-volume API found | Read/write `sound volume` | macOS playback / mock | `BACKEND_DEPENDENT` |
| Mute | Yes | No | No uniform player mute API across MusicKit surfaces found | Readable, but live writes returned error 9038 in Music.app 1.6.5 | Mock only until a reliable production mutation route is verified | `PARTIAL` |
| Shuffle | Yes | No | Player shuffle mode | Read/write shuffle enabled/mode | macOS playback / mock | `BACKEND_DEPENDENT` |
| Repeat | Yes | No | Player repeat mode | Read/write `song repeat`; changed values apply asynchronously | macOS playback / mock | `BACKEND_DEPENDENT` |
| Current track | Yes | No live-player state | Queue current entry | Read-only `current track` with metadata | macOS playback / mock | `BACKEND_DEPENDENT` |
| Playback position | Yes | No live-player state | `playbackTime` | Read/write `player position` | macOS playback / mock | `BACKEND_DEPENDENT` |
| Queue read | Yes | No player queue API | Player queue and current entry | Scripting definition exposes current playlist, not the Up Next queue | MusicKit native helper candidate / mock | `BACKEND_DEPENDENT` |
| Queue edit | Yes | No player queue API | Insert after current entry or at tail | No documented Up Next mutation commands | MusicKit helper candidate / mock | `PARTIAL` |
| Queue reorder | Yes | No | No documented arbitrary reorder operation found | No documented Up Next reorder operation | Mock only until a documented production route is verified | `UNAVAILABLE` |
| History | Yes | Recently played resources, not a complete timestamped event log | Recently played requests | Play count/date metadata, not full history | Apple Music API | `PARTIAL` |
| Search | Yes | Catalog and personal-library search | Catalog/library requests | Track metadata plus search within a Music.app playlist | Local in-memory metadata search; optional Apple API for catalog | `SUPPORTED` |
| Catalog | Yes | Albums, artists, songs, playlists, stations, genres, charts, and more | Typed catalog requests | Not a general remote catalog interface | Apple Music API | `SUPPORTED` |
| Library songs | Yes | Paginated personal library | Typed library requests | Scriptable library tracks | Music.app local first; optional Apple API | `SUPPORTED` |
| Library albums | Yes | Paginated personal library | Typed library requests | Derivable from scriptable tracks | Music.app-derived local first; optional Apple API | `SUPPORTED` |
| Library artists | Yes | Paginated personal library and search | Typed library requests | Derivable from scriptable tracks | Music.app-derived local first; optional Apple API | `SUPPORTED` |
| Library playlists | Yes | Paginated personal library | Typed library requests | Scriptable playlist objects | Music.app local first; optional Apple API | `SUPPORTED` |
| Create playlist | Yes | Create library playlist, optionally with tracks and parent folder | API access; platform-specific mutation availability | `make` a user playlist | Apple Music API / macOS | `SUPPORTED` |
| Rename playlist | Yes | No documented REST rename endpoint found | No uniform documented macOS mutation route | Writable playlist name | macOS only until API support changes | `BACKEND_DEPENDENT` |
| Delete playlist | Yes | No documented REST delete endpoint found | No uniform documented macOS mutation route | Generic `delete` command | macOS only, guarded by capability and editability | `BACKEND_DEPENDENT` |
| Add track to playlist | Yes | Append tracks to a library playlist | Playlist-addable items on supported platforms | File add and object duplication are scriptable, with source limitations | Apple Music API first | `SUPPORTED` |
| Remove track from playlist | Yes | No documented REST removal endpoint found | No uniform documented removal route | Generic deletion may work for editable playlist elements | macOS only after integration tests | `BACKEND_DEPENDENT` |
| Reorder playlist | Yes | No documented REST reorder endpoint found | No documented arbitrary REST-equivalent operation | No explicit track reorder command | None until verified | `UNAVAILABLE` |
| Favorite / Love | Yes | Add/remove/query favorites; resource `inFavorites` metadata | Available through API/model properties | Writable `favorited` properties | Apple Music API / macOS | `SUPPORTED` |
| Ratings | Like/dislike and legacy stars where applicable | Like/dislike values `1` and `-1`; no star scale | API ratings | Writable 0–100 track/album ratings plus disliked/favorited | Capability-specific rating model | `PARTIAL` |
| Recently Added | Yes | `/v1/me/library/recently-added` | Library requests | Track `date added` metadata | Music.app-derived local view; optional Apple API | `SUPPORTED` |
| Recently Played | Yes | `/v1/me/recent/played` and related history resources | Recently played requests | Played date/count per track | Apple Music API | `SUPPORTED` |
| Recommendations | Yes | Personal recommendations endpoints | API access | No general recommendation interface | Apple Music API | `SUPPORTED` |
| Listen Now | Yes | Recommendations and history building blocks, not guaranteed desktop-tab parity | Same API building blocks | Not scriptable as a data surface | Apple Music API-composed view | `PARTIAL` |
| Browse | Yes | Catalog charts, genres, activities, curators, playlists, and search; no desktop-layout contract | Same catalog building blocks | Not a general data surface | Apple Music API-composed view | `PARTIAL` |
| Charts | Yes | Catalog charts endpoint | Typed chart requests | Not a catalog data interface | Apple Music API | `SUPPORTED` |
| Genres | Yes | Catalog genres endpoints | Typed catalog requests | Track genre metadata only | Apple Music API | `SUPPORTED` |
| Radio | Yes | Live radio and station metadata | Station playback | Can play/open supported stream or Apple Music content | Apple API metadata + playback backend | `BACKEND_DEPENDENT` |
| Stations | Yes | Live stations, station genres, and personal station; not every desktop creation workflow | Station playback | Current stream metadata; no general station catalog | Apple Music API + playback backend | `PARTIAL` |
| Lyrics | Yes | Availability flag only; no documented lyric-text endpoint | `Song.hasLyrics`; no documented lyric-text request | Plain `lyrics` track property, availability varies by track/source | macOS plain lyrics where returned; provider abstraction otherwise | `PARTIAL` |
| Timed lyrics | Yes | No documented endpoint | No documented synchronized lyric payload API | Not exposed in the scripting definition | None | `UNAVAILABLE` |
| Artwork | Yes | Artwork metadata and templated URLs | Artwork model/view | Scriptable artwork data for library objects | Apple Music API + artwork cache | `SUPPORTED` |
| Lossless metadata | Yes | Extended `audioVariants` includes lossless and hi-res lossless | `AudioVariant` | Script dictionary does not expose a reliable lossless flag | Apple Music API metadata | `SUPPORTED` |
| Dolby Atmos metadata | Yes | Extended `audioVariants` includes Dolby Atmos | `AudioVariant.dolbyAtmos` and active player variant | Script dictionary does not expose a reliable Atmos flag | Apple Music API metadata | `SUPPORTED` |
| Explicit flag | Yes | `contentRating` is `clean` or `explicit` | Content rating property | Explicit property is available for tracks where Music.app reports it | Apple Music API | `SUPPORTED` |
| Play count | Yes | Not a general LibrarySongs REST attribute | `Song.playCount` where available | Writable `played count` | MusicKit/macOS | `BACKEND_DEPENDENT` |
| Date added | Yes | Present for relevant library resources and recently-added results | `libraryAddedDate` | Read-only track `date added` | Apple Music API / macOS | `SUPPORTED` |
| Smart playlists | Yes | No smart-rule model or smart-playlist management API found | No smart-rule management API found | User playlists expose a read-only `smart` flag | macOS read-only discovery | `PARTIAL` |
| Playlist folders | Yes | Read and create library playlist folders | API access | Folder playlist class and playlist parent | Apple Music API / macOS | `SUPPORTED` |

## Locally verified Music.app surface

The installed Music.app 1.6.5 scripting definition and live JXA behavior were rechecked on 2026-08-02 before implementing the macOS backend.

| Music.app operation | Local verification | Milestone 2 exposure |
|---|---|---|
| Installed/running detection | The application bundle and `Application("com.apple.Music").running()` are available without launching Music.app | Exposed; startup never launches Music.app |
| Player state and position | Live structured query returned `paused` and a numeric `player position` | Exposed and polled at 500 ms |
| Current title/artist/album/duration | Live current-track reads succeeded, including Unicode-safe JSON transport | Exposed |
| Persistent ID/database ID | Both are documented. JXA requires `persistentID()`/`databaseID()` exact acronym casing; valid IDs were observed after correcting the former casing bug, while some streamed items may still omit them | Persistent ID is preferred, then database ID; otherwise title + artist + album + duration form an explicitly ephemeral identity |
| Play, pause, toggle, next, previous | Commands are present in the installed scripting definition under the playback access group | Exposed |
| Seek and sound volume | Properties are documented read/write; live changed-value writes succeeded | Exposed |
| Mute | Documented read/write, but both JXA and AppleScript changed-value writes returned Music.app error 9038 | Read/display only; mutation capability disabled |
| Shuffle | Documented read/write; live changed-value writes succeeded | Exposed |
| Song repeat | Documented read/write; live changed-value writes succeeded after Music.app's asynchronous state update | Exposed and reconciled by the next poll |
| Current playlist | Read-only and described as the playlist containing the targeted track | Not treated as Up Next |
| Up Next, Play Next/Later, queue reorder | No corresponding class, property, or command exists in the installed definition | Unsupported; all production queue capabilities disabled |
| Library tracks/playlists | Bulk playlist properties and bounded track property arrays were live-verified; some concrete playlist ranges need selected-property per-track fallback | Implemented progressively with explicit local source/loading state |
| Selected item playback | `play` accepts track or playlist specifiers; `whose` resolves persistent/database identifiers | Implemented for library tracks, playlist-context tracks, and playlists; normal user-playlist path live-verified |
| Favorites and ratings | Writable properties are documented, but mutation behavior was not exercised for this milestone | Capability remains disabled until dedicated mutation tests exist |

The 2026-08-03 runtime audits exercised the real TUI and installed library: startup playback synchronization; external state/seek changes; TUI playback controls; 107 discovered playlists; bounded loading of a 12,997-track library; real user-playlist names, track metadata, navigation, and contextual playback; selected playlist playback; progressive status; and clean TUI shutdown. Normal tests remain fixture-only, while three explicit live tests are ignored by default.

## Verified constraints

1. Personalized `/v1/me` requests require a Music User Token in addition to the developer token.
2. MusicKit manages user tokens automatically on Apple platforms and the web. Apple documents no direct terminal token endpoint. The implemented bridge uses a loopback-only MusicKit-on-the-Web consent page and stores the result in macOS Keychain.
3. Apple Music API identifiers and personal-library identifiers are different namespaces and must remain distinct domain types.
4. REST responses expose play parameters and preview assets, not reusable full-track audio URLs. Full subscription playback remains in Apple-managed MusicKit or Music.app surfaces because of authorization and DRM.
5. The installed Music.app 1.6.5 scripting definition was inspected at `/System/Applications/Music.app/Contents/Resources/com.apple.Music.sdef`. Live structured JXA reads and no-op property writes were also checked. It documents playback controls and many library properties, but no Up Next queue interface; some streamed current items do not return persistent/database IDs reliably.
6. Apple Events automation requires explicit user consent on macOS. Packaged/hardened builds also need the Apple Events entitlement.
7. No undocumented MediaRemote or private framework is a required dependency. Such mechanisms remain out of scope unless Apple documents them publicly.
8. Developer Tokens use ES256, a 10-character Key ID header, the 10-character Team ID as `iss`, and `iat`/`exp` claims. Apple permits at most 15,777,000 seconds (six months); this application uses 30 days with a five-minute refresh margin.
9. Apple does not publish a dependable fixed Music User Token lifetime or application-managed refresh contract. The token is treated as opaque and reauthorization is required after Apple rejects it.
10. `/v1/me/storefront` is the authentication probe and authoritative account storefront. The optional configured lowercase country code is only a future pre-auth catalog default; `us` is never hardcoded.

## Primary sources

- [Apple Music API overview](https://developer.apple.com/documentation/applemusicapi)
- [User Authentication for MusicKit](https://developer.apple.com/documentation/applemusicapi/user-authentication-for-musickit)
- [Generating Developer Tokens](https://developer.apple.com/documentation/applemusicapi/generating-developer-tokens)
- [Create a media identifier and private key](https://developer.apple.com/help/account/capabilities/create-a-media-identifier-and-private-key/)
- [Get the user's storefront](https://developer.apple.com/documentation/applemusicapi/get-a-user-storefront)
- [MusicKit framework overview](https://developer.apple.com/documentation/musickit)
- [MusicKit product overview, including web playback](https://developer.apple.com/musickit/)
- [MusicKit on the Web documentation](https://js-cdn.music.apple.com/musickit/v3/docs/index.html)
- [MusicPlayer](https://developer.apple.com/documentation/musickit/musicplayer)
- [SystemMusicPlayer](https://developer.apple.com/documentation/musickit/systemmusicplayer)
- [MusicPlayer queue](https://developer.apple.com/documentation/musickit/musicplayer/queue)
- [Apple Music API playlists](https://developer.apple.com/documentation/applemusicapi/playlists-api)
- [Apple Music API ratings](https://developer.apple.com/documentation/applemusicapi/ratings-api)
- [Recently played resources](https://developer.apple.com/documentation/applemusicapi/get-recently-played-resources)
- [Recommendations](https://developer.apple.com/documentation/applemusicapi/recommendations)
- [Catalog charts](https://developer.apple.com/documentation/applemusicapi/charts)
- [Apple Music stations](https://developer.apple.com/documentation/applemusicapi/apple-music-stations)
- [Song attributes and audio variants](https://developer.apple.com/documentation/applemusicapi/songs/attributes-data.dictionary)
- [Scripting Bridge](https://developer.apple.com/documentation/scriptingbridge)
- [MediaLibrary](https://developer.apple.com/documentation/medialibrary/mlmedialibrary)
- [NSMetadataQuery](https://developer.apple.com/documentation/foundation/nsmetadataquery)
- [Manual Music library/playlist XML export](https://support.apple.com/en-au/guide/music/-mus27cd5060f/mac)
- [Apple Events entitlement](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.security.automation.apple-events)
- [macOS Automation privacy controls](https://support.apple.com/guide/mac-help/allow-apps-to-automate-and-control-other-apps-mchl108e1718/mac)
