# Folder capabilities and validation

## Supported scope

The v0.27 development implementation supports creating root-level folders,
renaming folders, and moving saved playlists between the library root and
uniquely named root folders. Moves affect library organization, not playlist
contents or the playback queue. Folder-to-folder moves are absent, so the player
cannot introduce folder cycles.

Every operation freezes its account and source identity, checks fresh listings,
sends one mutation, and requires read-back before reporting success. Folder reads
have account/request guards. Mutation verification rejects unknown item types,
duplicates, incomplete totals and repeated cursors, with a 1,000-entry bound.
The service exposes no usable folder revision guard: concurrent changes can race
an operation. Cancellation or uncertain results require Refresh/inspection, never
automatic retry or rollback.

## Live evidence and exclusions

With explicit operator authorization, a temporary-object round trip verified
root creation, rename, playlist moves into/out of a folder, and cleanup. The
probe used only newly created folders and an empty temporary playlist, did not
play audio, and prohibited credential renewal or migration. Private receipts were
retained locally; they must not be committed or included in screenshots/releases.

An earlier nested-creation probe found that `folderId` was ignored and the new
folder appeared at root. Its known empty root residual was removed separately
and root absence verified. Nested creation is now rejected before dispatch.

Removing the nonempty temporary folder left its owned playlist resource readable.
That does not establish membership/deletion behavior for arbitrary saved contents.
Folder deletion is therefore **not exposed**. Cover upload remains unavailable:
reference schemas mark it internal-tier and supported client authorization has not
been established. No local-only cover override was requested.

Reference: python-tidal commit `cce10953477362c4383731ad6a8f4ef6296a6168`,
`tidalapi/user.py` and `tidalapi/playlist.py`. Reference code is not itself proof
of current service behavior—the nested-creation result demonstrates why.

## Explicitly opted-in checks

Normal `cargo test --locked` does not run live probes. Never run all ignored tests.
These commands require fresh operator authorization for that run, not just release
approval. The account must have a sufficiently fresh saved session; the probe
refuses to renew credentials alongside the running player.

Read-only contract check:

```sh
TIDAL_TEST_FOLDERS=1 cargo test --locked live_folder_read_contract -- --ignored --test-threads=1
```

Temporary-object mutation round trip:

```sh
TIDAL_TEST_FOLDERS=1 cargo test --locked live_temporary_folder_round_trip -- --ignored --test-threads=1 --nocapture
```

The mutation probe records a private receipt before proceeding, captures returned
identities, and attempts bounded cleanup of known objects. An uncertain creation
may have no returned identity. On failure, inspect the private receipt and verify
residual objects before considering another run; do not blindly repeat creation
or cascading deletion. Cleanup failure is a blocker, not a successful test.
