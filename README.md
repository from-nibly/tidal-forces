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
and restart the app to update.

This is one executable, not Electron, an AppImage, a webview or a wrapper around
Python/mpv/ffmpeg. It uses standard Linux runtime libraries: glibc, ALSA, your
graphics driver/OpenGL, and X11 or Wayland. Minimal systems may need
`libasound2`, `libxkbcommon0`, `libxkbcommon-x11-0`, `libegl1`, and `libgl1`.
A desktop session D-Bus is needed for system media controls. The executable is
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

### Actual lossless streaming

The lossless connection supports **unencrypted DASH FLAC** and direct BTS FLAC
streams. Native decoding uses rodio/Symphonia. DASH initialization and media
fragments are fetched in the background; only the current fragment and two ahead
are retained. Playback does not wait for the whole track. Seeking selects and
buffers the appropriate fragment and decodes to the requested sample offset on
the audio-control worker before the output callback switches sources. A seek
briefly holds an additional prepared fragment; network waits do not run in the
seek callback. The player reports TIDAL's returned codec, bit depth and sample rate. Live
16-bit / 44.1 kHz FLAC playback and seeking have been verified with a subscription.

**There is no silent lossy fallback.** If TIDAL offers only AAC/LOW for a track,
lossless mode reports that and does not play it as “lossless.” Some tracks can
be restricted or unavailable at the requested quality for this client/account.
The player does not claim exclusive-mode, bit-perfect output or automatic device
sample-rate switching; the OS mixer may resample audio.

**Compatibility sign-in** remains available for AAC-only device authorization
and direct BTS streams. AAC quality choices apply to that connection, not the
PKCE lossless connection. DASH AAC, encrypted audio/DRM, and live DASH manifests
are not supported. No subscription bypass, DRM bypass, third-party music proxies
or external decoder processes are used.

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
```

Account checks require sign-in. Playback verification checks nine seconds of
lossless audio across segment boundaries; `--seek` also exercises pause/resume,
a seek to 45 seconds, and subsequent playback (use a track longer than 50 seconds).
The audio test plays a quiet 150 ms tone. Automated fixtures are synthesized
locally, not TIDAL music. Tests verify FLAC segment continuity sample-for-sample,
seeking across buffer eviction, bounded caching, manifest rejection, OAuth state,
folder parsing, queue behavior and private credential persistence.

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
- `audio.rs` / `dash.rs`: native audio, segmented lossless buffering and seeking.
- `desktop.rs`: standard MPRIS integration via souvlaki's Rust/zbus backend.
- `store.rs` / `install.rs`: private atomic sessions and desktop installation.

Inspired by Spotifast's native-first architecture; API compatibility researched
against python-tidal. Built with egui, rodio/Symphonia, stream-download,
reqwest/rustls, Tokio, roxmltree, souvlaki/zbus and Inter fonts. Inter is by Rasmus
Andersson, under SIL OFL (`assets/fonts/OFL.txt`). No Spotifast source or artwork
is bundled.

Not yet included: playlist/folder editing, lyrics, offline downloads, TIDAL
Connect, infinite radio, gapless track transitions, tray icon or built-in updater.

MIT licensed. Not affiliated with or endorsed by TIDAL. TIDAL is a trademark of
its respective owner; this app uses its own name and original icon.
