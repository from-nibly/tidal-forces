# Daily-use windows and controls (v0.32 development)

All windows, MPRIS/media keys, tray events and CLI commands act on one player,
queue, output selection and listening-history owner. Opening or closing the mini
player does not start another audio engine, replace the queue or start playback.

## Mini player

Open it from the player-bar overflow, Settings → General, the tray, or
`tidal-forces --mini-player`. It has artwork, title/artist, source-quality reporting,
transport, subsecond seek and volume. Its menu opens Now Playing and changes the
same shuffle/repeat state as the main player. Closing the mini window leaves the
main player and playback unchanged. Closing the main window normally exits both.

Always-on-top is a request, not a guarantee. The option is disabled in Wayland
sessions because this native backend does not provide that window-level control.
A compositor rule can target the title `Tidal Forces · Mini`; its application ID
remains `rocks.tidalforces.Player`. Classic skins remain later work.

## Opt-in Linux tray

Settings → General enables the StatusNotifierItem/DBusMenu integration. It uses
existing native D-Bus support and embedded artwork, without a new runtime library.
The menu offers Open player, Now Playing, Mini player, Play/pause, Previous, Next
and Quit. Primary activation requests the main window; secondary activation
controls play/pause. Vertical wheel input changes volume in bounded steps.

- Off by default, including legacy preferences. Startup remains visible and paused.
- On X11, close-to-tray requires a registered item and a live registered host.
  Without that confirmation, Close still exits. A disappearing host restores a
  window hidden by the app; checks run every second with a one-second deadline.
- **Native Wayland cannot reliably hide/restore windows through this backend.**
  Tray controls remain available, but Close still exits. Focus requests are subject
  to compositor policy; no compositor-specific commands are issued.
- Dirty or in-flight credentials never hide behind close-to-tray. The existing
  confirmation remains authoritative. Quit explicitly requests exit, restores the
  main window when necessary and still honors that confirmation.
- Disabling the tray restores an app-hidden window. A failed bus connection can
  be retried by toggling the setting. Watcher/host disappearance and replacement
  are checked independently of audio work; they never restart playback.

The menu intentionally exports static application labels rather than account or
track information. The tray's command inbox is bounded to 32 entries. Protocol
references: KDE's `org.kde.StatusNotifierItem`/`StatusNotifierWatcher` interfaces and
Canonical's `com.canonical.dbusmenu` interface (also documented in ksni's `spec/`).
No third-party tray implementation is copied or launched.

## CLI commands

`--play`, `--pause`, `--toggle`, `--next`, `--previous`, `--stop`,
`--seek SECONDS`, `--seek-by SECONDS`, `--volume PERCENT`, `--now-playing`,
`--mini-player`, `--raise` and `--quit` require an already-running compatible
instance. They never launch a second player. Acknowledgement means **queued**,
not that playback authorization, output selection or a seek has succeeded.
Seek values can be fractional; volume is 0–100. Existing TIDAL-link activation
continues to use the same instance and restores an app-hidden main window.

## Isolated validation

Run normal tests and `python3 tests/tray.py` inside the development environment.
The wrapper builds the test binary and creates a fresh private bus and HOME/XDG
roots. The ignored native contract refuses the desktop bus: it requires an owned,
private temporary directory and matching, non-symlink Unix socket. It checks item
registration, native property/layout signatures, queued controls, host loss,
watcher replacement and connection cleanup. It also runs the window-close policy
unit tests with Wayland environment variables removed. Never run all ignored tests.

Synthetic native fixtures cover the mini layout at minimum/normal/high zoom.
A separate private Xvfb/D-Bus probe checks actual native child-window creation and
close, continued main-window lifetime, CLI routing and graceful Quit. These checks
are not proof of a real desktop host's presentation or compositor focus behavior.
No real account mutation, audible playback or physical-device testing is implied.
