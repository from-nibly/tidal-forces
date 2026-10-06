# Tidal Forces

**Your TIDAL library. A native Rust desktop player. No browser engine.**

Tidal Forces uses TIDAL-inspired dark surfaces, large typography, album artwork,
a collection sidebar and a persistent player. Its architecture takes cues from
[Spotifast](https://spotifast.rocks) ([source](https://github.com/crmne/spotifast)):
Rust + egui, background network work, native decoding, event-driven repainting,
and a single executable. This is an independent implementation, not a Spotify
client with its branding replaced.

## Download and install

Download `tidal-forces-linux-x86_64` and `SHA256SUMS` from
[Releases](https://github.com/from-nibly/tidal-forces/releases/latest).
On Linux x86_64 (Ubuntu 22.04 / glibc 2.35 or newer), using Nushell:

```nu
sha256sum --check SHA256SUMS
chmod +x tidal-forces-linux-x86_64
./tidal-forces-linux-x86_64 --install
```

Open **Tidal Forces** from your desktop's application launcher. The installer
copies the running executable to `~/.local/bin/tidal-forces` and creates an XDG
desktop entry and an embedded SVG icon under your user data directory. No sudo
is needed. Run a newer download with `--install` to update it (restart the app).

This is **one executable**, not an AppImage, Electron bundle, script calling
Python/mpv/ffmpeg, or a Nix store closure. As with most native Linux programs, it
uses the host's glibc, ALSA, graphics driver/OpenGL, and X11 or Wayland libraries.
It is not a fully static executable. Ubuntu/Pop!_OS desktop installations provide
these runtime libraries. Minimal installations may need `libasound2`,
`libxkbcommon0`, `libxkbcommon-x11-0`, `libegl1`, `libgl1`, and the relevant
X11/Wayland libraries.

## Connect and listen

1. Click **Connect with TIDAL**. Authorize the device on TIDAL's website in your
   browser. Your password is never handled by this app.
2. Open your collection or playlists, or search for tracks and albums.
3. Double-click a track, click an album cover then **Play all**, or use **+** to
   add a track to the queue.
4. Use the player controls to pause, seek, change volume, skip, shuffle upcoming
   tracks or repeat the queue. Change streaming quality in **Settings**.

- `Space`: play / pause (except while typing)
- `Ctrl+K`: search
- `Ctrl+Left` / `Ctrl+Right`: previous / next
- **Load more tracks** paginates collections beyond the first 100 tracks.
- Search returns up to 50 tracks/albums; the playlist sidebar loads up to 100
  playlists. These bounds are explicit, not a claim of complete library sync.

### Account and playback limitations — please read

This is an **unofficial** client. It uses the account-authenticated native TIDAL
API and public compatibility-client identifiers also used by
[python-tidal](https://github.com/tamland/python-tidal), not TIDAL's public
preview-only developer integration. A paid subscription and TIDAL's permission
for that client/track/region are required. TIDAL can change or revoke this access.
A successful sign-in alone does **not** guarantee full playback.

Supported: unencrypted TIDAL BTS manifests containing direct HTTPS streams,
including FLAC and AAC decoded natively with Symphonia/rodio. Streaming starts
with a small prebuffer; the rest downloads into an ephemeral temporary file.
Seeking is handled by the native decoder and streaming cache.

**No DRM bypass, encrypted-stream decryption, third-party streaming proxies,
offline downloads, or subscription bypass.** DASH manifests are currently
unsupported. If a track returns DASH, try **High** quality. Unsupported playback
is reported as an error, not silently substituted with a preview. The player
shows the actual quality returned by TIDAL (which may be **HIGH/AAC even when
LOSSLESS is requested** with this compatibility client); it does not promise hi-res,
exclusive-mode, bit-perfect playback, or gapless transitions.

This first version does not include playlist editing, lyrics, recommendations,
TIDAL Connect, system media-key/MPRIS integration, a tray icon, or a built-in
updater. It plays on the default OS audio device. Closing the window exits.

## Privacy

No telemetry. Session tokens stay in
`~/.config/tidal-forces/session.json` (or the XDG config directory). The directory
is mode `0700` and the atomically-written token file is mode `0600`. Tokens are
**not encrypted at rest**; local processes running as your user can read them.
Use **Settings → Sign out** to stop playback and remove the saved session.
The audio cache uses temporary files that are removed when their streams close;
abnormal process termination may leave OS temporary files behind.

Only TIDAL's APIs, artwork servers and stream CDN URLs are contacted. No audio
or tokens are committed to GitHub or included in release artifacts. Developers
with approved compatible credentials can override `TIDAL_CLIENT_ID` and
`TIDAL_CLIENT_SECRET` at runtime. Do not commit private credentials.

## Build and verify

Install stable Rust and the native build dependencies. On Ubuntu (Bash):

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
./target/release/tidal-forces --verify-playback 12345678
```

The last two require a signed-in account; replace the example ID with a playable
TIDAL track. `--audio-test` plays a quiet 150 ms test tone. `--verify-playback`
requires at least three seconds of actual decoder/audio progress before success.
CI cannot certify account-dependent playback without a real subscriber login.

## Release automation

Every push to `master` runs formatting, Clippy, tests, an optimized release build
and a native-window smoke test under Xvfb in GitHub Actions. Successful pushes
publish a commit-specific GitHub Release with the **raw single executable** and
SHA-256 checksum. Reruns replace that commit's assets; builds for different
commits do not cancel each other. Pull requests run the same checks but never
publish. `Cargo.lock` is committed and all builds use `--locked`.

## Implementation

- `src/ui.rs`: native egui interface; no blocking API or decoder work on the UI
  thread. Repaints are driven by input and backend events, not a render loop.
- `src/backend.rs`: bounded command channel, OAuth device polling and catalog
  worker; generation IDs discard stale search and playback responses.
- `src/api.rs`: authenticated requests, token refresh and manifest validation.
- `src/audio.rs`: native audio-owner thread and background disk-backed HTTP
  stream buffering. Network/decoder errors surface in the interface.
- `src/store.rs`: atomic private credential persistence.
- `src/install.rs`: self-installing desktop integration with embedded assets.

## Acknowledgements

Inspired by Spotifast's native-first architecture. API compatibility researched
against python-tidal. Built with egui/eframe, rodio/Symphonia, stream-download,
reqwest/rustls and Tokio. The embedded Inter fonts are by Rasmus Andersson,
licensed under the SIL Open Font License (see `assets/fonts/OFL.txt`). No
Spotifast source or artwork is bundled.

MIT licensed. Not affiliated with or endorsed by TIDAL. TIDAL is a trademark of
its respective owner. The app uses its own original icon and name.
