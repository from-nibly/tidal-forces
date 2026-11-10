# P7a public synthetic inline-DASH experiment

**Latest result: audible public AAC-LC inline-DASH playback confirmed by the
operator, with test-app stop and receiver idle confirmed.** No local server or
audio relay was used. This is a synthetic-fixture result, not a TIDAL account-media
or general Cast capability claim. Earlier intermittent control/cleanup failures
remain a reliability issue.

## Scope and fixture

This development-only probe tests whether the Default Media Receiver accepts a
small inline MPD encoded as a `data:application/dash+xml;base64,...` content ID.
It does not use a TIDAL account, account media, signed service URLs, credentials,
a local listener, relay, transcoding or the running player's audio path.

`tests/fixtures/cast-inline.mpd` references the already-public AAC-LC synthetic
440 Hz fragments at release source `c83b8c2554416044824afd08367c10fdcd50c119`.
The timeline is four 45056-sample segments followed by 39936 samples at 44100 Hz,
about five seconds. Before the experiment, all six public initialization/media
files were fetched and matched byte-for-byte to the local fixtures; each response
advertised `Access-Control-Allow-Origin: *`. No media was uploaded.

## Explicit CLI contract

The example's exact argument modes are:

- `--dash-data-url-test-ip-stdin --volume-percent=50`: strict device certificates.
- `--dash-data-url-test-ip-stdin --allow-legacy-device-certificates --volume-percent=50`:
  requires separate explicit legacy-device-certificate approval for that receiver.

Both modes also accept `--volume-percent=100` instead of 50, but only with
separate explicit approval for that test. Omitting the volume or supplying another
value fails closed. Neither mode implicitly raises volume to 100%.

Both take one private IPv4 address on stdin. These are not permission to run a test.
Do not reuse a consumed live authorization, discover another target, or fall back
to a different volume/representation/receiver automatically.

The original public FLAC tone mode retains its distinct exact 100% opt-in; it is
not the mode used here. Strict remains the default. The explicit legacy mode does
not relax fresh SHA-256 nonce/TLS binding, roots, validity, key constraints or
mandatory signed revocation.

The shared exercise refuses active applications (including paused media apps),
muted/unknown states and unexpected media. It verifies the owned app identity,
sends at most one LOAD, checks media-session identity, and attempts to stop only
its own media/app and restore the previous volume. The short static fixture,
bounded polling and transport deadlines limit the experiment. Launch and LOAD
requests now allow up to eight seconds, capped by the unchanged overall deadline;
other requests retain two seconds. Private TLS tests cover launch and LOAD replies
delayed 2.25 seconds, beyond the former two-second limit. Cleanup can fail
on a lost/poisoned connection or changed ownership; it must not be reported as
successful merely because the fixture is short. There is no automatic reconnect,
LOAD retry or fallback playback.

## Observed live results

All attempts below were separately scoped by the operator. Full addresses and
receiver identities are intentionally omitted.

1. **Originally selected HDMI receiver, strict:** preflight failed after about two seconds,
   before the idle check or application/volume controls. The original diagnostic
   did not distinguish connection/TLS/authentication causes. No playback attempted.
2. **Alternative HDMI receiver, strict:** rejected RSA/SHA-1 in the device
   certificate path. No application commands or volume changes.
3. **Alternative receiver with separately approved legacy-device-certificate compatibility:**
   authentication passed; receiver was idle and unmuted; 50% and the owned idle
   test app were confirmed; one inline-DASH LOAD was sent. The probe failed and
   reported cleanup/volume restoration **unconfirmed**. The original combined
   diagnostic did not preserve the separate playback and cleanup reasons. The
   operator was absent and supplied no audible result.
4. **Explicitly requested retry on the same alternative receiver:** authentication passed; the receiver
   was idle/unmuted at **50%**. This attempt failed with `Receiver read failed`,
   followed by `Receiver channel is unusable` during cleanup. It **did not reach
   the LOAD stage**, so no tone was sent on this retry. Its final app/volume state
   is unconfirmed. The pre-first-attempt volume was not recorded and cannot be
   inferred from the retry's 50% reading.

5. **Separately approved 100% retry on the alternative receiver:** authentication
   passed and the receiver was idle at 50%. New stage/IO diagnostics identified
   `TimedOut` while awaiting the receiver LAUNCH response under the two-second
   request deadline. **No SET_VOLUME or LOAD was sent on this attempt.** The probe
   now defers the volume increase until its owned, empty app is confirmed ready;
   this prevented a failed launch from raising the volume. App launch may still
   have taken effect despite the missing reply, so final app state is unconfirmed.
   This result does not test DASH acceptance.

6. **Explicit post-reboot 100% attempt:** with the bounded launch/LOAD wait change,
   authentication, idle state at 50%, owned app readiness, and 100% volume were
   confirmed. The receiver acknowledged LOAD with the expected inline content ID
   and a media-session ID. The subsequent media-status request timed out, poisoning
   the connection; cleanup and restoration were unconfirmed. The operator again
   missed the test, so no audible result was established. A LOAD acknowledgement
   alone does not prove that the DASH audio decoded or played.
7. **Explicit ready-to-listen retry:** the operator confirmed stopping the app and
   restoring 50% before requesting another single attempt. Fresh authentication
   passed, but the receiver reported an active app/session. The probe aborted
   before any volume change, launch or LOAD. The operator was advised to use
   “Stop casting” rather than merely pause/stop the audio. No takeover or further
   retry was attempted.

8. **Retry after the operator explicitly killed the previous cast:** fresh
   authentication and idle/unmuted state passed. The actual starting volume was
   **100%**, not the earlier requested manual baseline of 50%, so no volume
   increase was needed. One inline-DASH LOAD produced reported playback progress;
   the probe stopped its owned app and confirmed receiver idle with the starting
   100% volume preserved. The operator confirmed hearing the synthetic tone twice.
   The operator was explicitly told the volume remained at 100% and to lower it
   as needed. This proves audible playback of this public AAC-LC DASH fixture via
   a `data:` manifest without a listener or audio relay.

After each unconfirmed-cleanup failure, the operator was immediately asked to
check the receiver and its volume manually.
No further live command is authorized by these consumed attempts. The final
successful attempt establishes audible synthetic playback and confirmed app
cleanup, but does not explain all earlier status/cleanup failures. **TIDAL media
compatibility, codec variants, signed-URL expiry and production reliability remain
unresolved.** The final starting volume was preserved at 100%, not lowered to 50%.

The subsequent [account-music proof](cast-music-validation.md) records later
transport hardening and separately authorized audible whole-track TIDAL playback.
The remaining sections here describe the earlier synthetic-only checkpoint.

## Private verification and next diagnostic gate

On Rust 1.95 the latest probe passed 51 regular tests, strict example Clippy/formatting,
and both exact private contracts through `tests/cast-auth.py`. The expanded TLS
contract covers strict modern-device inline-DASH control, rejection of a legacy
device in strict mode, and explicitly opted-in legacy inline-DASH control at both 50% and 100%. Fake
channel cases cover active/muted state, LOAD rejection, unexpected media, external
volume change, changed media-session identity, and terminal playback errors.

These passing contracts did not reproduce all live read/cleanup failures. The
hardware playback result rests on the final live receipt and operator confirmation,
not merely on the private contracts. The latest diagnostics
now preserve separate static playback/cleanup failures, allowlisted request-stage
labels and IO error kinds, and print the pre-test volume, without raw packets,
URLs, tokens or device addresses. Tests cover redacted stage labels, avoiding
redundant volume commands, and never raising volume when app launch fails.

Before requesting another live test, investigate offline:

- retain sanitized request-stage and IO-kind diagnostics (now available);
- post-LOAD status correlation/session identity and missing replies under the
  remaining two-second status budget; launch and LOAD now have privately tested
  eight-second windows capped by the original overall deadline;
- retain the new no-op avoidance when volume already matches, and the delayed
  volume increase until the owned empty app is ready;
- safe cleanup under connection failure without inventing ownership, restoring
  an unknown earlier volume, or silently introducing reconnects/retries.

Do not interpret transport uncertainty as a reason to weaken authentication or
media/session ownership checks. No TIDAL account-media attempt, listener, relay,
Devices UI, release or installation is cleared by this experiment.
