# P7a authorized current-track delivery preflight

**Later result: [a whole current TIDAL song played audibly](cast-music-validation.md)
on the separately approved alternative receiver, without a server/relay. Owned
cleanup passed. A later approved short excerpt also passed automated natural-end
confirmation after the unsolicited-event/startup handling fixes.**

The development native worker now passed [three-codec control/seek validation](cast-native-validation.md).

The sections below retain the earlier preflight history, not current playback
permissions. All described live scopes have been consumed.

## Earlier short-music approval and readiness

After the [audible public inline-DASH proof](cast-inline-dash-validation.md), the
operator explicitly approved music at **100%** on that alternative receiver: one
current-track test, only if idle, at most ten seconds, local player paused, owned
session cleanup and starting-volume restoration. Its explicit legacy-device-chain
policy remains separate from the original strict-authenticated target below.

The initial **local-only readiness check failed the saved-expiry safety margin**:
its recorded `expires_at` was not more than 60 seconds beyond the local clock.
This is not evidence that TIDAL rejected the token or that the running player's
session is invalid. The operator reports that music still plays. The player can
refresh credentials during normal API calls and tries to persist them; failed
persistence retains fresh credentials in memory with a warning. Already-authorized
streams can also continue playing. Verify saved-state/profile readiness before
requiring reauthentication. No playback-information lookup, receiver contact,
token refresh or credential mutation occurred in that probe. A later local check
passed without agent-side credential changes. Finite account-media sending and
account/player guards were subsequently implemented and privately verified;
[the music proof](cast-music-validation.md) records the separate excerpt and whole-song
approvals/results. No downgrade, automatic retry, listener, relay or local playback
fallback was authorized.

## Earlier scope and observations

The operator selected a reachable HDMI target and authorized the current track at
50% device volume for up to ten seconds, only if idle, with owned-session stop and
prior-volume restoration. No account mutations or relay were authorized. The earlier
Chromecast Audio tone target was clarified to be in the garage; it was not reused.
Addresses and receiver identifiers remain in ignored private records, not this file.

- The older saved HDMI address timed out. Two separately authorized, bounded mDNS
  discovery attempts found no receivers. No services/firewall settings were changed.
- The operator supplied two candidate addresses. One bounded Cast-port check per
  supplied address found both reachable, and the operator selected one.
- That selected HDMI receiver authenticated with `audio_only=false` under the
  **strict** certificate policy. No legacy device-certificate exception was needed.
  No application commands were sent by the authentication probe.
- The first account preflight stopped before a network request at the saved
  sign-in expiry safety guard. The operator refreshed it in the player and confirmed pause.
- The next preflight matched the active account, current queue occurrence and paused
  MPRIS metadata, then obtained an authorized inline DASH stream at the current
  configured quality. No receiver-fetchable manifest URL was available.
- The operator explicitly approved a temporary HIGH/AAC-quality lookup for the same
  track. That also returned inline DASH. The player's quality preference was never
  changed. No further quality fallback was attempted.

Neither lookup sent a media URL to a receiver or changed receiver volume. Signed
URLs, credentials, account identifiers and track metadata were not printed or
retained as probe output. Do not treat successful manifest authorization as proof
of remote playback.

## Read-only native preflight

`TIDAL_CAST_READINESS_ONLY=1` makes the explicitly opted-in preflight return after
checking local sign-in/current-track/paused-player readiness, before constructing
an HTTP client or making a playback lookup. It never contacts a receiver.


`src/cast_stream_tests.rs` is compiled only for tests. Its ignored live preflight
requires `TIDAL_CAST_CURRENT_TRACK_PREFLIGHT=1` and exact test selection; the optional
`TIDAL_CAST_PREFLIGHT_AAC=1` lookup additionally requires explicit quality approval.
Do not run all ignored tests.

It reuses the production credential loader, including native Secret Service and
metadata/digest validation. It does not refresh/save/migrate/clean credentials.
The token must have more than sixty seconds remaining. It rejects an account or
queue-occurrence mismatch and requires the running player to be paused with matching
title/artist/album before and after the request. It rechecks credential metadata and
current track afterward. No local playback command is sent.

The one GET uses the normal playback-authorization endpoint and account country,
without redirects, proxies or automatic retries. Its HTTP timeout is ten seconds,
response limit one MiB, and overall preflight deadline twenty-five seconds. The
production manifest parser rejects protected/unsupported streams. Only a static
stream-shape classification is reported; transport errors are redacted.

The pure account/occurrence regression and normal application suite pass on both
Rust 1.95 and 1.99.1 (445 normal tests, 11 opt-in tests excluded), with strict all-target
Clippy and formatting. The explicitly approved live checks above are separate from
that default suite.

## Earlier delivery decision

An inline SegmentTemplate manifest describes initialization and media fragments;
it is not itself a remotely retrievable MPD. Do not invent an asset/manifest URL,
silently lower quality again, upload account media, use a public proxy or bypass
subscription/DRM controls.

The subsequent [public-source investigation](cast-tidal-receiver-research.md) found
that TIDAL's own Web SDK wraps inline DASH in a `data:` URL for its local player.
That motivated the no-server synthetic experiment, which subsequently succeeded;
the later music proof also established audible playback for one account track. A published TIDAL-specific receiver
handoff was not established. Do not treat a relay as the only conceivable route.

A local manifest/media relay would require a separately reviewed design and explicit
authorization before opening a listener. That review must address access control,
signed-URL exposure, HTTP/TLS and receiver-origin behavior, request/byte/lifetime
limits, cancellation/cleanup, and account/track/receiver identity changes. No relay
or transcoding implementation has been enabled. The one-track audible music proof
does not clear broader codec, expiry, target integration or production-reliability
gates. See the music-validation record for the later terminal-state fix and bounded
positive completion proof.
