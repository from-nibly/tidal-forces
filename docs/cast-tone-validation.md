# P7a bounded synthetic Cast playback proof

**Audible playback is confirmed on the operator-selected Chromecast Audio.** This
is an isolated development probe, not enabled Cast output in the player. No TIDAL
account media, credentials, LAN relay, device discovery or other receiver was used.

## Exact authorization and evidence

The operator approved an idle-only short synthetic-tone test with prior-volume
restoration, then changed the requested level from 5% to 50% before any playback.
The first 50% test reported progress and successful cleanup, but the operator was
not nearby. An explicitly requested retry again reported progress; the operator
reported no sound. The operator then requested 100%. That one-second test reported
progress and cleanup, and the operator confirmed **“Yes, I heard it.”**

All three attempts confirmed an idle, unmuted receiver before mutation, launched
only the default media receiver, loaded the fixed tone once, stopped the owned test
app and confirmed receiver idle/prior-volume restoration. No retries were automatic.
The 100% setting is specific to this synthetic test; it is **not** a recommended
music volume or permission to use that level for account-media tests.

The test permissions above are consumed. New live tests require fresh scope.
Do not run the CLI merely to check whether the saved target is still available.

## Fixed public fixture

The one-second 440 Hz synthetic FLAC was already published in this repository:

- Source commit: `c83b8c2554416044824afd08367c10fdcd50c119`.
- Path: `tests/fixtures/tone.flac`.
- SHA-256: `8bcb52b917c2ceea995a15939602ebc66fffe24146f6f5e6276c572cc9317064`.
- Public HTTPS fetch verified 20,324 bytes, HTTP 200, permissive CORS and advertised
  byte-range support. No credentials or signed account URL are involved.

The receiver fetches that immutable raw GitHub URL directly. The probe cannot load
an arbitrary URL, playlist or track. No new public upload or LAN listener was made.
See the fixture README for its original local generation command.

## Guarded probe

`examples/cast_auth/tone.rs` runs only after the full quarantined TLS/device-auth
proof, including the separately approved legacy device-certificate policy, fresh
SHA-256 TLS binding and mandatory signed revocation. It has no access to accounts.
The usual authentication-only mode still cannot send playback commands.

The audible CLI now requires all three exact flags: `--tone-test-ip-stdin`,
`--allow-legacy-device-certificates`, and `--volume-percent=100`. The last explicit
volume guard was added after the confirmed test and privately regression-tested;
old two-flag invocations fail closed. Target input is one private IPv4 address on
stdin, never discovery. This interface is not installed with the player.

The protocol proof:

1. Reads authenticated receiver status; rejects active apps, unknown idle state,
   muted/unknown mute state, and malformed volume. Only absent apps or the known
   idle Backdrop qualify. Does not unmute the receiver.
2. Sets the expressly approved level and rechecks idle state before launch.
3. Records the launched default receiver's session and transport identities. Checks
   empty media state, unchanged receiver ownership and acknowledged volume before
   the single LOAD of the fixed one-second fixture.
4. Polls at most eight times for remote progress/completion. It never blindly
   repeats LOAD. Receiver status alone is not treated as audible proof; the operator
   confirmation above supplies that separate observation.
5. Checks session, media identity and media-session identity before stopping only
   its own test app. Restores the previous volume and checks the acknowledged result.
   It does not overwrite an externally changed volume or stop foreign media.

Frames remain bounded to 64 KiB and aggregate encrypted traffic to 512 KiB. Requests
have two-second IO limits and 32-frame reply limits. Authentication gets at most
ten seconds; the authenticated test gets at most twenty additional seconds. Only
validated heartbeat PINGs get PONGs. Replies must match namespace/source/request ID.
No peer-controlled messages, session IDs or addresses are logged.

Cleanup is attempted even after an ambiguous mutation failure. A disconnected,
malformed or timed-out channel may make restoration impossible: that is reported
as **cleanup/volume restoration unconfirmed**, never as success. External ownership
changes also stop the test without taking over the new session. Cast has no atomic
compare-and-swap launch/volume API; preflight/read-back checks reduce, but cannot
eliminate, concurrency races. This is not yet the production ownership/recovery
implementation.

## Private verification and remaining gates

`python3 tests/cast-auth.py` runs the exact isolated TLS and legacy contracts using
fresh synthetic keys, including a full scripted authenticated tone/control exchange
and an authentication-failure case proving that no control frame follows failure.
Regular unit tests cover successful restoration, active/muted refusal, failed LOAD
cleanup, foreign media, externally changed volume, malformed idle state and exact
explicit CLI opt-in. No private test contacts saved receivers or plays local audio.

This proves one authorized public FLAC fixture through the default receiver, with
operator-confirmed audible output. It does **not** prove TIDAL media authorization,
account-stream delivery, expiry/range recovery, every codec/rate, remote quality,
shared queue/transport integration or the second Chromecast. Account-media tests
need separate permission, track choice and volume. The installed v0.33 player and
its running preview remain unchanged. P7a is not complete and no release/install
approval follows from this proof.
