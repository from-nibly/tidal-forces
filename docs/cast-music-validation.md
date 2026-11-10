# P7a authorized TIDAL music proof

## Latest result

[Native integration/control validation](cast-native-validation.md) now also covers
TIDAL FLAC, AAC-LC and HE-AAC at44.1kHz, with pause/resume and paused/playing seeks
on short excerpts and full manifest timelines. Owned cleanup passed. This is separate
from the historical whole-song and natural-completion evidence below.

**Natural completion is now verified on the real receiver.** A separately approved
7.988-second current-quality FLAC excerpt was allowed to end naturally. The native
probe retained an unsolicited `FINISHED` event followed by an empty correlated
status reply; owned cleanup and unchanged player identity also passed.

The receiver first acknowledges LOAD as `IDLE` without a recognized terminal reason,
then reports buffering/playing. That initial state is now pending startup—not
completion or failure. Explicit cancellation/error/interruption still fails, as
would an unknown idle state after startup. A private TLS reproduction covers this
startup sequence and the completion event/empty-reply ordering. No success is
inferred from elapsed time or disappearance.

The shared reducer is in `src/cast_media.rs`. Completion is scoped to the confirmed
LOAD/session/content and delivered once. Wrong app routes, stale replies, foreign
media and conflicting terminal states cannot advance the queue. Diagnostics contain
only a bounded sequence of schema enums, not peer values, URLs or identifiers.

The earlier whole-song audible result and failed pre-fix assertions are retained
below as history. The full-length song was not rerun with the new collector; the
current automated positive proof is the finite excerpt's natural completion.

**The operator confirmed hearing a whole current TIDAL song correctly on the
selected alternative Chromecast.** The source was clear FLAC at 44.1 kHz, delivered
using an inline `data:` DASH manifest and direct receiver-to-CDN segment fetches.
No local server, audio relay, transcoding, borrowed credentials or DRM workaround
was used. This was a separately approved music test at 100% volume.

Owned-app stop and receiver idle were confirmed. The actual starting volume was
100% and was preserved, not lowered. The local player stayed paused under the
probe's account/occurrence/player guards. The test did not change its queue,
quality preference, credentials or installed executable.

**The earlier automated whole-track runs were not full passes:** it observed
playback progress but did not confirm the final `FINISHED` state. Both whole-track
runs reached this same strict failure after roughly the expected track length,
while cleanup succeeded. This observation gap was subsequently reproduced and
fixed as described above. The operator missed the first run and explicitly
approved another, confirmed readiness, and then confirmed hearing the whole song.
A missing completion observation is not proof of an audio failure, but user-heard
success must not erase the unresolved terminal-state check.

This proves one source/receiver/account-media path, not general Cast support,
bit-perfect receiver output, all qualities/codecs, expiry recovery, seeking,
queue integration, or an officially supported TIDAL-specific receiver handoff.
No release/install approval or additional live-test authority follows from it.

## Sequence

1. The first local readiness check failed the saved-token 60-second safety margin.
   The operator correctly questioned the claim that their live sign-in was expired.
   The running player can refresh credentials during normal API calls; the probe
   cannot. A later read-only readiness check passed without agent-side credential
   refresh or a forced sign-out/sign-in.
2. A test-only finite `SegmentList` excerpt was implemented. Only complete prefix
   segments fitting at most eight seconds are included, so a control failure cannot
   leave an entire track playing when only an excerpt was approved. One authorized
   current-quality FLAC excerpt passed playback-progress and cleanup checks.
3. The operator requested a whole song. A separate explicit whole-track mode was
   implemented, rather than silently extending the excerpt mode.
4. The first whole-track attempt stopped at manifest validation, before receiver
   contact. A newly approved read-only structural inspection emitted only recognized
   schema names/counts and namespace categories—no values, text, URLs or IDs. It
   identified a standard DASH `Role` element missing from the allowlist.
5. Narrow support was added for an empty standard `main` role under an adaptation
   set or representation. Other roles/schemes, unreviewed attributes/placement,
   protection, external manifest references and unreviewed elements still fail
   closed. The fix was verified privately before the newly approved whole test.
6. The whole-song attempts produced the latest result above: audible success
   confirmed on the second run, cleanup confirmed, terminal-state validation open.

## Scope and guards

The test-only integration is in `src/cast_stream_tests.rs` and
`src/cast_clip_tests.rs`, reusing the isolated native authentication transport in
`examples/cast_auth/`. It is not a production Devices UI or playback target.

- Exactly one fresh playback-information request at the paused track's existing
  requested quality. Returned quality must match; no automatic downgrade, refresh,
  retries, direct-full-file fallback, listener or local playback fallback.
- The native credential loader verifies stored metadata/keyring contents without
  mutation. Metadata and any legacy credential-file revisions are checked before
  lookup and throughout control phases. Account, current occurrence and quality
  must remain unchanged.
- The running player's unique D-Bus owner is pinned. Uncached MPRIS metadata and
  paused state are checked before lookup, launch, volume changes, LOAD and during
  polling. An unavailable/changed guard stops the test and attempts owned cleanup.
- Original stream parsing rejects protection. Media URLs must be HTTPS on
  `audio.tidal.com` or its subdomains, with no user-info, fragments or nonstandard
  ports. Only the signed media URLs go into the receiver's manifest; account access
  and refresh tokens never go to the receiver.
- Manifests remain in memory only. No signed URLs, credentials, account IDs, track
  names or raw receiver messages are emitted into probe output.
- The selected receiver's explicit legacy-device-certificate exception retains
  roots, validity/key constraints, fresh SHA-256 nonce/TLS binding and mandatory
  signed revocation. It is not a default policy change for other receivers.
- Receiver must be idle and unmuted. Volume is not raised until the owned empty
  app is ready. Existing sessions/media are never taken over. One LOAD only;
  app/media session identity and content identity remain guarded.
- Cleanup attempts to stop only owned media/app and restore the starting volume,
  without overwriting an external volume change. Fatal IO or ambiguous ownership
  can still make cleanup unconfirmed; manual receiver control may then be needed.

## Whole-track bounds

`authorized_current_track_whole_playback` is a separate ignored test requiring
explicit whole-track, current-track, 100% volume and legacy-device-chain opt-ins.
The excerpt entry point rejects the whole-track flag. No live ignored test should
be invoked without fresh scope; never run all ignored tests.

The whole manifest must be static, at most 8192 bytes, with one period,
representation and segment template, reviewed schema features and a finite track
of at most fifteen minutes. No next-track or queue request is sent. Saved-token
validity and authenticated receiver proof must cover the full test window.

Authentication retains its 512 KiB wire bound. Only after verified authentication,
whole-track control traffic has a 32 MiB ceiling, one-second polling, and a finite
track-duration-plus-grace deadline. Proof expiry is checked during IO. Launch/LOAD
responses have at most eight seconds; other requests retain two seconds and every
request is capped by the overall deadline. Whole-track success requires observed
completion, not merely positive position or elapsed wall time.

An optional private cancellation marker makes the account/player guard fail and
attempt owned cleanup. It does not authorize reconnects, another LOAD, stopping
foreign sessions or changing the running player's credentials.

## Timeout recovery and verification

Application-frame state now retains partial headers/payloads across read timeouts
on the same authenticated TLS connection. Cleanup can finish a late frame, discard
unrelated request IDs and obtain fresh status. There is no reconnection or LOAD
retry. EOF, malformed frames, bounds failures and write failures remain fatal.
Media-status queries include the known media-session ID after LOAD.

Private TLS cases cover missing replies and timeouts partway through headers or
payloads, followed by confirmed owned cleanup. Other tests cover account-guard
changes before/during playback, external ownership/volume changes, finite excerpts,
whole-track completion requirements, duration limits, schema redaction and narrow
role handling. Local synthetic AAC and FLAC fixtures also decoded through finite
SegmentList documents using file-only FFmpeg, without an audio device or network.

Latest regular suite: 506 passed, 15 ignored on Rust 1.95. Cast example: 56 regular
tests plus the two exact private contracts. Focused tests and strict all-target
Clippy also passed on Rust 1.99.1. The real whole-track test nevertheless **failed
its terminal-state assertion**; do not represent the private suite as overriding
that live result.

## Remaining integration gates

Completion handling has passed private positive/negative contracts and the real
finite-excerpt test. Product integration remains in progress: the private target
boundary in `src/cast_target.rs` uses the existing occurrence-aware queue and guards
account/play generations, duplicate/repeat-one completion, ambiguous outcomes and
explicit return to Local. It is not yet wired into the native UI/worker.

Local stop requests are asynchronous, and MPRIS also reports paused while buffering.
Neither is sufficient proof of a released local output. A test-only native handoff
command waits for the native audio server's flush acknowledgement, closes the output
on its control thread, and acknowledges only if its playback generation still
matches. Isolated null-Pulse and null-PipeWire contracts confirm consumption stops
and the released stream disappears. Compatibility-output transfer remains unverified
and fails closed. A server flush cannot retract samples already sent to hardware. Production transfer must wait for that handshake;
it must not infer quiescence from a pause flag. No decoder/network/disk work is added
to the audio callback.

The regular DASH parser now preserves its already-authorized document in bounded,
shared memory for future receiver delivery without a second account lookup. Manifest
and segment Debug output redact URLs. No manifest persistence was introduced.

Native worker/action routing, capabilities, controls, queue/history/position
integration, UI/review and broader resilience validation remain open. The operator
has explicitly allowed repeated needed at-most-eight-second tests at 100% on the
same selected receiver under the existing idle/local-pause/owned-cleanup guards.
This is not permission for other devices, discovery, full-song retries, takeover,
disruption tests, publishing or installation; stop on uncertain ownership/cleanup.
