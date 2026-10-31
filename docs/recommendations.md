# Radio and bounded recommendation continuation (v0.32 development)

Track and artist radio use the existing TIDAL routes. Track menus distinguish
**Browse radio** (no playback change) from **Start radio** (explicit source change).
Refresh changes only the browsed view. **Save loaded radio as playlist** captures a
fixed list, preserving repeats, through the existing account-scoped export draft.
Opening that draft does not mutate an account: creation and bounded, read-back
playlist batches still require the explicit confirmation. Existing drafts are
preserved; a new loaded-radio snapshot can be chosen in the export dialog.

## Optional automatic continuation

Settings → General → **Continue with TIDAL track recommendations** is off by
default, including old preferences. When enabled:

- Only active, non-buffering playback with repeat off and a fully loaded source
  can request continuation. At most two upcoming songs remain before a
  request starts. Pending source loading and prepared audio have priority.
- Each lookup is seeded from the current **track**, not an invented album or
  playlist radio endpoint. Artist radio remains explicitly browsable/startable.
- At most **3 lookup attempts and 150 accepted tracks** per playback source/session
  or explicit re-enable; each response has at most 100 rows and each append at most
  50 tracks. Requests are not retried automatically after an error or an empty/
  duplicate-only response. Exhausted budgets stop further fetching.
- Deduplication covers the current source, manual Up next, current track, redo and
  bounded recent queue history, plus duplicates within the result. Known unavailable
  tracks are excluded. This does not promise globally unique or endless music.
- Manual entries and existing source entries retain priority. Shuffle randomizes
  only the new batch, not existing upcoming entries. Accepted recommendations are
  appended with new occurrence identities and an explicit source-title suffix.
- Results require matching account, request, queue revision, current occurrence
  and playback generation. The callback handoff is reconciled/revoked before the
  final append check. Pause, stop, source edits, account changes and repeat changes
  invalidate pending work. Clearing the source explicitly suspends continuation.
- A result never calls Play or resumes a stopped player. If a short song ends
  before a usable result arrives, the queue ends normally. There is no waiting
  loop, automatic restart, playback-error skip or infinite recommendation loop.

The worker uses a valid-token read-only snapshot with no credential refresh or
persistence, disabled redirects, an eight-second total timeout, a 1 MiB response
limit and one cancellable job. Provider errors stay local to continuation and do
not stop playback or overwrite playback errors. Non-track entries are excluded;
malformed metadata fails closed. No additional service or host is contacted.

## Verification limits

Normal tests cover mock authorization/status/redirect/body bounds; deduplication,
source/manual ordering, shuffle, budgets, stale scope, pause/end/clear behavior,
read-only radio browsing and immutable export drafts. Native synthetic radio and
General-settings fixtures exercise layout. These do not establish live account
recommendation availability or authorize real playlist creation. No native
album/playlist radio, infinite unique-track supply or physical playback guarantee
is claimed.
