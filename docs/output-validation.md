# Output recovery and OS-managed routing validation

## Private compatibility-output contract

Run `python3 tests/output-recovery.py` in the Rust/native development environment.
It builds (but does not run) the test executable, then invokes only
`audio::next_tests::output_recovery_on_isolated_null_alsa` on a separate D-Bus
session with private HOME/XDG paths and ALSA configuration. An optional argument
supplies an already-built test executable.

The test explicitly selects compatibility output. It starts with an unavailable
plugin, changes only its private configuration to ALSA's null output, and checks
that a new playback request cannot reopen a failed output. Only the current fault
acknowledgement permits recovery. Synthetic stream faults and stale retries are
covered without using a real device.

## Private PulseAudio contract

Run `python3 tests/output-pulse.py`, with the PulseAudio daemon available as a test
tool. The application itself does not run that daemon or an external audio helper.
The script starts its own daemon with **only** a private Unix protocol socket and
two null sinks, disabling normal startup scripts and inherited Pulse settings.
It supplies private HOME/XDG paths and runs only
`pulse_output::tests::native_routing_on_private_null_server`.

The Rust test validates the private directory, socket, server variable and exact
null-only configuration before connecting. It checks discovery, ignored legacy
fingerprints, repeated source changes, cork/resume, client-buffer reset, live
application-only selection, unchanged server default, rejected selection without
stream failure, externally requested movement, removal of the old sink without a
false failure, and callback lifetime cleanup. A server may reject opening an
explicitly requested sink that no longer exists; that is not an app identity guard.

## Private PipeWire contract

Run `python3 tests/output-pipewire.py` with PipeWire, pipewire-pulse, WirePlumber
0.5, pactl and dbus-daemon installed. Locally checked with PipeWire 1.6.6 and
WirePlumber 0.5.14; the Ubuntu 22.04 CI job does not run this newer-policy fixture.
It creates private HOME/XDG/runtime/config paths and a private D-Bus session.
Its explicit graph contains only two virtual null sinks. WirePlumber uses the
policy profile with audio, Bluetooth and video hardware monitors disabled, and
cannot access the system bus. All clients use the private audio-server sockets.

The Rust contract checks the owned private directory, socket and exact fixture
configurations before opening audio. It exercises the full 44.1 kHz PCM/player path,
sustained playback with a deliberate 250 ms writer stall, repeated song changes and
live stream moves while checking that the server default remains unchanged. A
private null-sink monitor counts actual nonzero synthetic samples: advancing player
positions alone cannot establish successful delivery. No real audio is recorded;
only aggregate counts are retained.
The wrapper validates the null-only sink inventory before running the contract and
terminates only its own daemon processes. Evidence remains in the printed private
temporary directory.

**Never run all ignored tests.** Other ignored contracts can mutate an account.
These contracts neither inspect real outputs nor establish physical-device behavior.

## Runtime contract and limits (v0.32)

- Native Linux output uses libpulse, including PipeWire's PulseAudio compatibility
  server. The OS audio server controls routing and fallback. No `DONT_MOVE`, sink
  pinning, device/port fingerprint matching, route subscription or moved-event stop
  remains. Headphone removal may route audio to speakers under OS policy.
- System default passes no named sink when opening; existing-stream behavior after
  a system-default change is determined by the server's policy, not an app watcher.
- The native picker moves only this application's stream, never the system default
  or another application's output. Rejected move requests display a notice without
  terminating the existing stream. The saved choice is a preference, not proof of
  the stream's current physical route; the OS may move it elsewhere.
- New/legacy preferences default to OS routing. Old fingerprint fields are ignored,
  and the old require-selection preference is normalized to system default when
  loaded. Selecting output alone never starts stopped playback.
- Actual stream/context failures and failed transport operations still stop playback
  and require explicit retry. Ordinary routing notifications do not. No automatic
  switch to the separate compatibility backend is attempted.
- Native client audio is 48 kHz, stereo, float32, checked against the negotiated
  stream specification. This is **not** the DAC's format. A 100 ms per-stream buffer
  target (400 ms maximum requested capacity) leaves scheduling headroom. The client
  does not request ADJUST_LATENCY/device-latency changes, and automatic prebuffering
  lets the server recover after underruns. Server/device latency may differ. Seek/load flushes cannot retract sound
  already delivered to a device.
- Compatibility output uses the current CPAL default. Switching into or out of
  compatibility mode pauses playback; Play reloads at the retained position.
- Real headphone, Bluetooth, hotplug and audible tests require **separate explicit
  opt-in**. Version approval does not authorize them. The running review preview
  remains unchanged until replacement is approved.

Normal tests also cover frame-aligned ramps, bypass, DSP/gain gates, stale fault
and availability events, and occurrence preservation. The binding's callback
cancellation patch remains documented in `vendor/README.md`.
