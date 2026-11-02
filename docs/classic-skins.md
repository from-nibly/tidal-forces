# Optional classic mini-player skins (P6)

The branded native mini player remains the default. Settings → Appearance →
**Optional classic mini-player skin** imports a user-provided, absolute local `.wsz`
path. Importing selects the new appearance but does not open a player or start audio;
**Open mini player** opens the existing shared-state mini view. **Branded** switches
back without stopping playback. **Load saved skin** restores the private local copy.
Use only artwork you are entitled to use. No artwork is downloaded, uploaded or
bundled from third parties.

## Compatibility, not a Winamp emulator

Supported: classic 275 × 116 main-panel artwork with `main.bmp` and `cbuttons.bmp`.
Optional `titlebar.bmp`, `posbar.bmp`, `volume.bmp`, `shufrep.bmp` and `playpaus.bmp`
provide title, seek, volume, shuffle/repeat and status sprites. Entry names are
case-insensitive; a containing folder is allowed. Missing optional sprites use native
fallback controls. Original synthetic debug/test artwork is not a released default.

Text is native, not bitmap-font emulation; the title is clipped rather than scrolled.
The native header retains main-player, branded-mode and mini-close controls. The
source-quality/status line stays **outside** the artwork. The legacy bitrate/channel
area is covered rather than inventing a bitrate or output-device claim.

Transport, volume and seek dispatch the same actions as the main UI/MPRIS/CLI.
**Stop clears the app's queue**, matching its existing Stop action, not every legacy
Winamp behavior. Eject opens the main player; EQ opens native audio settings; PL opens
the shared queue. Shuffle/repeat use the authoritative occurrence-aware queue. The
classic balance area directs users to native DSP settings instead of adding a second
processor. Seek drags are scoped to the playback generation and cannot carry across
source changes. Closing the mini view never quits the main app or stops playback.

Modern `.wal` skins, scripts, plugins, custom cursors, bitmap fonts, legacy shade
modes and separate skinned EQ/playlist windows are not supported. Unrecognized files
are ignored, not executed or extracted. The native window manager still owns window
placement and supported always-on-top behavior; Wayland limitations are unchanged.
Classic pixel art has small hit targets and arbitrary contrast: keyboard focus,
arrow/Home/End slider control, scaling and the branded alternative are available,
but this is not an accessibility certification of arbitrary third-party artwork.

## Safety and persistence

- One background loader at a time. Failed imports keep the currently displayed skin.
  Choosing Branded while an import finishes prevents late reactivation.
- Local regular files only; Unix opens use no-follow/nonblocking flags and descriptor
  checks. The file is read with an 8 MiB cap. No skin paths are extracted.
- At most 128 archive entries, 256-byte safe names, 4 MiB per entry and 16 MiB total
  declared expansion. ZIP links, encryption, unsafe names, duplicate names/assets,
  inconsistent local/central headers and unsupported compression are rejected.
- Only stored/deflate ZIP and recognized BMP assets are decoded. Recognized image
  payloads must satisfy CRC/size checks. Bitmap dimensions are capped at 1,024 × 1,024,
  decoder allocation and aggregate decoded pixels at 8 MiB. Required sprite bounds
  are checked; only the used rectangles are uploaded to the GPU, with nearest filtering.
- A validated archive is atomically copied to `classic-skin.wsz` in the app's private
  config directory (directory 0700/file 0600 on Unix). This is one local slot, not a
  collection or remote sync service. Switching to Branded retains it for reuse.
  No credentials, account state or audio routing settings are migrated.

The exact zip version is pinned because its bounded iterable-reader API is upstream
unstable. License notices ship through `--licenses`; see `THIRD_PARTY.md`. Format
coordinates were checked against
[Webamp's classic sprite layout](https://github.com/captbaritone/webamp/blob/master/packages/webamp/js/skinSprites.ts);
no Webamp source or artwork is incorporated.

## Private validation

Normal tests cover parsing limits, traversal, duplicates, encryption/link/header
rejection, CRC/truncation, invalid BMP sizes, private atomic persistence, native
layout/quality visibility and shared playback/keyboard/generation guards.

`python3 tests/skin-import.py` builds the test executable **before** replacing HOME,
then runs only `classic_skin_import_on_private_profile` in a fresh private profile,
with unreachable buses/output and an inert player. The opt-in test refuses a nonprivate
profile and checks real import/reload/preference persistence without a real account,
credentials, device or running preview. Never run all ignored tests.

Debug `--visual-fixture` supports `mini-classic` using original synthetic artwork.
Native Xvfb fixtures are not proof of every third-party skin or platform behavior.
Full version verification and native operator review remain required before release.
