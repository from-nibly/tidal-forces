# Output recovery validation

Run the non-audible Linux contract with `python3 tests/output-recovery.py` in the
normal Rust/native development environment. It builds (but does not run) the test
binary, then invokes only `audio::next_tests::output_recovery_on_isolated_null_alsa`
on a separate D-Bus session with private HOME/XDG paths and a private ALSA config.
An optional argument supplies an already-built application test executable.

The contract starts with an intentionally unavailable plugin, replaces only its
private configuration with ALSA's null output, and checks that a new playback
request alone cannot reopen output. An explicit, current fault acknowledgement
permits recovery in the same player. It also injects a synthetic stream fault while
paused and checks that stale acknowledgements cannot reopen the newer failure.
No real output is selected and no audio is made audible. Evidence remains in the
printed private temporary directory. Never run this test directly against the
user's ordinary ALSA environment or run all ignored tests.

Normal tests cover complete-frame gating, unchanged healthy samples, stale faults,
queue/occurrence preservation, saved position and explicit retry. These do not
establish physical headphone hotplug or real-device recovery behavior.

## Scope and remaining gate

The app opens the current OS default only; it no longer asks Rodio to search other
devices on open failure. Output errors pause playback and require the user to choose
the desired **system output** and press Play. An in-app output picker and stable
physical-device preference are not implemented by this slice.

PulseAudio/PipeWire/OS routing can redirect a healthy virtual default endpoint
without emitting a CPAL stream error. This slice cannot detect or prevent that
system-controlled headphone-to-speaker switch. It must not be advertised as a
physical-headphone-loss safety guarantee. Host route discovery/selection and real
hotplug validation remain separate P4 work. Actual device/audible tests require
fresh operator opt-in; release approval does not authorize them.
