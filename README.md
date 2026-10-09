# Tidal Forces

**Your TIDAL library. A native Rust desktop player. No browser engine.**

TIDAL-inspired dark surfaces, album artwork, nested playlist folders, Daily
Discovery, personal mixes and a persistent player. Inspired by
[Spotifast](https://spotifast.rocks) ([source](https://github.com/crmne/spotifast)):
Rust + egui, background network work, native audio decoding, event-driven
repainting, and a single executable. This is an independent implementation.

## Download and install

Download `tidal-forces-linux-x86_64` and `SHA256SUMS` from
[Releases](https://github.com/from-nibly/tidal-forces/releases/latest).
On Linux x86_64 (Ubuntu 22.04 / glibc 2.35 or newer), using Nushell:

```nu
sha256sum --check SHA256SUMS
chmod +x tidal-forces-linux-x86_64
./tidal-forces-linux-x86_64 --install
```

Open **Tidal Forces** from the desktop application launcher. Installation copies
the running executable to `~/.local/bin/tidal-forces` and registers an XDG desktop
entry and embedded icon. No sudo is needed. Run a new download with `--install`
and restart the app to update. The standalone installer also registers `tidal://`.

**Home Manager installations:** update the package's GitHub release URL and
checksum, add the TIDAL scheme to its managed desktop entry/MIME defaults, and
apply with `home-manager switch`. Do **not** run `--install` on top of a managed
installation or replace its executable manually.

This is one executable, not Electron, an AppImage, a webview or a wrapper around
Python/mpv/ffmpeg. It uses standard Linux runtime libraries: glibc, ALSA, your
graphics driver/OpenGL, and X11 or Wayland. Minimal systems may need
`libasound2`, `libxkbcommon0`, `libxkbcommon-x11-0`, `libegl1`, and `libgl1`.
The graphical player requires a desktop session D-Bus for single-instance
activation and system media controls. The executable is
not fully static and does not depend on a Nix store closure.

## Lossless sign-in

1. Click **Connect with TIDAL**, or **Settings → General → Connect with TIDAL**
   when upgrading an existing AAC-only device session.
2. Sign in on TIDAL's website in the browser opened for you.
3. The final redirected page may say **Oops**. Copy its complete address from the
   address bar and paste it into the app's **Connect lossless playback** dialog.
   Do not share this address in chat: it contains a one-time authorization code.
4. Click **Finish lossless sign-in**. Your old session stays connected until the
   new authorization succeeds.

The app uses OAuth authorization-code + PKCE, checks the redirect host/path and
state, and expires pending authorizations after ten minutes. Your password never
passes through this app. Refresh tokens retain their authentication type across
restarts. The fixed native-client redirect requires the copy/paste step; there
is no embedded browser or browser-cookie extraction.

### Lossless preferred, with lossy fallback

The default preference requests lossless from TIDAL. If a track is only available
as AAC, it plays automatically at the quality TIDAL provides. The player reports
the **actual** format—for example, `LOW · HE-AAC`—rather than labeling it lossless.
Lossless tracks still play as FLAC.

Supported streams include **unencrypted DASH FLAC, AAC-LC, HE-AAC, HE-AAC v2**,
and direct BTS streams. FLAC decoding uses rodio/Symphonia; DASH AAC uses a
statically bundled FDK AAC decoder with Symphonia demuxing. AAC decoder state is
preserved across fragments, and seeking uses a preceding fragment as preroll. DASH initialization and media
fragments are fetched in the background; only the current fragment and two ahead
are retained. Playback does not wait for the whole track. Seeking selects and
buffers the appropriate fragment and decodes to the requested sample offset on
the audio-control worker before the output callback switches sources. A seek
briefly holds an additional prepared fragment; network waits do not run in the
seek callback. The player reports TIDAL's returned codec, bit depth and sample rate. Live
16-bit / 44.1 kHz FLAC playback and seeking have been verified with a subscription.

Some tracks can still be restricted or unavailable for this client/account.
Fallback does not bypass TIDAL authorization or DRM. The player does not claim
exclusive-mode, bit-perfect output or automatic device sample-rate switching;
the OS mixer may resample audio.

**Compatibility sign-in** remains available for AAC-only device authorization
and direct BTS streams. High and Low AAC preferences also work with PKCE sign-in.
Encrypted audio/DRM and live DASH manifests are not supported. No subscription
bypass, DRM bypass, third-party music proxies or external decoder processes are used.

## Browse and listen

- **Home:** your current **My Daily Discovery** tracks and your personal **My
  Mixes**, fetched from TIDAL—not hardcoded playlists or generic favorites.
- **Playlist folders:** expandable folders mirror the hierarchy in your TIDAL
  account, including nested folders. Children load when expanded. Cursor-based
  pagination fetches every page of each folder. **Refresh** reloads the account
  hierarchy; the app does not move or rename anything in your account.
- **Library:** Tracks, Albums, Artists, and Playlists & folders tabs. Library
  pages retain raw API offsets when an unavailable item is omitted.
- **Favorites:** row/player hearts and album/artist controls save or remove items
  from your TIDAL collection. A heart with a small **dropdown indicator** means
  membership is unknown and opens explicit save/remove choices; it is not an error
  or an assumed unsaved state. Absence is considered confirmed only after
  the complete library for that type has loaded. Membership is currently learned
  from opened Library pages and confirmed writes, not preloaded or cached across
  startup. Pending writes disable repeated
  clicks; failed or uncertain writes do not falsely flip the state. These use the
  existing authenticated compatibility API and remain subject to client permissions.
- **Search:** the compact right-aligned field finds tracks, albums and artists;
  its internal magnifier submits, and the separate Ctrl K badge identifies the
  focus shortcut. Narrow Queue/History overlays close when search focus is requested.
  Click an album, artist or mix to browse it. **Back / Forward** revisit pages
  without restarting radio or track playback; search queries are retained.
- Double-click a track (or focus its row and press Enter) to play. **Play all**
  continues beyond the visible page for albums, playlists and favorite tracks.
  **+** adds a manual Up next entry; **… → Play next** inserts one at its front.
- **Radio:** track and artist radio are available from track-row **...** menus
  and right-click menus, including search, album, playlist, mix and discovery
  track lists. The current-player **...** menu/cover and queue right-click menus
  offer the same actions. Artist search results offer **Start artist radio**.
  These load TIDAL-generated radio tracks and start playback; unavailable radios
  report an error rather than substituting a locally invented playlist.
- Pause, seek, volume, next/previous, reversible source shuffle and repeat
  **off / source / track** are supported. Repeat-track honors natural completion;
  pressing Next still advances. Closing the window exits and flushes playback state.

Collections paginate beyond 100 tracks via **Load more tracks**. Folder contents
and mix tracks are fully paginated. Search returns up to 50 results per type.
Radio loads the endpoint's initial set of up to 100 tracks; it is not an infinite
radio feed. Daily Discovery refreshes when you open or refresh Home.

## Navigation and appearance

**Settings** is a full page with General, Appearance, Playback, Library & Privacy,
Shortcuts, and About sections. Appearance offers comfortable/compact track rows
and 75–150% interface scaling, persisted beside the visualizer preference without
modifying credentials. At narrow logical widths the sidebar becomes an icon rail
and the queue opens in a floating panel, rather than crushing the track list.
Track tables render only visible rows, retaining their original playlist indices.

The **v0.8.0 native visual pass** adds a shared dark/teal visual system,
artwork-led headers, responsive Library grids, virtualized media shelves, clearer
track columns and compact vector actions. Queue/History tiles show artwork, title
and artist; the queue becomes a right-anchored overlay at narrow sizes. Listening
history has a direct **Record listening history** switch, with privacy/retention
information in Settings and a tooltip rather than an inline admonition. Track
rows support click-to-focus and Enter playback; favorite/queue actions never
implicitly play a track. Player-bar quality labels truncate with full hover text,
and a subtle scrim keeps the existing visualizer behind readable controls.
Transport icons share a fixed centerline. Brief PCM snapshot contention retains
only the last fresh, same-epoch visualization, without extending its 180 ms stale
limit; pause/seek/track changes still clear it immediately.

The sidebar uses aligned folder/playlist rows, an inline new-playlist **+**, and
artwork already supplied by TIDAL (generic placeholders when unavailable). No extra
catalog lookup or cover-upload capability is implied. Collection headers expose
**Play** in source order and **Shuffle**. Shuffle chooses the first track from the
loaded entries, preserves duplicates and manual Up next, and inserts later source
pages as they load; it does not claim an unbiased initial choice across unloaded
pages. Filter/sort, selection and metadata-edit tools remain P3 work.

The [implementation roadmap](docs/feature-plan.md) remains staged. Native visual
review comes before library/playlist power tools. Lyrics/Now Playing, mini player,
advanced audio tools and Google Cast remain pending. Genuine TIDAL
Connect remains separately gated; there is no network discovery or laptop receiver
enabled.

## P3a track selection (v0.9.0)

Track tables now support checkboxes, Ctrl/Shift-click selection, Shift+Up/Down/Home/End
ranges, and Ctrl+A for **loaded** tracks. Selection identifies source occurrences,
so duplicate track IDs remain independent. It survives appended pages, but clears
on replacement, refresh, navigation or account changes; cached view restoration is
still future work.

**Copy links** (or Ctrl+C with table focus) copies public TIDAL track URLs in source
order, preserving duplicates. Text fields retain their own clipboard shortcuts.
**Selection actions** adds the batch to Up next or prepends it with Play next while
preserving its order. The entire batch is validated before changing the queue;
invalid IDs, capacity or occurrence exhaustion cannot leave a partial addition.

This is the first P3 slice, not completion of library power tools. Filtering,
sorting, caches, multi-link paste, bulk account writes, metadata editing, playlist
reordering and pins remain pending. Ctrl+C copies links; Ctrl+V into a playlist is
not yet implemented. These additions follow in subsequent reviewed versions.

## Queue and paused restore

Successful startup restoration is silent; actionable errors remain visible.

The queue separates **manual Up next**, a **return path** when going back, and the
remaining **source context**. Each entry has a local occurrence ID, so duplicate
songs can be selected, moved or removed independently. Manual entries have drag
handles plus keyboard-accessible Up, Down and Remove controls. Stale drags are
rejected if the queue, source context or account changes during the gesture. Starting a different album/playlist replaces the source,
not manual Up next; shuffle never rearranges manual entries. Queue actions clearly
separate clearing manual entries, clearing the remaining source, and stopping and
clearing everything. These are local actions, not TIDAL playlist edits.

Source continuation loads in 100-item requests independently of the browsing page.
Playlist revisions and raw item offsets are preserved, including videos/unavailable
items. Changed revisions stop continuation with an explicit error, not a mixed
queue. Favorites have no verified atomic revision contract: changed counts and
repeated page-boundary IDs are rejected, but an equal-count external edit may not
be detectable. Search and radio retain the finite result limits described above.
Queue storage is bounded to 50,000 source/manual entries, 100 backtracking entries
and a 32 MiB state file; limit failures are reported, never silently truncated.

Private, account-scoped `accounts/<user-id>/player-state.json` files under the app's
configuration directory retain the queue, current occurrence/position, repeat,
shuffle, volume, quality preference and last browse route. **Restore is always
paused**. Pressing Play resolves a fresh authorized stream and prepares its saved
position before handing the decoder to the mixer. Failed resume preparation does
not silently play the beginning. No audio or signed stream URLs are persisted.

A coalescing worker serializes and atomically writes state off the UI/audio threads;
progress checkpoints are batched and normal exit flushes the latest state. Corrupt,
future-version or wrong-account files are rejected without touching credentials.
An explicit new queue may replace invalid saved state. Sign-out retains private
playback snapshots for later sign-in; **Settings → Library & Privacy → Stop and
clear playback queue** clears queue entries, not navigation/preferences or TIDAL data.
This playback state is separate from the opt-in listening-history recorder. Browse routes reload their
catalog pages; table selection, scroll offsets and loaded browse pages are not yet
restored as a full view snapshot.

### Save the queue to TIDAL

**Queue actions → Save queue as TIDAL playlist…** captures the current and upcoming
entries in playback order, including manual entries and duplicates. It excludes
past queue history and future repeat cycles. Finish source pagination first;
unloaded tracks are never silently dropped. Review the name/description and
explicitly confirm before any account write.

The new owned playlist is filled in batches of at most 100 tracks (up to 50,000
snapshot entries). Each write uses a revision guard; its returned ETag pins an
ordered read-back before the next batch is sent. **Write ETags are required**:
missing revisions, changed ownership/counts, unavailable items and uncertain writes
stop the export. This compatibility-API contract is tested synthetically, not yet
validated by a live queue-export round trip on your account.

Cancel or closing the dialog stops further batches; an in-flight request may
finish. A failed/cancelled export may leave a partial playlist. Use **Inspect
destination playlist**, or refresh Library if creation could not be confirmed.
There is no automatic retry, rollback or deletion. Later queue edits do not change
the confirmed snapshot; export jobs do not resume automatically after restart.

## Local listening history

Enable **Record local listening history** in **Settings → Library & Privacy** or
in the queue panel's **History** tab. Recording is **off by default**, account-scoped,
and never uploaded or presented as TIDAL's server-side history. A small Home shelf
shows recent recorded plays; clicking one plays that song without discarding manual
Up next entries.

A play qualifies after 30 seconds of rendered playback, or half of a shorter track.
The counter measures consumed audio frames, bounded by monotonic elapsed time—not
track position, seek distance, UI timers or buffered downloads. Pauses and stalls
add no time; muted playback does count. Repeated plays get separate records. Manual
track changes or abrupt exit can lose the last polling/checkpoint interval rather
than inventing playback time.

Private `accounts/<user-id>/history.json` files retain at most 1,000 plays for 90 days
(with a 16 MiB file limit). Writes are batched on the storage worker; normal exit
flushes pending edits. Corrupt, future-version and wrong-account files disable
collection instead of being silently overwritten. Retry loading or explicitly reset
unreadable history from its controls. Disabling recording keeps existing entries;
**Clear local history** deletes them after confirmation. Sign-out retains that
account's history. Clearing history does not change the playback queue, credentials
or TIDAL data; future listening can create new entries if recording stays enabled.

## Edit playlists

- **+ New playlist** in the sidebar creates a playlist in your TIDAL library's
  root folder, with an optional description.
- Use **… → Add to playlist…** or right-click a track. The picker includes your
  own playlists across all folders, supports filtering, and can create a new
  playlist with that track. The same action is available in now-playing and the
  playback queue. Existing duplicates are skipped and reported.
- Inside a playlist you own, use **… → Remove from this playlist…**, then confirm.
  This removes only the selected occurrence, not every copy of that song, and
  does not modify the playback queue. Videos/unavailable entries are excluded
  from the audio view without losing their original playlist positions.
- Edits use TIDAL revision checks. If a playlist changed elsewhere, refresh it
  before editing again; the app never blindly retries an index-based deletion.
  Followed/editorial playlists remain read-only. Collaborative editing,
  reordering, renaming and deleting entire playlists are not included yet.

## Open TIDAL links

Open `tidal://track/123`, `tidal://album/123`, `tidal://artist/123`,
`tidal://playlist/<uuid>` or `tidal://mix/<id>` using your desktop's normal URL
handler. `tidal://browse/…` links are also supported. An existing player receives
the link and raises its window; otherwise the app starts. Links received before
sign-in are retained until you connect.

Track links open and play that song. Album, artist, playlist and mix links open
the corresponding page without interrupting playback. Artist pages show popular
tracks and albums. Official `https://tidal.com/browse/…`, `www.tidal.com` and
`listen.tidal.com` links can also be pasted into Search or passed explicitly:

```nu
tidal-forces --open 'tidal://track/471571362'
```

The app registers only the `tidal` scheme, not a catch-all HTTPS handler. Unknown
link types, foreign hosts and malformed IDs are rejected. Authentication links
must still go through the separate masked sign-in dialog.

## Player bar visualizer

**Settings → Appearance → Player bar visualizer** offers **Off**, **Spectrum**,
and **Waveform**. Clicking empty space in the bottom player bar cycles those modes;
playback buttons, seeking and volume controls keep their normal behavior.

- Spectrum shows 48 bass-to-treble bands with smooth decay and falling peaks.
- Waveform draws a glowing trace behind the controls. Both use colors extracted
  from the current album artwork, with a Tidal Forces palette when art is unavailable.
- The picture follows actual decoded FLAC/AAC audio, before output volume. It
  still moves when muted; pausing/stopping clears it. No microphone or system
  audio capture is used.
- The mode defaults to Off and is saved separately from credentials in
  `~/.config/tidal-forces/appearance.json`. Animation is capped at a roughly
  30 Hz requested refresh rate and disabled while paused, off or minimized.

The independent implementation is inspired by Spotifast's player-bar visualizer.
A fixed-size, atomic PCM snapshot keeps the tap non-blocking, with no tap allocations,
locks or FFT work in the output callback. The UI computes a windowed 2048-point FFT;
only the visualization is downmixed and capped at 48 kHz. Playback samples pass
through unchanged. Seeks and track changes invalidate the previous snapshot.

## Standard desktop media controls

The app exposes **`org.mpris.MediaPlayer2.tidalforces`** on the session bus,
including playback state, metadata, artwork, position, volume and transport
commands. System play/pause works even when this window is unfocused. No global
key grabs or replacement desktop bindings are installed.

- Media play/pause, next and previous: through your desktop's normal MPRIS routing
- `Space`: play / pause inside the app, except while typing
- `Ctrl+K`: search
- `Alt+Left` / `Alt+Right`: back / forward
- `Ctrl+Left` / `Ctrl+Right`: previous / next

For testing (Nushell):

```nu
playerctl --player=tidalforces play-pause
playerctl --player=tidalforces status
```

`playerctl` is an optional diagnostic/controller, **not an app dependency**.
When several players are registered (for example the old TIDAL client or Firefox),
your desktop/controller chooses which one receives a generic media-key command.
Closing a competing player removes it from that selection. Tidal Forces does
not take over other players or change your shortcut routing.

## Account access and privacy

This is an **unofficial** client using TIDAL's authenticated native API and public
compatibility-client identifiers also used by
[python-tidal](https://github.com/tamland/python-tidal). A paid subscription and
TIDAL's authorization for the client/track/region are required. TIDAL can change
or revoke access; a successful login does not guarantee every track or format.

No telemetry. The default/legacy sign-in mode uses
`~/.config/tidal-forces/session.json` (or its XDG equivalent), with a `0700`
directory and atomically-written `0600` file. This mode is explicitly disclosed
and is **not encrypted at rest**; processes running as your user can read it.
Only TIDAL's APIs, image servers and provided audio CDN URLs are contacted over
the network. Tokens and music are never included in source control or releases.

On Linux, **Settings → Library & Privacy → Move sign-in to OS keyring…** offers
explicit migration to a configured, unlocked Secret Service provider. The native
D-Bus client uses the standard local `plain` session algorithm; protection at rest
depends on the desktop provider and its configuration, not on this transport.
The app does not call `Unlock` or `Prompt`, install a provider, or change desktop
keyring settings. A locked/missing/default-unconfigured vault reports an error.
Secret MIME labels are advisory: GNOME Keyring returns `text/plain` even for JSON
writes. Session identity, plain-session parameters and size limits remain enforced;
credential verification uses exact bytes/digests and the credential schema, not MIME.

Migration journals a fresh app-scoped item, writes and reads back the exact secret,
then atomically commits private `credentials.json` metadata before removing the
legacy token file. Rotating tokens uses the same copy-on-write process; interrupted
writes retain cleanup references. Existing keyring mode never silently falls back
to plaintext. Failed refresh persistence keeps new tokens in memory with a visible
warning and retry action; normal window close asks for confirmation while a known
credential operation is pending or sign-in remains unsaved. Forced termination
cannot preserve unsaved in-memory credentials. CLI checks report storage failures.

**Sign out and remove saved sign-in** first records signed-out intent, then removes
stored copies. If the keyring is locked, automatic sign-in remains disabled while
cleanup is pending. Failed metadata writes are reported and may leave automatic
sign-in possible; retry removal. Storage preference stays in keyring mode after
sign-out. Cleanup only targets journaled app items; a changed legacy token file is
left intact for explicit recovery/removal. Unreadable metadata fails closed.

Use one credential-changing client per profile. After migration, do not run older
player versions against that profile: they cannot read keyring sign-in and do not
understand its metadata. Migration requires your explicit UI confirmation; automated
tests use synthetic credentials and do not migrate your profile. Successful GNOME
Keyring migration was user-confirmed after the MIME compatibility fix; additional
provider/recovery scenarios still need opt-in validation.

DASH fragments are held in a bounded memory buffer. Direct BTS streams use
anonymous temporary-file caching; normal shutdown removes those files, while
abnormal termination may leave OS temporary files behind. Approved compatible
device credentials can be supplied with `TIDAL_CLIENT_ID` and
`TIDAL_CLIENT_SECRET`; never commit private credentials.

## Build and verify

Install stable Rust and native dependencies. On Ubuntu (Bash):

```bash
sudo apt-get install build-essential pkg-config libasound2-dev libx11-dev \
  libxi-dev libxcursor-dev libxrandr-dev libxkbcommon-dev libxkbcommon-x11-0 \
  libwayland-dev libgl1-mesa-dev
cargo build --locked --release
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --all -- --check
```

Developer checks (Nushell):

```nu
./target/release/tidal-forces --smoke-ui
./target/release/tidal-forces --audio-test
./target/release/tidal-forces --check-account
./target/release/tidal-forces --check-account --refresh
./target/release/tidal-forces --verify-playback 257836968 --seek
./target/release/tidal-forces --verify-playback 471571362 --seek
```

Account checks require sign-in. Playback verification checks nine seconds of
audio at the best returned quality across segment boundaries; `--seek` also exercises pause/resume,
a seek to 45 seconds, and subsequent playback (use a track longer than 50 seconds).
The audio test plays a quiet 150 ms tone. Automated fixtures are synthesized
locally, not TIDAL music. Tests verify FLAC segment continuity sample-for-sample,
seeking across buffer eviction, bounded caching, manifest rejection, OAuth state,
folder parsing, queue behavior and private credential persistence. AAC-LC,
HE-AAC and HE-AAC v2 fixtures verify full-rate stereo output, uninterrupted codec
state across fragments, and seek preroll against a continuous reference decode.
Visualizer tests cover sample-for-sample passthrough, pre-volume capture, silence,
frequency detection, peak decay, high sample rates, concurrent snapshot consistency,
pause/seek/track invalidation, artwork colors and preference persistence.
Playlist API fixtures cover pagination, duplicate skipping, ownership checks,
revision conflicts, and exact occurrence removal. URI parsing rejects untrusted
hosts and malformed identifiers. Test single-instance activation on an isolated
bus so it cannot contact your running player:

```nu
dbus-run-session -- cargo test --locked single_instance_forwards_links_and_activation -- --ignored
```

The native Secret Service wire contract is tested with synthetic secrets on a
private D-Bus session, never your desktop vault:

```nu
dbus-run-session -- env TIDAL_TEST_SECRET_SERVICE=1 cargo test --locked native_secret_service_contract_on_isolated_bus -- --ignored
```

Debug builds also include a non-interactive native screenshot harness. It uses
fictional tracks/artwork, an inert player, no API worker, no network/file image
loaders, no MPRIS registration, and no application-state storage. It captures a PNG
and exits; it cannot operate your account. Run on an isolated display for QA:

```nu
./target/debug/tidal-forces --visual-fixture /tmp/tidal-native.png collection 1240 820 1
```

Fixture pages: `collection`, `home`, `albums`, `artists`, `history`, `settings`.
The final arguments are window width, height and zoom (0.75–1.5). This developer
option and synthetic catalog are not included in release builds.

`live_playlist_round_trip` is separately ignored by default: explicitly running
it creates a temporary playlist on the signed-in account, verifies create/add/
duplicate/remove behavior, then deletes that test playlist. Never run all ignored
tests blindly against a real account.

## Release automation

Every push to `master` runs formatting, Clippy, tests, an optimized build and a
native-window smoke test under Xvfb in GitHub Actions. Successful pushes publish
a commit-specific Release containing the **raw single executable** and a SHA-256
checksum. Different master pushes do not cancel each other. Reruns replace the
same commit's assets; pull requests run checks without publishing. `Cargo.lock`
is committed and all CI builds use `--locked`.

## Implementation and acknowledgements

- `ui.rs`: native egui rendering; folders, mixes, discovery and radio controls.
- `api.rs` / `auth.rs`: catalog requests, pagination, authenticated manifests,
  device sign-in, PKCE and session refresh.
- `backend.rs`: bounded command channel and generation-guarded background work.
- `audio.rs` / `dash.rs` / `aac.rs`: native audio, bounded FLAC/AAC buffering,
  AAC decoder continuity, and prepared seeks.
- `visualizer.rs` / `visualizer_ui.rs`: bounded PCM tapping, UI-thread spectrum
  analysis, album-derived colors, and native player-bar rendering.
- `desktop.rs` / `links.rs`: standard MPRIS integration, strict URI parsing and
  session-bus single-instance activation.
- `store.rs` / `install.rs`: private atomic sessions and desktop installation.

Inspired by Spotifast's native-first architecture; API compatibility researched
against python-tidal. Built with egui, rodio/Symphonia, stream-download,
reqwest/rustls, Tokio, roxmltree, souvlaki/zbus, FDK AAC and Inter fonts. Inter is by Rasmus
Andersson, under SIL OFL (`assets/fonts/OFL.txt`). No Spotifast source or artwork
is bundled.

Not yet included: playlist reordering/renaming/deletion, folder editing,
lyrics, offline downloads, TIDAL
Connect, infinite radio, gapless track transitions, tray icon or built-in updater.

The Rust application is MIT licensed; the bundled AAC codec and fonts retain
their own licenses. See [THIRD_PARTY.md](THIRD_PARTY.md), run `--licenses` for the
full notices, or `--export-fdk-source PATH` to extract the complete codec source
from the executable. No separate codec installation is needed.

Not affiliated with or endorsed by TIDAL. TIDAL is a trademark of
its respective owner; this app uses its own name and original icon.
