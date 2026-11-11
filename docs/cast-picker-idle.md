# Cast picker idle CPU regression (v0.34.1)

The v0.34 picker displayed an egui Spinner for the lifetime of its live discovery
subscription, including after devices had resolved. Spinner painting requests an
immediate repaint. On native Wayland, where blocking vsync is disabled, this can
continually redraw the entire player rather than leave an idle picker asleep.

The fix uses static discovery status text and the existing discovery-driven wakeup.
Unread discovery update bursts now coalesce into one wake while preserving the
latest inventory and terminal state. Browsing, cancellation and device control
semantics are unchanged; closing the picker still ends discovery.

## Evidence

The deterministic UI regression failed before the fix: all ten measured settled
frames requested another redraw within250ms. Empty and populated picker variants
pass after the fix. A separate mailbox regression verifies one wake per unread
burst, subsequent wake after consumption, and retained terminal delivery.

An isolated native debug probe used synthetic catalog/device data, an inert backend
and decoder, private Xvfb/D-Bus/profile, software rendering, one llvmpipe thread and
vsync off. It performed no account, LAN, receiver or audio-output operations.
Each eight-second run sampled process CPU for three settled seconds:

- Before: closed0.00CPU-seconds/5frames; empty2.87CPU-seconds/228frames;
  populated2.88CPU-seconds/202frames.
- After: closed/empty/populated each0.00CPU-seconds within timer resolution;
 5/6/6frames respectively across the full eight-second runs.

This confirms the native repaint regression, not a precise CPU prediction for every
live desktop/device/GPU. It does not benchmark the real mDNS daemon's network load.
The operator reviewed the fixed native preview and approved publication/install.

## Reproduce privately

With native build dependencies, Xvfb and dbus-daemon available:

```zsh
cargo build --locked
python3 tests/cast-picker-idle.py fixed target/debug/tidal-forces
```

The probe entry point is debug-only. Its fake discovery handle starts no service
browser. The runner isolates profile, graphics and buses, suppresses live audio
endpoints, bounds each run and stops only its own fixture processes. It reports
CPU/frame observations rather than asserting host-dependent CPU thresholds; normal
unit tests enforce the deterministic repaint and wakeup contracts.
