# Tidal Forces: capability-led feature and UI plan

**Status: v0.9.0 P3a approved for publication/installation; playlist paste is next.**
Published baseline: v0.8.0 (`63a05c178fe1`).
Keep Rust/egui, native playback, one distributable executable, and the existing
GitHub release → Home Manager deployment path. Do not introduce a web runtime.
The local interactive companion is `.lavish/tidal-roadmap.html`.

## Release approval policy

For each version: implement and verify, present the native preview, resolve each
individually tracked feedback item, then wait for the user's version approval.
That approval authorizes **both publication and Home Manager installation**; it
does not authorize subsequent unapproved versions or live mutation/device tests.
Do not push master before approval because its workflow publishes automatically.
Use a development branch and `-dev` version for the next iteration. Verify the
GitHub artifact and checksum before updating the managed pin; never overwrite it
with `--install`. Preserve a running preview until a safe replacement is approved.

## Current P3a slice

The approved v0.9.0 slice adds source-occurrence selection to the
shared track table: checkboxes with mixed select-all state, Ctrl/Shift-click,
keyboard ranges across virtualized rows, loaded-only select-all and scoped copy
shortcuts. Copied public track links retain source order and duplicates; text
fields keep native selection/clipboard behavior. Bulk Play next/Add to Up next
validates the whole batch before mutation and preserves ordered duplicate entries.
Selection survives page append but resets on replacement, refresh, navigation or
account changes. Existing raw playlist occurrence indices, ETag checks and removal
confirmation remain unchanged.

Validation: 148 normal tests, strict Clippy/fmt, isolated single-instance/Secret
Service contracts, debug build and release-mode check pass. Pointer tests cover
range/toggle/check-box selection without autoplay, clipboard ownership and duplicate
copying, guarded raw-index removal, and keyboard navigation to the 10,000th virtual
row. Capacity/ID exhaustion tests verify atomic queue failure. Isolated native
smoke and synthetic layout captures were checked; no live account mutation or
system-clipboard probing was performed.

P3 is not complete. Filter/sort, clipboard paste, caches (including favorite
membership), safe bulk account operations, playlist metadata/reorder, local pins,
and capability-gated folder/cover work remain. The user approved this slice for
publication/installation and continuation; track-link paste into owned playlists
is the next reviewable slice. No live mutation testing is implied by that approval.

## Implementation history

The v0.5–v0.8 preview milestones below are consolidated in approved v0.8.0.
Older test counts and deployment statements describe those milestones, not the
current release authorization.

The initial P1 foundation is implemented in the working tree: shared theme tokens,
Library tabs, explicit/confirmed favorites editing, persistent search and bounded
back/forward history, categorized full-page Settings, saved density/zoom, compact
navigation, responsive queue placement and virtualized track rows. API writes use
the existing authenticated v1 compatibility routes; contract tests are synthetic,
not evidence of live write permission on every client/account. No live account
mutation tests were run. Back/forward reload catalog data rather than restoring a
complete cached/paginated view; deeper browse-view snapshots remain pending even
with the playback-state restoration below.

Validation for this slice: strict Clippy, 55 normal tests (two opt-in tests excluded),
the single-instance test separately under an isolated D-Bus session, optimized
v0.5.0 build, and native graphical smoke with an empty configuration and null audio
output. Native Settings was checked at 1240×820 and 1000×660; headless egui tests
also exercise high-zoom logical sizes, 10,000 track rows, favorite-button interaction,
request backpressure, account isolation, and non-autoplay navigation. No real screen
reader or live favorite mutations were exercised. This build is not published or
deployed; the installed managed player remains unchanged.

**P2a is implemented in the working tree (unreleased v0.6.0):** occurrence-aware
manual/source queues; Play next, removal, button-based reorder and scoped clears;
reversible source shuffle; off/source/track repeat; guarded album/playlist/favorites
continuation beyond the visible page; account-scoped paused restore of queue,
position, volume, quality and last browse route. A coalescing worker writes private,
versioned snapshots off the UI/audio threads and flushes on exit. A restored stream
is authorized afresh and its saved position is prepared before entering the mixer.
Late restore responses cannot replace a newer queue or explicit playback intent.
Validation now includes 80 normal tests, strict Clippy/fmt, isolated D-Bus testing,
an optimized v0.6.0 build and isolated native graphical smoke. Tests cover 10,000
mixed queue operations, 10,000-row UI rendering, source boundaries/revisions,
corrupt/account-mismatched snapshots, exit flushing, restore races and in-memory
saved-position audio preparation. No live mutation, audible playback, Cast or
credential migration test was performed. The running v0.5.0 preview was left alone.

Limits remain explicit: 50,000 source/manual entries, 100 backtracking entries and
32 MiB snapshots. Favorites count/boundary checks are not an atomic service revision;
equal-count external edits can escape detection. Full browse-page/scroll/selection
snapshots are not restored.

**P2b additions (v0.7.0-dev):** opt-in, account-scoped local listening
history with Queue/History tabs, a compact Home shelf, confirmed clear/disable
controls and bounded retention (1,000 plays / 90 days; 16 MiB files). Playback
consumption is counted independently of visualizer mode and seek position, bounded
by monotonic elapsed time; paused/buffering time earns no credit. Muted playback
does count. A play qualifies at 30 seconds or half a short track. Storage remains
separate from credentials and playback state, and failures are disclosed rather
than silently overwriting unreadable data. Normal exit flushes pending writes;
manual switches/crashes can lose the last polling/checkpoint interval.
Manual Up next now supports drag handles with insertion markers, occurrence identity
and account/context/revision guards; Up/Down buttons remain the keyboard alternative.
95 normal tests cover these additions, including actual pointer-driven drag testing,
rendered-sample/seek/pause accounting, repeat records, retention, account isolation,
clear/disable/recovery, persistence errors and virtualized History layouts. Strict
Clippy/fmt, isolated D-Bus tests and isolated development-build native smoke passed.
The optimized v0.6.0 review binary and running preview are unchanged; these new
features are only in the development source/build. No live account mutations,
audible playback or credential migration tests were run.

The user has prioritized a dedicated native visual-polish pass immediately after
P2, before broader library/audio/Cast feature work (V1 below).
**The P2 foundation implementation is now complete in v0.7.0-dev.** Explicit Linux
Secret Service migration uses write/read verification, copy-on-write refresh storage,
private metadata and recovery journals, with no silent keyring-to-plaintext fallback.
Locked/unavailable vaults and persistence failures are visible; fresh tokens remain
in memory for retry. Signed-out intent is persisted before deletion, and normal
window close requires acknowledgement when known credential work is pending or
credentials are unsaved. The app does not request automatic vault unlock prompts.
Provider security and availability remain OS-dependent; use one credential-changing
client per profile, and do not run older versions after migration.

Queue-to-playlist saving captures a fixed current/upcoming snapshot with duplicate
IDs preserved. Explicit confirmation starts bounded, account/cancellation-guarded
batches; each mutation's ETag pins ordered read-back before continuing. Missing
write ETags stop export rather than weaken revision safety. Cancellation stops
further batches but cannot undo an in-flight request; failed/uncertain operations
retain the destination for inspection and never automatically retry or delete it.

Validation: 119 normal tests, strict Clippy/fmt, isolated single-instance and fake
Secret Service D-Bus contracts, development build and isolated native smoke passed.
Failure coverage includes interrupted migrations, verification/commit failures,
locked sign-out, refresh persistence, profile separation, unsaved-exit confirmation,
205-entry exports, stale/foreign destinations, duplicate order, cancellation during
ownership reads, uncertain writes and narrow confirmation layouts. **No real vault,
real credentials or live playlist export was exercised.** Write-ETag availability
and provider-specific behavior still require separately approved live validation.
The running v0.6.0 review process and optimized binary remain unchanged; no new
release, install, Home Manager switch or audible test was performed. Browse restore
is still route-level; deeper cached/scroll/selection snapshots remain later work.

**V1 native visual pass is approved in v0.8.0.** The implemented shell
now uses consistent dark/teal surfaces, typography, focus states and vector controls;
artwork-led collection/Home headers; responsive Library grids and viewport-bounded
media shelves; aligned track columns, explicit click-to-focus/Enter behavior and
non-playing favorite/queue actions. Queue/History tiles retain readable title/artist
metadata, tested manual drag grips, compact move/remove controls and a narrow-window
right-anchored overlay. History exposes a direct recording switch with details in Settings/tooltips;
Settings groups and the player bar share the same surface/spacing system. Actual
quality labels retain full hover text; visualizer scrimming is display-only.

Review feedback has also been implemented: transient visualizer snapshot contention
retains only fresh same-epoch display data (never renewing the 180 ms deadline),
transport buttons share a centerline, the sidebar create action is an inline +,
and folders/playlist thumbnails use aligned rows with bounded indentation. Existing
provider image/squareImage fields are used without extra catalog fetches; missing
artwork gets a generic placeholder. Unknown favorite membership now uses a dropdown
indicator rather than an error-like question mark, preserving explicit save/remove
choices. Collection headers match the unframed artwork/title/action arrangement,
with Play and genuine Shuffle below the header and responsive Title/Artist/Time
columns. Initial shuffle selection is among loaded occurrences; continuation pages
join afterward, and manual order/duplicates remain intact.

Subsequent review removed the routine restoration success notice, replaced History's
privacy disclosure/static recording status with an accessible recording switch, and
matched the roadmap's compact right-aligned search field, internal icon, shortcut
badge and responsive breadcrumb. Explicit search focus closes the narrow Queue
or History overlay so it cannot obscure the field; wide docked panels stay open.
History remains opt-in, off retains records, errors stay visible and clearing still
requires confirmation. Favorite membership is still learned from Library pages and
confirmed writes, not prefetched on boot; Spotifast's batched membership queries
and account-scoped liked cache are comparison evidence, not implemented TIDAL parity.

A user-reported real keyring migration attempt failed during secret response
validation. Read-only provider identification found GNOME Keyring 46.1; its
[source](https://github.com/GNOME/gnome-keyring/blob/46.1/daemon/dbus/gkd-secret-secret.c)
returns `text/plain` irrespective of the supplied MIME type. The client now treats
MIME as advisory without relaxing session, parameter, size, byte/digest or schema
checks. The original failure was reproduced on an isolated fake service before
the fix. Updated contracts cover normalized MIME, rejected malformed/changed
responses, retained legacy credentials after failure, verified retry/cleanup and
no plaintext fallback on changed keyring contents. After the corrected preview
was loaded, the user confirmed that real migration worked. This is user-confirmed
provider interoperability, not an agent vault audit or proof of every recovery
scenario. No agent read/retry/delete of real vault items was performed during
investigation/tests; no migration or cleanup action was triggered by the agent.

Validation: 140 normal tests, strict Clippy/fmt, isolated D-Bus contracts, development
build and release-mode check passed. Pointer regressions cover manual dragging in
both queue layouts plus table favorite/add actions and keyboard playback; 10,000-row
track and media-card fixtures stay viewport-bounded. Native screenshot inspection
covers normal/minimum windows and 150% scale with a debug-only, input-disabled,
fictional catalog: no API worker, audio device, network image loader, MPRIS or
application-state store. This harness is excluded from release builds. The user
reviewed the native previews and fixes, confirmed the corrected keyring migration,
and approved publishing/installing v0.8.0 and continuing the plan. Agent credential
verification remained synthetic; successful real migration is user-reported.

P3 library/playlist tools are next. Later phases include lyrics/Now Playing,
mini player, advanced audio and Google Cast. Do not treat the interactive prototype
as implemented functionality. Publication and installation now follow the explicit
per-version approval policy above; live account mutations and audible/device tests
still require separate opt-in.

The user has **no genuine TIDAL Connect test receiver**. Using the laptop as a
receiver may be investigated only if a lawful, supported, redistributable route
exists; it is not a workaround for missing partner authorization. Defer C1 rather
than ship an unverified receiver or relabel Chromecast as Connect.

## 1. Product direction

Build a complete, focused TIDAL music player—not a Spotify feature checklist.
Port useful interaction patterns, adapt service-dependent features to TIDAL,
and omit unsupported content types. Keep the current dark/teal identity, but
replace accumulated one-off layouts with a small consistent component system.

Core principles:

- Browsing never implicitly replaces playback. Play actions explicitly do.
- One authoritative playback state serves the main window, mini player, MPRIS,
  keyboard shortcuts and future remote control.
- Local preferences, TIDAL account changes, and remote-device actions are
  visibly different. Pinning locally must not silently rearrange the account.
- Every control reflects an implemented, authorized capability. Do not ship
  speculative Connect devices or permanently disabled podcast tabs.
- Source audio format, local DSP, and device output are different facts. Preserve
  accurate FLAC/AAC labels; do not imply bit-perfect or verified remote hi-res.
- Keep network, disk, decoding preparation and FFT work off the UI callback;
  do not introduce blocking work into the audio callback.

## 2. Research and porting decisions

### Evidence and limits

The public TIDAL OpenAPI snapshot inspected is **1.10.157**. An endpoint appearing
in that schema does **not** establish access: operations carry an access tier.
Favorites and core playlist mutations are `THIRD_PARTY`; lyrics, artwork upload,
cover mutation, folders and `userPlaybackStates` operations inspected are
`INTERNAL`. Existing native/compatibility routes may differ. Verify the current
client's authorized capabilities before choosing an adapter or requesting new
consent; do not replace the working sign-in/streaming path wholesale.

TIDAL's official Connect page says integrations are currently supported only
for device partners. Community code is a research lead, not proof of working
Connect: Tideway's real-connect module describes itself as hardware-unverified;
its older OpenHome path must not be mistaken for genuine TIDAL Connect.

No podcast or episode resources were present in the inspected API paths or
schemas. This does not prove that no spoken-word recording exists in TIDAL:
ordinary catalog tracks still work. It does mean there is no supported podcast
subsystem to port on the evidence available.

### Feature disposition

| ID | Feature | Decision and UI destination |
|---|---|---|
| F01 | Favorite/unfavorite tracks; saved albums/artists | **Build.** Library tabs, row/player hearts, album/artist save controls. Third-party collection APIs exist; validate grant/scopes and batching. |
| F02 | Playlist rename, description, delete/unfollow, track reorder | **Build after contract tests.** Playlist header menu and shared track table. Distinguish deleting an owned playlist from removing a followed one from the collection. |
| F03 | Multi-selection, drag/drop, cut/copy/paste links | **Build.** Shared track table, selection toolbar and reusable playlist picker. Retain occurrence identity and revision guards. |
| F04 | Search within lists, sorting and full-context playback | **Build.** Collection toolbar. Play all must include later pages, not just the currently loaded 100 tracks. Filtering/sorting must not silently fall back to an unfiltered queue. |
| F05 | Queue reorder, insert/play-next, remove, clear, save as playlist | **Build.** Queue panel with separate manual Up next and source-context sections. These are local-player features; remote support is capability-dependent. |
| F06 | Restore song/position/queue/volume/navigation | **Build.** Restore paused by default; retain the visualizer setting and existing authentication. Use a separate, versioned player-state file. |
| F07 | Listening history | **Build locally.** History panel + compact Home shelf. Count listening time, not seek distance; offer clear/disable and account separation. No uploading or fabricated TIDAL history. |
| F08 | Synced/plain/full-screen lyrics | **Conditional build, TIDAL first.** User-selected presentation: click album artwork for full-screen Now Playing, cover left and lyrics right; never a lyrics drawer. The compatibility library exposes lyrics/subtitles, but verify access; public schema lyrics are internal-only. Plain-text/unavailable states; LRCLIB only with explicit opt-in. |
| F09 | Gapless and transport/volume smoothing | **Build.** Audio pipeline; no new main navigation. Prefetch the next decoder into bounded storage; test real track boundaries and encoder padding, not just DASH fragments. |
| F10 | Output picker, EQ, preamp, balance, mono, normalization | **Build in stages.** Devices popover and Playback settings. DSP defaults off. Validate TIDAL gain/peak units before normalization; missing gain means unavailable, not an invented value. |
| F11 | Repeat-one, persistent shuffle, autoplay | **Build repeat/shuffle.** Player controls. **Adapt autoplay** from available TIDAL track/artist recommendations, with deduplication and bounded fetching; do not promise Spotify's resolver or endless unique tracks. |
| F12 | Radio browse/refresh/save and other seeds | **Build browse/refresh/save** using existing track/artist radio. Album/playlist seeds stay gated: no corresponding route was established; any composite recommendation feature must be explicitly labeled, not called native TIDAL radio. |
| F13 | Navigation history, command shortcuts, accessibility | **Build into the foundation.** Back/forward, persistent search entry, focusable semantic controls, shortcut help and virtual-row navigation. Preserve Ctrl+K; add familiar aliases rather than breaking existing keys. |
| F14 | Local pins, section sorts, compact/resizable library | **Build.** Left navigation and Library. Pins/custom display order are local; TIDAL folder order remains separately labeled. |
| F15 | Appearance, density, zoom, themes, translations | **Build tokens, contrast, density and zoom first.** Light/system modes next; custom palettes and translation catalogs later. Do not sprinkle unstructured styling or strings across feature modules. |
| F16 | Mini player, always-on-top, Winamp skins | **Build a branded mini player first.** Same actions/state as the main window. Respect Wayland window-manager limits. **User decision:** schedule classic `.wsz` skins after the core releases as an optional mode. They are not the default design direction. |
| F17 | MilkDrop/projectM | **Optional later feature.** Visuals launcher in player overflow, separate native visual window. Gate on licensing, binary size, GPU stability and optional preset-download consent. Not required for core parity. |
| F18 | Tray/background playback | **Build for Linux desktops that support it.** Keep-playing-on-close is opt-in; closing currently exits, so do not silently reverse that default. |
| F19 | OS credential storage | **Build with safe migration.** Linux Secret Service first; preserve legacy credentials until keyring write/read verification succeeds. Never silently discard sign-in or write fresh plaintext as an invisible fallback. |
| F20 | Virtualization, persistent metadata/art cache | **Build incrementally.** Visible rows only; bounded, account-scoped metadata caches and disposable artwork cache. Not offline music downloading. |
| F21 | Dedicated command-line controls | **Build after central actions.** Reuse existing single-instance IPC and command dispatch; retain MPRIS/playerctl. No separate controller with divergent queue semantics. |
| F22 | TIDAL Connect | **Research now, implement if the gates pass.** Controller/sender first; a Connect receiver on this computer is a separate, more restricted project. See section 7. |
| F23 | Podcasts / audiobooks / Spotify-only social features | **Omit.** No supported matching TIDAL resource model established. Do not add empty navigation or substitute unrelated content. |
| F24 | Other platforms and self-updating | **Defer native Windows/macOS releases** until Linux behavior is stable. Add platform adapters then. **Do not add a self-updater for Home Manager**; optional unmanaged packaging can be considered separately. |
| F25 | TIDAL folder editing | **Conditional TIDAL-specific addition.** python-tidal exposes creation, rename and moving entries even though Spotifast cannot edit Spotify folders. Verify current authorization and deletion semantics on temporary folders only. |
| F26 | Playlist cover upload / visibility | **Capability-gated.** Public metadata editing does not imply image-upload permission; cover mutation/upload are internal-tier in this schema. Ship name/description first; do not advertise unverified image upload. Validate supported visibility transitions separately. |
| F27 | Google Cast / Chromecast output | **Plan and validate on the user's two Chromecast devices.** Native sender/control integration, distinct from TIDAL Connect. Chromecast Audio first, regular Chromecast second; prove an authorized media-delivery path before promising codec/quality/gapless support. |

Not part of this parity plan: offline downloads/DRM workarounds, local-file
playback, audio crossfade, playback-speed changes, exclusive/bit-perfect output,
DLNA/AirPlay, synchronized multi-room, or scrobbling to third parties.
These could become separately scoped projects. Google Cast is now explicitly in
this plan following the user's Chromecast hardware clarification; it is not being
silently substituted for genuine TIDAL Connect.

## 3. Information architecture

### Stable application shell

**Left:** Home, Search, Library; then local Pins and the account's Playlists /
Folders; account/settings at the bottom. Library replaces the ambiguous current
“My collection” destination. Library has Tracks, Albums, Artists and Playlists
tabs, with saved mixes added only if useful and supported. Keep personal mixes
and Daily Discovery prominent on Home rather than creating a new navigation
item for every recommendation type.

**Center:** the active page, with consistent top navigation, a contextual header,
action row and content area. Back/forward preserves scroll/filter/sort/selection
for bounded recent routes. The global search field always targets Search;
“Filter this playlist” is a separate, clearly scoped field.

**Right:** one optional resizable side panel with **Queue / History** tabs.
Remember the chosen tab and width. Queue controls remain available while browsing
Settings or another collection. **Lyrics never occupy this drawer.**

**Full-screen Now Playing — user decision:** clicking the player-bar album artwork
opens a TIDAL-web-style focused view: large album artwork and track details on the
left, lyrics on the right, playback controls below. Hide navigation, search and the
Queue/History drawer in this view; this is a dedicated full-window surface, not an
expanded side panel. A close/collapse control or Esc returns to the exact prior
page, scroll, selection and panel state without interrupting playback. Optional
OS-level fullscreen is a separate action and restores the prior window geometry.
The artwork stays a Now Playing button even when lyrics are missing. Keep the
existing right-click artwork context menu; clicking album/artist text navigates
normally rather than entering this view.

**Bottom:** a persistent roughly 96–112 logical-pixel player, with three zones:

1. Artwork, title/artist links, favorite and source-format detail.
2. Shuffle, previous, primary play/pause, next, repeat; seek below.
3. Playback target (local / Google Cast / TIDAL Connect), volume, panel toggles and overflow.

Keep mini-player, visualizer modes and less-common actions in overflow. Put EQ
and normalization in Playback settings, reachable from output/quality details.
No permanent row of a dozen feature icons. Show an active DSP indicator and an
actual remote-target label when relevant; do not repurpose the source-quality
badge as an unsupported output-quality guarantee.

### Page patterns

- **Home:** Daily Discovery, personal mixes, recently played and saved/pinned
  shortcuts. No fabricated editorial recommendations or podcast shelf.
- **Collection:** modest cover/title/count header; Play, Shuffle, Save and overflow;
  then filter/sort/density controls and one shared virtualized track table.
  Avoid the current oversized blank header area for small result sets.
- **Artist/album:** use the same header, actions and track-row vocabulary; add
  catalog sections only when backed by returned TIDAL data.
- **Selection:** Ctrl/Shift-click and keyboard range selection. A contextual bar
  shows count and permitted bulk actions. Text editing keeps native clipboard keys.
- **Queue:** Now playing, manually queued Up next, then source context. Dragging
  here edits playback order, not the saved TIDAL playlist. Undo local queue changes.
- **Now Playing / lyrics:** open through album artwork, an accessible Now Playing
  button, or L. Large artwork stays left and lyrics right at supported desktop sizes;
  reduce artwork before compromising readable lyrics. Synced lines follow playback;
  scrolling suspends follow, a Follow button resumes it, and clicking a timed line
  seeks. Plain lyrics do not offer invented timestamps. Missing/loading lyrics use
  an honest right-side state, not a drawer fallback. No automatic full-screen entry
  on song change. Preserve this view across tracks, and restore browsing state on exit.
- **Settings:** a full center-page surface with General, Appearance, Playback,
  Library & Privacy, Shortcuts and About. Search settings without moving the
  player; reserve modal dialogs for focused editing/confirmation, not all settings.
- **Devices:** a popover opened from the player, grouped by **This computer**,
  **Google Cast**, and, only when implemented, **TIDAL Connect**. Label Chromecast
  targets as Cast—not Connect. Do not call an OS audio device “Connect.” A remote
  section appears only when an implemented discovery capability is enabled.
- **Playlist/folder editing:** reusable forms with explicit ownership/permission
  states; image editing stays hidden until implemented and authorized.

### Responsive behavior

Use measured content minimums, not rigidly fixed panel widths:

- At approximately 1280 logical pixels and above: full left rail + center + optional
  right panel (suggested starting widths 216–260 / flexible / 280–360).
- Around 1000–1279: shrink secondary columns first; use compact navigation or let
  the side panel overlay the page instead of squeezing the track title into nothing.
- At smaller sizes / high UI zoom: the side panel becomes a dismissible overlay
  and the navigation becomes a drawer. Preserve transport/seek and keyboard access.
- Player metadata truncates with accessible full names; rare controls move into
  overflow before primary controls disappear. Do not reduce fonts to make it fit.
- Validate at the existing 1000×660 minimum, 1280×800, 1920×1080, multiple scale
  factors and with unusually long translated titles. Numbers are initial design
  targets, to be adjusted against actual native layout measurements.

## 4. Cohesive visual system

Retain current anchors: background `#0C0D0F`, panels `#121316`, raised surfaces
`#1C1E22`, muted text `#979BA5`, teal `#51E1DB`, and embedded Inter fonts.
Move them into semantic tokens before making a second theme.

- A 4/8-based spacing scale: 4, 8, 12, 16, 24, 32. Shared row heights of roughly
  56 comfortable / 40–44 compact; consistent 32–36 pixel utility targets.
- Typography roles, not per-widget arbitrary sizes: 28–32 page title, 18–20 section,
  14–15 body, 12–13 metadata. Tabular numerals for durations and queue positions.
- One vector icon family, consistent optical size/stroke. Extend/standardize the
  native icon painter; stop mixing glyphs, emoji, empty-looking square buttons
  and hand-drawn shapes of different weights. Provide labels and accessibility names.
- Stable radius/stroke vocabulary: small controls, medium cards/popovers, light
  separators. No gratuitous gradients, glass effects or heavy shadows.
- Teal means primary action/selection; amber/red have distinct warning/error roles.
  Differentiate hovered, selected, focused, playing, unavailable and pending rows.
  A playing marker is not a substitute for selection or keyboard focus.
- Artwork is the rich visual element. Default album-derived color stays in the
  visualizer/subtle accents; optional global tint must preserve contrast.
- The visualizer stays subdued behind controls; add a readability scrim if needed.
  Reduced-motion mode suppresses decorative animation, with explicit visualizer
  override rather than quietly breaking the user's choice.
- Use one button, menu, row, artwork, popover, empty state, toast, and dialog system.
  New features should compose these, not add another one-off design vocabulary.
- Loading: preserve prior content during refresh; skeletons only for initial loads.
  Row-level pending indicators for mutations. Inline errors for their owning page;
  actionable, time-bounded toasts for routine results; persistent banners for
  authentication/device failures. Do not overwrite every error into one global slot.
- Accessibility: AccessKit support, semantic rows/actions, visible focus, sufficient
  contrast, keyboard equivalents for drag/drop and no color-only status. Verify
  Linux screen-reader behavior rather than assuming enabling a crate feature is enough.

## 5. Data and architecture changes

The current `ui.rs` is about 2,300 lines and `App` mixes navigation, catalog data,
playlist dialogs, auth, queue and playback. `Backend` awaits catalog/mutation
requests in one loop; `audio.rs` replaces the sink on each track; `store.rs`
persists auth and visualizer preference, not playback state.

Refactor incrementally while retaining egui, the existing decoder code and the
request/event architecture. No new app framework, generic plugin system or
service abstraction for its own sake.

Suggested destination boundaries (extract as the corresponding slice lands):

- `ui/mod.rs`: App shell/composition, replacing the monolithic `ui.rs`.
- `ui/theme.rs`, `ui/widgets.rs`, `ui/track_table.rs`: tokens, semantic components,
  virtualization, selection, occurrence-aware actions.
- `ui/navigation.rs`, `ui/library.rs`, `ui/player_bar.rs`, `ui/side_panel.rs`,
  `ui/now_playing.rs`, `ui/settings.rs`: feature surfaces, not independent sources
  of playback truth. The full-screen lyrics view is separate from `ui/side_panel.rs`.
- `actions.rs`: shared user intent dispatched by widgets, keyboard, IPC and MPRIS.
- Focused state structs: navigation, catalog/library, playback, dialogs, preferences.
  Do not make separate copies of the same mutable state in each view.
- Evolve `queue.rs` into occurrence-aware playback state. Keep a small controller
  boundary around `audio.rs`; add the remote implementation only after feasibility.
- Extend `api.rs` in focused modules as it grows; preserve existing authorization,
  stream validation, revision and error semantics.
- Add narrowly scoped persistence/history/credential modules. Reuse atomic serde
  files for small state; do not add a database before cache/history scale requires it.

### Queue and identity contracts

- Each queue entry gets a unique local occurrence ID, independent of TIDAL track ID.
  Playlist rows retain provider identity/raw index + revision. Duplicate songs
  remain independently selectable, removable and resumable.
- Distinguish queue history, current item, manual upcoming entries and context
  continuation. Manual Play next does not get shuffled unexpectedly. Explicitly
  playing a new collection replaces its context, not the manually curated Up next
  list. Clear Up next clears manual entries; clearing the entire playback source
  is a separate explicit action. Initially, drag reorder targets the manual section,
  as in Spotifast; it must not appear to edit a read-only context or saved playlist.
- Repeat = off / context / track. Natural end honors repeat-one; explicit Next
  advances. Shuffle preserves enough original upcoming order to turn it off.
- Context playback knows the source, revision, paging cursor and displayed-order
  policy. Fetch continuation before reaching the end of a loaded page. Cancel
  stale prefetch on replacement/seek/queue edits. Never mix inconsistent revisions.
- Full filtered/sorted playback requires a complete matching order or a verified
  server-side operation. While indexing, label incomplete results; do not pretend
  sorting the first page sorted the whole playlist. Preserve existing valid audio
  while a larger view is prepared.

### Position and lyrics contracts

- Replace whole-second-only UI position with a duration/millisecond snapshot plus
  an observation time and playback state. Extrapolate smoothly only while playing,
  reconcile seeks/pauses, and stop extrapolating when a remote snapshot is stale.
  Keep MPRIS updates bounded rather than broadcasting on every animation frame.
- Parse timed subtitles defensively: bounded input, valid finite timestamps, stable
  ordering and explicit handling of duplicate/absent timing. Keep usable plain text
  when synchronization cannot be trusted; never invent click-to-seek timestamps.
- Follow uses the current playback target's confirmed position. Manual scrolling
  pauses follow; selecting Follow restores it. Switching tracks keeps Now Playing
  open but resets lyrics state. Exiting it restores browsing focus and state.
- Preserve provider attribution and writing direction; test fallback fonts and
  right-to-left text before claiming complete language support. A lyrics-provider
  failure does not pause playback or force the user out of Now Playing.

### Async and mutation contracts

- Prioritize playback work; use bounded/cancellable catalog jobs. Avoid an API
  mutex held across long network requests. Serialize token refresh, and serialize
  mutations per relevant account object; discard stale view events by generation.
- Favorite state is known/unknown/pending, not a guessed bool. Coalesce explicit
  desired-state changes; avoid one network lookup per visible row.
- Batch operations retain original occurrence identities through sort/filter.
  Apply revision guards wherever supported. Reorder/index-based deletion must not
  silently proceed on an unverified snapshot; refresh/reconcile on conflict.
- A timeout after a mutation is an uncertain result, not proof of failure.
  Refresh before retrying; do not duplicate writes or blindly replay deletion.
- Preserve current safe duplicate skipping; make any future “allow copies” action
  explicit and conditional on tested API behavior.
- A local pin uses no service mutation. Moving an entry between account folders
  requires collection permission, not necessarily ownership of the playlist's
  contents. Reject folder cycles; test nonempty deletion semantics before exposing it.
- No fake Undo for an irreversible/uncertain account operation. Confirm destructive
  actions; reliable local queue operations can offer immediate Undo.

### Storage and migration

- Keep credentials separate from `appearance.json`, versioned `player-state.json`,
  local history and disposable metadata/artwork caches. Account-scope personal data.
- Restore stopped/paused; pressing Play resolves current authorized streams and
  prepares the saved position. Never persist signed playback URLs or access tokens
  in queue/cache files. No unprompted remote-device reconnection on startup.
- Migrate existing appearance fields without resetting mode. Missing/corrupt new
  state files degrade gracefully and must not erase working credentials.
- Keyring migration: write secret, read back/verify, atomically mark metadata
  migrated, then remove legacy secret material. Handle locked/unavailable keyring,
  refresh persistence failures and interrupted migrations explicitly. Never delete
  the only good credential copy. The UI must disclose legacy-file mode until migrated.
- History: opt-in (off by default), bounded retention, atomic/batched writes,
  clear/disable controls. Disable retains existing records; Clear removes them.
  Use monotonic actual-play time; seeking and pausing do not count, repeat occurrences
  do. Do not claim to reproduce TIDAL's server-side listening history.
- Cache: byte budgets, expiry, ETag/revision checks, account separation and Clear
  cache. No persistent audio cache in this plan; existing bounded streaming stays.

## 6. Audio work

Keep gapless separate from optional DSP and from crossfade (out of scope).
A continuous local output path should consume current and prepared-next sources
without the current stop/recreate-sink transition. Move any blocking stream reads
and decoder preparation behind a bounded PCM handoff; the output callback must
not wait for a network/disk cache miss. An underrun is a buffering condition, not
listening time or permission to fabricate playback progress. Prefetch authorization,
initialization and a bounded amount of audio—not entire albums. Cancel work on
queue changes and keep a clear fallback when data/device format is unavailable.

Proposed local chain: decoder → validated gain normalization (optional) → EQ /
balance / mono (optional) → visualization tap → output-volume/ramp stage → OS output.
Apply explicit headroom/clipping policy when DSP boosts. Final limiter placement
and any normalization compensation for the visualization need measured tests;
never add an always-on processing stage to an otherwise bypassed path by accident.

- With DSP disabled, decoded samples should remain unchanged apart from existing
  output conversion, intentional volume/transport ramps and OS mixing.
- Verify same-format gapless boundaries sample-for-sample. Test mixed rates,
  channels and codecs; account for AAC priming/padding and FLAC end boundaries.
  Do not promise sample-perfect transitions across unsupported conversions.
- Request source quality independently of output format. A source-format label
  cannot prove the DAC's output format. Provide a details popover for what is known.
- Output selection/hotplug recovery must not require restarting the app. If
  headphones/remote output disappear, pause and ask where to resume rather than
  suddenly playing through speakers. Remember preference, not a stale device handle.
- Local EQ/normalization do not affect genuine Connect remote playback. Reflect
  this in settings and disable the local PCM visualizer for remote playback.

## 7. Network playback: Google Cast and TIDAL Connect are separate tracks

### Confirmed direction and test targets

The user corrected the earlier USB answer: use **Chromecast Audio** as the primary
network playback test target and a **regular Chromecast** for compatibility tests.
These are Google Cast devices, not evidence of TIDAL Connect support. The intended
flow is **Tidal Forces → Chromecast**, not phone → this PC → USB audio.

The two user-provided LAN addresses are recorded in the ignored local planning
file `.lavish/network-test-targets.json` and the local review artifact. Do not put
machine-specific addresses in application defaults, public fixtures or release
assets. They are supplied targets, not discovered/verified device identities;
confirm model, generation and firmware when testing begins. No network probes,
receiver launches or playback were performed as part of this planning update.

**Exclude this computer's USB audio device from Connect/Cast validation.** It is
only a local audio output; ordinary local-output work remains separate.

### Google Cast: feasibility and implementation milestone G1

1. Start with the supplied devices during an explicitly scheduled device test;
   use bounded discovery/manual address support rather than an unsolicited LAN
   sweep. Confirm receiver identity/capabilities and whether the device is busy.
2. Choose and prove a native Rust Cast sender/control path, preserving the single
   executable and no-browser-runtime constraint. Evaluate the authorized receiver
   application and media-loading contract; do not assume TIDAL's official Cast
   receiver accepts an arbitrary third-party session.
3. Prove authorized media delivery. A clear DASH manifest or signed stream URL
   that plays locally is not automatically Cast-compatible. Verify receiver codec,
   transport, authorization, URL expiry and seek/range behavior. Prefer receiver
   fetch of compatible authorized media. DRM circumvention remains out of scope.
4. If a native LAN relay is necessary, make that an explicit design gate: bounded
   buffering, narrow interface binding, short-lived unguessable access URLs,
   receiver-scoped access, no credential forwarding/logging, and shutdown when the
   cast session ends. Do not silently add transcoding; disclose any actual format
   conversion and its quality impact. No external player processes or browser.
5. Validate on Chromecast Audio first, then the regular Chromecast (generation
   currently unknown). Start with a permitted synthetic fixture at a safe volume,
   then authorized FLAC/AAC tracks. Request confirmation before interrupting an
   existing session or producing audible output. Do not claim identical hi-res,
   codec or gapless behavior across both devices without measurement.
6. Cover discovery/reconnection, play/pause/seek, volume, queue advancement, long
   playback and URL expiry, receiver reboot, LAN loss and another sender taking
   over. Never start local speaker playback automatically on a cast disconnect.

Use the shared playback-target boundary and Devices popover. Receiver state owns
transport/position; keep the local queue independent. Capabilities determine
whether remote queue editing is available. For direct remote playback, local DSP
and the PCM visualizer are inactive; do not decode a second stream just to animate
bars. Any later relay/DSP mode must declare its actual processing explicitly.

**G1 exit:** both supplied devices have repeatable playback/control tests and an
understood authorized transport, or report a specific per-device limitation.
Successful Chromecast playback validates **Google Cast**, never TIDAL Connect.
Cast failures must not be disguised by playing through the USB/local output.

### TIDAL Connect: independent feasibility track C1

Retain the original request to investigate genuine TIDAL Connect. Controller
support for an authorized network speaker/streamer remains conditional on an
acceptable integration route and an actual Connect-capable test receiver.
Neither supplied Chromecast is assumed to provide that capability. The user has
no other Connect devices and has left laptop-receiver feasibility to our discretion.
Keep receiver work deferred unless a lawful supported integration path is proven;
do not add an unverified network listener or let it block Google Cast/core releases.

### Connect research gate — start early, timebox to one investigation milestone

1. Identify a real user-owned test device and firmware, with consent for local
   discovery/testing. Verify it works with the official TIDAL app on the same LAN.
2. Confirm supported partner/developer access and applicable client authorization.
   Public schema `userPlaybackStates` entries are internal-tier, not an open
   shortcut to remote control. No client-ID guessing, borrowed private keys,
   vendor-binary redistribution, certificate-bypass workaround or DRM bypass.
3. Evaluate independently implementable controller interoperability. Community
   `_tidalconnect._tcp` / secure-WebSocket leads are hypotheses to verify, not a
   published specification. Review source provenance and licenses before reuse.
4. Prove discovery, authenticated identity/trust, track handoff, play/pause/seek,
   volume and authoritative state updates against real hardware. Mock discovery
   or HTTP 200 responses do not satisfy this gate.
5. Record supported operations, auth lifecycle, queue ownership and quality
   reporting. Check disconnects, account refresh and another controller taking over.

**Exit A:** a small working, authorized native controller proof with repeatable
hardware tests → schedule implementation.

**Exit B:** partner access/authorization/hardware trust cannot be established →
document the exact blocker and defer Connect. Ship the rest. Do not leave fake
controls or silently substitute another network protocol. The explicitly planned
Google Cast track can ship independently, labeled as Cast; DLNA remains out of scope.

### Integration after the gate

Use a single playback snapshot plus target-specific capabilities, not two player
UIs. Local audio consumes PCM; Connect is a control plane, not another PCM sink.
The selected target owns transport, position, metadata and queue authority.

- Main player, mini player, MPRIS and CLI all route through the same selected target.
- Separate “transfer this playback” from “control what that device is playing.”
  Transfer only what the protocol supports; explain if the queue cannot transfer.
- Stop/quiesce local output before a confirmed remote transfer; avoid double audio.
  Do not automatically seize a device after a phone takes over.
- Preserve the local queue independently. Show remote queue read-only if edits
  are unavailable. Never send speculative queue actions to a device.
- Cancel stale target events with generations, bound reconnection/backoff, validate
  advertised endpoints and payload sizes, and redact credentials/stream URLs.
- No fake remote spectrum: no decoded local PCM means the visualizer is inactive.
  Show verified receiver format or “format not reported,” not the old local badge.
- Discovery is explicitly enabled, LAN-scoped and bounded. No permanent discovery
  CPU/network churn when unused. No insecure certificate fallback to make a demo work.

## 8. Delivery sequence and exit criteria

Each stage should contain small releasable vertical slices, not a flag-day rewrite.
Google Cast feasibility and genuine Connect research can proceed as separate
workstreams without delaying core features. Cast does not depend on Connect
partner access. This is sequencing guidance, not authorization to delegate work.

| Stage | Deliverable | Exit criteria |
|---|---|---|
| P0 — baselines & gates | Source/capability ledger; recorded UI/audio/performance baseline; scoped account probes; Connect feasibility report. | Confirm scopes and endpoint contracts; document unsupported/unknown features. No changes to existing playlists/favorites. Record hardware dependency for Connect. |
| P1 — cohesive shell & library | Semantic tokens/components, modular UI, navigation history, full-page Settings, shared virtualized tables, Library tabs and favorites editing. | Existing playback/links/playlist actions still work; keyboard/focus/accessible names correct; 1000×660 layout passes. Clearly label loaded-only playback until P2; do not expose unimplemented filter/sort controls. |
| P2 — queue, state & privacy | Occurrence-aware queue, manual/context separation, full-context pagination, reorder/clear/save, repeat-one/shuffle, paused restore, local history, keyring migration. | Play all continues beyond the first loaded page; duplicate-occurrence and restart/crash/account-isolation tests pass; locked-keyring recovery preserves credentials; no audio starts on launch. |
| V1 — dedicated native visual polish (after P2, user-prioritized) | Bring the implemented shell, Home/Library cards, track tables, Queue/History and player bar toward the approved dark/teal design; cohesive hierarchy, spacing, typography and responsive interaction states. | Native layout/keyboard tests and review at minimum, normal and high-zoom sizes. No mock-only controls or claims that future lyrics/Now Playing/mini-player features exist. Runs before P3 and broader P4/P5 work. |
| P3 — account organization | Multi-select, clipboard, playlist metadata and safe reordering, local pins, filter/sort, caching; gated folder/cover operations. | Temporary-object live round trips and cleanup; stale ETag, video holes, unavailable rows and partial-write failures covered. Unsupported mutations absent from UI. |
| P4 — audio polish | Prepared-next playback, gapless where supported, ramps, local output selection/recovery, optional DSP and verified normalization. | Boundary/seek/pause tests across FLAC and AAC variants; bypass sample checks; no blocking capture/DSP allocations on callback; no surprise speaker fallback. |
| P5 — lyrics & daily-use surfaces | Artwork-click full-screen Now Playing (large cover left, TIDAL-first lyrics right), branded mini player, opt-in tray, shared CLI commands, bounded radio continuation. | No lyrics drawer; Esc restores prior page/scroll/selection/panel; missing/unsynced lyrics graceful; seek/follow works; all controls target one player state; no duplicate history; no new external hosts without consent. |
| G1 — Google Cast output | Native Cast sender and Devices UI; authorized media-delivery proof on Chromecast Audio, then the regular Chromecast. | P2/P4 playback-target boundary ready; both devices' control/quality/lifecycle results recorded; no local fallback or leaked credentials; precise limitations disclosed. Independent of C1. |
| C1 — Connect implementation, conditional | Native discovery/controller + Devices UI using the established playback boundary. | P0 feasibility passed, P2/P4 state/output boundary stable, real hardware lifecycle tests passed. Unsupported remote operations remain unavailable. |
| P6 — expansion after core releases | Light/system themes, translation catalogs, custom palettes; **classic Winamp skins scheduled per user choice**; MilkDrop remains optional; later native platform ports. | Design/accessibility/performance/license review, no regressions to core listening, no self-updater on Home Manager. |

A stage should be split further if it is too large for safe review. In particular,
favorites, queue persistence and keyring migration can each ship independently;
do not delay these useful improvements until every optional feature is complete.

## 9. Validation and release policy

- Keep formatting, strict Clippy, normal unit/integration tests, isolated D-Bus
  activation and graphical smoke checks. Never run every ignored test blindly.
- Add contract fixtures for API tiers, null/unknown metadata, permission refusal,
  pagination, transient failures and ambiguous mutation outcomes.
- Shared table tests: duplicate IDs/occurrences, sort/filter selection mapping,
  keyboard selection, copy/paste while editing text, drag auto-scroll, enormous
  collections and mixed unavailable/video rows.
- Screenshot/interactions at multiple sizes/scales for Home, Library, empty/error
  states, selection, queue, settings, lyrics and the mini player. Compare semantic
  states as well as pixels. Test keyboard-only and a real Linux screen reader.
- Establish performance budgets against v0.4.0 on the same machine and fixtures:
  startup-to-interactive, idle CPU, RAM/GPU cache, scrolling frame time and audio
  underruns. Use large synthetic catalogs without account data. Set numeric budgets
  from the measured baseline before accepting a performance-affecting change.
- Audio fixtures cover complete-track joins, changing formats, stream stalls,
  prefetch cancellation, output loss, EQ bypass, clipping protection and visualizer
  behavior. Existing segment continuity tests are necessary but not sufficient.
- All real account mutation tests are separately authorized and opt-in. Use new
  temporary playlists/folders. For a favorite test, record and restore the original
  state; never leave an existing favorite removed. Persist cleanup identifiers
  privately if interrupted, and verify cleanup before claiming success.
- Google Cast needs physical tests on both supplied Chromecasts, not just mocks:
  compatible authorized media, seek/range behavior, volume, queue advancement,
  signed URL expiry, reboot, LAN loss, sender takeover and no double/local audio.
  TIDAL Connect requires separate equivalent tests on a genuine Connect receiver;
  Chromecast success cannot satisfy that gate.
- Publish passing master builds, verify GitHub artifact/checksum, update the narrow
  Home Manager release pin/hash and switch only when authorized. Never run the
  standalone installer over the managed binary. Preserve unrelated HM edits.

## 10. Proposed defaults and questions for review

Recommended defaults: dark/teal, comfortable rows, collapsed optional right panel,
paused restore, DSP off, local-only pins/history, close exits until tray opt-in,
TIDAL-only lyrics, no network-device discovery until enabled. The interactive
prototype opens the Queue panel to demonstrate it; that is not a default-setting
proposal. Classic skins are scheduled after the core releases as the user requested;
MilkDrop remains an optional later extra. Neither is a foundation dependency.

Confirmed user decisions:

- Lyrics: album-art-click full-screen Now Playing, cover left and lyrics right;
  no lyrics drawer.
- Mini player: branded first, then optional classic Winamp skins after core releases.
- Network playback test targets: user-owned Chromecast Audio first, regular
  Chromecast second. Private addresses are in local planning notes. Treat these
  as Google Cast targets; do not use the USB device to test Connect/Cast.
- Genuine TIDAL Connect remains a separate research goal; the Chromecast tests
  do not imply that its partner/authentication/hardware gate is satisfied.

Open decisions:

1. Should lyrics remain TIDAL-only, or may an explicitly enabled LRCLIB fallback
   send track/artist/album lookup metadata to that third party?
2. The Chromecast direction is resolved. Confirm model/generation/firmware and
   an appropriate audible-test window when implementation reaches G1. An actual
   TIDAL Connect receiver and eligible integration route remain unidentified for C1.

## Sources

- Current app: `src/ui.rs`, `src/backend.rs`, `src/audio.rs`, `src/queue.rs`,
  `src/store.rs`, `src/api.rs`, `README.md` at `83b4051`.
- [TIDAL Connect developer policy](https://developer.tidal.com/documentation/connect).
- [TIDAL API reference](https://tidal-music.github.io/tidal-api-reference/) and
  [OpenAPI schema](https://tidal-music.github.io/tidal-api-reference/tidal-api-oas.json),
  version 1.10.157; access tiers checked per operation, not inferred from resource names.
- [python-tidal favorites](https://github.com/tamland/python-tidal/blob/9c41fbe6b2f2cd9fa00dca11e83574fd929ec020/tidalapi/user.py),
  [playlists/folders](https://github.com/tamland/python-tidal/blob/9c41fbe6b2f2cd9fa00dca11e83574fd929ec020/tidalapi/playlist.py),
  [lyrics and gain metadata](https://github.com/tamland/python-tidal/blob/9c41fbe6b2f2cd9fa00dca11e83574fd929ec020/tidalapi/media.py).
  Protocol reference only; no LGPL source copied into the app by this plan.
- [Tideway real-connect source and its validation caveats](https://github.com/J-M-PUNK/tideway/blob/a506f3df02f1376c92395f4bedf13e461c383f35/app/audio/tidal_connect_real.py).
  No runtime trust/auth choices are endorsed merely because this code contains them.
- [Spotifast reference snapshot](https://github.com/crmne/spotifast/tree/12deee472ee87e2c9ed0e0f13730cbdd9a6cfa45):
  `settings.rs`, `ui/collection.rs`, `ui/queue.rs`, `lyrics.rs`, `eq.rs`,
  `history.rs`, `sink.rs`, `credentials.rs`, and documented limitations.
