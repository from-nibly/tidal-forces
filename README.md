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

1. Click **Connect with TIDAL**, or **Settings → Enable lossless sign-in** when
   upgrading an existing AAC-only device session.
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
- **My collection:** favorite tracks are still available separately.
- **Search:** tracks, albums and artists. Click an album or mix to browse it.
- Double-click a track or choose **Play all**. Use **+** to append to the queue.
- **Radio:** track and artist radio are available from track-row **...** menus
  and right-click menus, including search, album, playlist, mix and discovery
  track lists. The current-player **...** menu/cover and queue right-click menus
  offer the same actions. Artist search results offer **Start artist radio**.
  These load TIDAL-generated radio tracks and start playback; unavailable radios
  report an error rather than substituting a locally invented playlist.
- Pause, seek, volume, next/previous, shuffle upcoming tracks and repeat queue
  are supported. Closing the window exits.

Collections paginate beyond 100 tracks via **Load more tracks**. Folder contents
and mix tracks are fully paginated. Search returns up to 50 results per type.
Radio loads the endpoint's initial set of up to 100 tracks; it is not an infinite
radio feed. Daily Discovery refreshes when you open or refresh Home.

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

No telemetry. Session tokens stay in `~/.config/tidal-forces/session.json` (or
its XDG equivalent), with a `0700` directory and atomically-written `0600` file.
They are **not encrypted at rest**; processes running as your user can read them.
**Settings → Sign out** stops playback and removes the session file. Only TIDAL's
APIs, image servers and provided audio CDN URLs are contacted. Tokens and music
are never included in source control or release artifacts.

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
