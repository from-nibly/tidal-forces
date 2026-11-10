# Native Cast integration and codec/control validation

## Development status

The cohesive [player Cast controls](cast-player-controls.md) are implemented across
the main, mini and classic players: named streaming device picker, no manual address
or certificate UI, confirmed-only connected state, connecting/stopping animation,
viewport-owned picker and explicit return-to-local paused. Receiver artwork and
track metadata are supplied on every LOAD. Native shared-queue tests cover full
continuation, repeats, shuffle, manual priority and next/previous, including rapid
Next/end-of-queue and paused-completion/pagination races. The running preview was
replaced with this latest UI after explicit approval and verified paused/visible.
User review remains pending; live artwork/queue validation requires separate approval.

The final private checks for this cohesive pass succeeded on Rust1.95 and1.99.1:
**564 application tests,72 Cast example tests**, strict all-target Clippy. Exact
private authentication/TLS and isolated null-Pulse/PipeWire contracts passed again.
No live playback/account/device-session tests were performed during this pass.

The 0.34.0-dev native authenticated worker, backend source preparation, acknowledged
local-output release, shared playback actions, occurrence-aware queue integration,
position/history handling and Devices controls are wired. Installed v0.33 remains
unchanged. After explicit approval, the previous paused player was gracefully closed
and the 0.34.0-dev native review preview was opened paused. User review/version
approval remains a gate; opening the preview does not authorize publication/install.

The implementation uses one existing queue and one authenticated session per LOAD.
Account/play-generation/occurrence guards fence stale preparation and local events.
There is no automatic reconnect, quality downgrade or local playback fallback.
Unknown release blocks replacement playback; an explicit authenticated, read-only
idle check can clear that block without stopping a foreign session. Returning to
local output remains paused. The operator approved built-in reviewed Cast legacy certificate-chain compatibility
for the product, rather than a user-facing certificate setting. Trusted roots,
validity/key/path constraints, signed revocation and fresh SHA-256 nonce/TLS binding
remain mandatory. Strict-only policy remains available in the isolated probe. This
compatibility policy is not equivalent to modern certificate-signature strength;
see [certificate investigation](cast-legacy-compatibility.md).

Only reviewed clear DASH is accepted, with a bounded static full manifest and a
15-minute track limit. Authorized XML and signed media URLs stay in RAM. The receiver
fetches CDN segments directly: no listener, relay, transcoding or account token is
sent to the receiver. Receiver output format is not reported; local DSP and
visualization do not apply. TIDAL Connect remains a separate capability.

## Real receiver results — 2026-11-09

The operator renewed testing permission for the selected receiver for this date.
Readiness was restored through the existing player by the operator, not external
credential refresh or migration. Every test required the local player paused,
unchanged account/occurrence/credentials/player identity and an idle, unmuted receiver.

TIDAL returned three distinct codecs for the current track:

- LOSSLESS: FLAC, 44.1 kHz.
- HIGH: AAC-LC, 44.1 kHz.
- LOW: HE-AAC, 44.1 kHz.

Each passed the actual native worker's control sequence on a finite 7.988-second
excerpt and again using the full manifest's approximately 290-second timeline:
PLAYING, confirmed PAUSED, forward seek while paused, backward seek while paused,
resume, seek while playing, and owned-session stop/release. Full-timeline seeks
reached approximately 189 seconds while paused, returned to 1 second, then reached
approximately 145 seconds while playing. These were bounded control tests, **not
whole-song playback or a new full-song natural-completion proof**.

All six successful runs confirmed release, preserved the actual starting 100%
volume without sending volume commands, and left the installed player unchanged.
These are receiver protocol/control observations, not new human audibility reports
or proof of the receiver's physical output format. They do not establish all tracks,
sample rates, HE-AAC v2, direct/BTS sources, expiry recovery or other receiver models.
Earlier operator-heard whole-song playback and the explicit natural-end excerpt
proof remain separately documented in [music validation](cast-music-validation.md).

Two initial native runs failed a media-status timeout after LOAD; owned cleanup
succeeded both times. Redacted phase diagnostics localized the failure. The worker
now allows up to eight seconds for startup media-status replies, retains the normal
two-second steady-state budget, delays its first poll 250 ms and caps startup at
30 seconds. No failed LOAD is repeated and no transport is reconnected. Private
regressions distinguish startup and steady-state budgets.

Integration checks also caught a terminal-channel issue: after a valid release and
worker exit, channel disconnection could be misreported as another, unconfirmed
release. Terminal delivery is now one-shot, including worker-loss failures. UI tests
cover worker drop, duplicate completion, stale local events, seek serials and pending
volume confirmation. A failed/closed worker clears unconfirmed volume intent.

## Verification and boundaries

Rust 1.95 and 1.99.1 checks after these fixes: 542 application tests passed,
17 ignored; 71 Cast example tests passed, two ignored; strict all-target Clippy
passed on both toolchains. Formatting and diff checks passed. The exact private
TLS/authentication runner and isolated null-Pulse/PipeWire handoff contracts passed
again on the final development code. No blanket ignored-test run is allowed.

The native helper is an explicitly ignored, separately gated test:
`cast_stream_tests::authorized_current_track_native_controls`. It requires a bounded
permission deadline, private selected target, fresh account/player guards and
explicit legacy policy. Quality overrides affect only that lookup, not player
preferences. Full-manifest control tests have an observer deadline and cleanup grace.
Private local wrappers additionally pin the date, binary digest and player identity.
Documentation is evidence, not continuing permission for any live test.

Remaining gates include native preview/physical handoff review and broader recovery,
expiry and device compatibility. Publishing/installing and replacing the existing
review preview require their separate approvals.
