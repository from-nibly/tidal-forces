# Output recovery and native routing validation

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
covered. This preserves the v0.30 controller contract without using a real device.

## Private native-routing contract (v0.31)

Run `python3 tests/output-pulse.py`, with the PulseAudio daemon available as a test
tool. The application itself does not run that daemon or an external audio helper.
The script starts its own daemon with **only** a private Unix protocol socket and
two null sinks. It disables normal daemon startup scripts, realtime priority,
shared memory and inherited Pulse settings, supplies private HOME/XDG paths, and
runs only `pulse_output::tests::native_routing_on_private_null_server`.

The Rust test refuses an ordinary environment: it validates the private directory,
socket, server variable and exact null-only configuration before connecting.
It checks discovery, explicit target identity, rejected movement, cork/resume,
client-buffer reset, target removal, no fallback to the remaining sink, explicit
reopening, and callback lifetime cleanup. It does not inspect or alter real sinks,
ports, cards, desktop routing or other applications. Evidence remains in the
printed private temporary directory. The script stops only its own daemon.

**Never run all ignored tests.** Other ignored contracts can mutate an account.
Neither null-output contract establishes physical headphone behavior.

## Runtime contract and capability limits

- Native Linux output is a libpulse client, compatible with PulseAudio and
  PipeWire's Pulse server. The stream names a sink and requests `DONT_MOVE`;
  errors, removal, movement, identity changes and exposed active-port changes
  halt consumption and require explicit recovery. No automatic ALSA/device search.
- Desktop first use and legacy preferences require explicit output selection before
  playback authorization. Selection alone does not start playback.
- Saved named choices include a tagged device identity and active-port digest.
  Stable serial/bus-path/Bluetooth identity is used when supplied by the server.
  Otherwise a process-specific identity forces selection again after restart.
  Missing/changed/corrupt preferences never silently select a different device.
- The system-default choice resolves and locks a sink when opening. It does not
  continuously follow default changes. Explicit retry may resolve a new default;
  choose a named output to retain stricter device/port matching.
- Native client audio is 48 kHz, stereo, float32, verified against the negotiated
  stream specification. This is **not** the DAC's format. The client requests a
  bounded buffer; server/device latency may differ. Seek/load resets flush queued
  client audio, not sound already delivered to a physical device.
- Compatibility output opens only the current CPAL default and never searches
  alternate devices. It cannot monitor a healthy virtual endpoint's physical route.
- Port notifications arrive after host routing changes. Sink locking and prompt
  pause cannot guarantee zero buffered sound on speakers after a physical unplug.
  A server that exposes no port cannot supply a physical-port guarantee.
- The private PulseAudio test validates protocol/controller behavior, not a
  particular PipeWire/session-manager/hardware combination. Real headphone,
  Bluetooth, hotplug and audible tests require **separate explicit opt-in**;
  version approval does not authorize them.

Normal tests additionally cover frame-aligned ramps, bypass, DSP/gain gates,
stale fault/availability events and occurrence preservation. Native synthetic UI
fixtures have no backend, device or credential access. The running review preview
must remain unchanged until replacement is approved.

The Rust binding's narrow callback cancellation patch is documented in
`vendor/README.md`; cancellation tests use only the private server above.
