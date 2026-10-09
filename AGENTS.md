# Tidal Forces working agreements

## Architecture and safety

- Keep this a native Rust/egui player with native decoding and one executable.
- Preserve occurrence-aware queue identity, media keys/MPRIS, explicit audio-quality
  reporting, and account/revision guards. Never bypass subscription or DRM controls.
- Never expose credentials, signed media URLs, private account data or local device
  addresses in source, logs, screenshots or releases.
- Real account mutations, credential migration/cleanup and audible/device tests need
  separate explicit opt-in. Version approval does not authorize those tests.
- Do not run all ignored tests: the live playlist round trip mutates an account.
  The Secret Service contract must use its documented isolated D-Bus invocation.
- Keep the running review preview unchanged during development. Check playback and
  obtain approval for replacement; quit gracefully, never force-close unsaved sign-in.

## Review and releases

The operator's standing release policy is:

1. Implement the next version and verify it locally.
2. Present the working native preview for review. Track each feedback item in its
   own task and resolve it before advancing the phase.
3. Wait for the operator's approval of that version.
4. Once approved, publish **and install** that version; this is the standing
   authorization for both steps, not for subsequent unapproved versions.

Master pushes automatically publish GitHub releases. Do not push to master before
version approval. Use a development branch for the next unapproved version, keep
its package version marked `-dev`, and add versioned release notes before publishing.

Deploy the verified GitHub artifact/checksum through the configured Home Manager
package pin, then switch Home Manager. Read that repository's AGENTS.md first and
preserve unrelated work. Never use the application's `--install` or replace a
Home Manager-managed executable directly. Do not roll a migrated credential profile
back to a player version that cannot read its keyring metadata.

`docs/feature-plan.md` is the phase/capability ledger. P3 library/playlist power tools
follow the approved v0.8 foundation and visual pass; later audio/lyrics/mini-player,
Cast and appearance work retain their documented capability and validation gates.
