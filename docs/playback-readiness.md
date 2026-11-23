# Playback readiness errors (v0.34.3)

The operator reviewed the native preview and approved this version.

When TIDAL rejects a playback authorization request with **HTTP 401/substatus4005**,
the player advances to the next shared-queue occurrence instead of stopping on that
song. A notice retains code4005 and the failure count. This does not establish why
TIDAL rejected the asset, nor imply that the song was deleted from its catalog.

- Confirmed missing catalog tracks and code4005 readiness failures share a ceiling
  of **five consecutive failures**. On the fifth, playback stops with an explicit
  explanation. There is no indefinite queue traversal, including repeat-context.
- Real successful playback, explicit Play/retry or manual navigation resets the
  streak. Authorization success alone does not. Paused playback or an output fault
  does not auto-resume; an exhausted queue remains stopped.
- Queue occurrence, account and playback-generation guards reject stale replies.
  Duplicate entries, manual Up next priority, shuffle and repeat policy are retained;
  a rejected track is skipped with a manual advance, not a natural-completion event.
- Availability markers are temporary/account-scoped and clear on successful playback
  or account-session changes. They do not permanently label the catalog as deleted.
- During Cast preparation the same policy applies only after acknowledged output
  release, before receiver LOAD. An active receiver or unknown release blocks a skip.
  New occurrences still pass through the normal local-output-release handshake.
- Prefetch failure does not skip the currently playing song. Other authorization or
  subscription errors, rate limiting, network failures and decoder failures retain
  their stop/error behavior. No credentials are renewed because of4005, no same-asset
  retry or quality downgrade is added, and no access control is bypassed.

Validation uses synthetic API responses, inert audio, and in-process Cast channels.
No real account, track, speaker or receiver playback is exercised by these tests.
