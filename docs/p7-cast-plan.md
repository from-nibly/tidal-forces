# P7 — Google Cast, gated by a working authorized path

**Release update:** the operator approved v0.34.0 publication and managed installation.
The cohesive player picker, metadata and shared-queue implementation is complete;
[release notes](releases/v0.34.0.md) distinguish verified behavior from remaining
final-build live receiver gates. The proposal/history below records earlier phases.

**Status:** P7 implementation/private feasibility work is authorized and has started
as `0.34.0-dev`; v0.33.0 remains published and installed. Separately approved,
bounded read-only inspection of the two saved targets is complete. The operator also
approved authentication-only connections to one Chromecast Audio, then one explicit
legacy-device-certificate retry. That retry authenticated successfully. A separately
approved [bounded public FLAC tone](cast-tone-validation.md) also played audibly,
with test-app stop and prior-volume restoration confirmed. A later separately
approved [public inline-DASH AAC-LC test](cast-inline-dash-validation.md) also played
audibly without a listener/relay, with owned-app stop and idle confirmed. Its starting
100% volume was preserved; earlier intermittent status/cleanup failures remain.
A later separately approved [whole TIDAL FLAC track](cast-music-validation.md)
also played correctly according to the operator, without a server/relay, with
owned cleanup confirmed. Its failed final-state assertion was reproduced privately
and fixed; a subsequent approved 7.988-second excerpt passed natural-completion and
cleanup checks. Private shared-queue/target and acknowledged local-output-release
contracts now back a wired native worker, playback routing and Devices UI.
[Native control validation](cast-native-validation.md) passed actual TIDAL FLAC,
AAC-LC and HE-AAC at44.1kHz, including paused forward/backward seeks and playing
seeks on short excerpts and full track timelines. Native preview review and broader
recovery remain gated. Live permission is date/target scoped, not granted by this
document; other devices, discovery, disruption, preview replacement, publishing and
installation retain their separate gates.
Broader product capabilities remain gated. See the
[initial source audit and remaining proof gates](cast-feasibility.md).

## Recommendation and goal

Use P7 as the next delivery label for the existing **G1 Google Cast** milestone in
[the capability ledger](feature-plan.md#7-network-playback-google-cast-and-tidal-connect-are-separate-tracks).
There was no previously defined P7. This is a proposal, not a claim that Cast is
already supported. Keep genuine TIDAL Connect separate and conditional.

Goal: select a Chromecast from Tidal Forces and control authorized playback using
the existing native main window, mini player, CLI and media keys, without duplicate
local audio, compromised credentials or invented quality claims.

Primary target: the operator's Chromecast Audio. Compatibility target: the regular
Chromecast; model/generation/firmware remain unverified. Private addresses stay in
ignored local notes. The computer's USB output is not a Cast test target.

## Baseline and scope

The v0.33 baseline has shared `Action` commands (`src/actions.rs`), occurrence-aware
queues, generation guards, and native local audio. `src/ui/actions.rs` still routes
transport into the local player in that baseline. The development branch now
routes shared actions through the selected native target. P7 retains this boundary,
not replacing
the native decoder or create a second authoritative queue/history owner.

In scope after approval: bounded opt-in discovery/manual targeting, native Cast
session control, explicit transfer, truthful target state, local queue preservation,
and documented per-device playback/control/quality capability results.

Out of scope: TIDAL Connect, DLNA, laptop receiver mode, automatic device takeover,
permanent background discovery, implicit LAN relays/transcoding, DRM bypass,
non-English translations, MilkDrop, new platform ports and account organization.
Local PulseAudio/PipeWire routing remains OS-managed, as requested. Remote Cast
loss must not trigger local playback; this does not reinstate local sink pinning.

## Delivery steps and stop gates

### 1. P7a — prove feasibility before product work

- Review public Cast protocol/library documentation, maintenance and licenses.
  Select a native Rust route only after examining device authentication, framing,
  heartbeat, cancellation, timeouts and payload bounds. No browser/runtime helper.
- Establish an authorized receiver application/media-loading contract. Do not
  assume TIDAL's official receiver accepts this client's session, or that a locally
  playable signed URL can be fetched by a Chromecast.
- Record receiver codec/container, TLS/device-authentication, authorization,
  URL-expiry, seek/range and DASH requirements. Do not borrow client credentials,
  suppress trust checks or circumvent subscription/DRM controls.
- With separately scheduled device-test permission, identify the two receivers and
  whether they are busy. Start on Chromecast Audio with a permitted synthetic
  fixture at a confirmed safe volume, then an authorized service track.

**Exit:** repeatable native control and actual receiver playback over an understood,
authorized delivery path. A moving clock or accepted LOAD response is not proof.
If receiver authorization/trust or media delivery cannot be established, stop and
produce a specific blocker report. Do not ship a decorative Cast picker.

**Conditional relay gate:** prefer direct receiver fetch. If that cannot work,
present a separate relay design for approval before opening a listener: narrow
interface binding, receiver-scoped access, short-lived unguessable URLs, bounded
memory/backpressure, range/expiry behavior and shutdown with the session. Forward
no service credentials to the receiver; redact all media/capability URLs. Any
format conversion must be separately justified and disclosed, never silent.

### 2. P7b — build the target boundary with private fixtures

- Route shared transport through Local or Cast capabilities. Keep native decoding,
  local DSP and local queue behavior unchanged in Local mode.
- Introduce target/session generations in addition to account/play generations.
  Reject late events after disconnect, transfer, account change or another sender's
  takeover. Bound queues, workers, frames, retries and shutdown.
- Publish one selected-target snapshot: transport, confirmed position/duration,
  volume, metadata and verified format/capabilities. Receiver reports, not local
  decode progress, own remote state. Unknowns remain explicitly unknown.
- Preserve local queue occurrences independently. Avoid duplicate advancement,
  history/listening credit, lyrics clocks and recommendation requests. Keep remote
  queue edits unavailable unless the proven contract supports them.
- Test account switches, duplicate tracks, stale replies, out-of-order status,
  ambiguous command outcomes and teardown using private mock peers. Do not blindly
  retry a LOAD or command that might already have succeeded.

**Exit:** local regression suite stays green; target changes cannot cross accounts,
advance two queues, double-credit listening or start two outputs.

### 3. P7c — deliver the Chromecast Audio vertical slice

- Add a Devices surface distinguishing **This computer** from **Google Cast**.
  Discovery is explicitly enabled and bounded; opening the app never scans the LAN.
  Treat advertised device names/endpoints as untrusted input; validate and cap them.
- Separate device selection from **Transfer playback**. Selection alone neither
  launches a receiver application nor starts audio. Confirm before interrupting a
  busy receiver; never seize it automatically.
- Implement an explicit transfer state machine. Quiesce local output before sending
  a remote start; commit remote ownership only on matching confirmation. On timeout
  or ambiguous failure, remain safely paused/unconfirmed and offer explicit recovery
  rather than resuming locally or retrying a start speculatively.
- Wire confirmed play/pause, seek, volume, next/previous and app-owned queue advance
  through main, mini, classic skin, CLI and MPRIS. Advertise only supported actions.
- Keep direct-Cast DSP and PCM visualizers inactive. Show verified source/receiver
  format separately, or **format not reported**; do not reuse local DAC badges.
- On disconnect, takeover or restart, retain the local queue and require explicit
  return-to-local playback. Never auto-reconnect in a way that seizes another session.

**Exit:** an operator-reviewed native slice plays and controls actual authorized
media on Chromecast Audio, with no double audio or misleading UI.

### 4. P7d — compatibility, resilience and review

Repeat on the regular Chromecast and record its actual model/firmware. Test the
following per receiver under explicit permission, with no unsupported capability
inferred from the other device:

- Synthetic fixture and authorized FLAC/AAC delivery; actual audible/sample evidence.
- Pause/resume, seek/ranges, volume, duplicate occurrences, repeat/shuffle and queue
  boundaries; disclose any unsupported behavior or audible transition gap.
- Long playback/URL expiry, token refresh and cancellation while preparing/loading.
- LAN loss/reconnect, receiver restart and another sender taking control. Destructive
  disruption tests require an agreed window; never reboot a device uninvited.
- Target switching while paused/playing, application quit/relaunch, account change,
  stale receiver status and unsupported remote queue operations.
- Privacy/log redaction, oversized/malformed frames, untrusted endpoints, bounded
  resource use and idle behavior with discovery off. No credential or device-address
  material in screenshots, fixtures or releases.
- Native keyboard/readability/layout checks at minimum, normal and zoomed sizes,
  including Dark/Light/custom palettes, mini/classic and lyrics state.

**Exit:** a per-device capability/evidence record and native review. Both devices
must have repeatable results or explicit limitations; mocks alone cannot establish
Cast support. Only then request version approval, publish and install separately.

## Proposed implementation locations

After feasibility, expect a small Cast worker/transport module plus target state and
capabilities, shared-action dispatch integration, and a Devices UI. Review touchpoints
include `src/ui/actions.rs`, `src/player_state.rs`, `src/ui/playback.rs`,
`src/ui/playback_next.rs`, `src/playback_position.rs`, `src/queue.rs`, history/radio,
MPRIS and native quality reporting. Names and dependency choices are not committed
by this plan; choose them after the protocol proof to avoid speculative abstractions.

Keep existing local persisted state compatible. Never persist signed URLs or tokens
in device preferences. Default to Local/paused on launch; do not restore a remote
session by silently launching a receiver or starting audio.

## Approval boundaries and first action

1. P7 implementation has been authorized. This permits code/private tests,
   not live LAN probes, receiver launches, audible output or account mutation.
2. Separately schedule discovery/control and a safe-volume playback window for
   Chromecast Audio, then the regular Chromecast. Confirm their identities and
   whether takeover/disruption tests are allowed at that time.
3. Review any necessary relay before listener implementation; it is not assumed.
4. Keep the current preview unchanged until explicit replacement permission. Work
   on a development branch with a `-dev` version once implementation is authorized.
5. Native version approval is required again before publication and managed install.

**Recommended first implementation milestone:** P7a feasibility only, with an explicit
stop/go result before investing in the Devices UI. TIDAL Connect partner access and
eligible hardware remain a separate unresolved track, not a dependency for P7.
