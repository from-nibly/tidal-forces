# Cast player controls (v0.34)

Version0.34 is approved for release. See the [release notes](releases/v0.34.0.md)
for verification scope and remaining live validation limits.

## Choose where music plays

Use the **Cast button on the player** to open Playback devices. The same control is
available in the main player, branded mini player and classic mini player.

- **This computer** selects local playback.
- Nearby Cast receivers appear by their friendly name and model. No address or
  certificate setting is required. Discovery updates while the picker is open and
  stops when it closes. Escape closes the picker without changing playback.
- Choosing a receiver transfers the current position and playing/paused intent.
  With no current track, it selects the target for your next Play action.
- The icon animates while connecting/stopping and fills only after the owned media
  session is confirmed. Selecting a target alone is not shown as a connection.
- **Stop casting · return here paused** releases the receiver before enabling local
  output. It does not automatically press Play on this computer.

If stopping cannot be confirmed, replacement playback remains blocked. Stop playback
on the receiver, then use the picker's explicit check. No receiver session is taken
from another app, and there is no automatic reconnect or local playback fallback.

## One queue and one set of controls

Cast uses your existing queue, including manual additions, duplicate occurrences,
shuffle and repeat. The player, media keys, CLI and mini/classic controls operate on
that same queue. Natural completion starts the next queued occurrence; reaching the
end stops playback. Pause and seek preserve the selected target. Returning locally
preserves the current position and remains paused.

Artwork, title, artist and album are supplied as standard Cast music metadata for
each track. Artwork uses public HTTPS TIDAL image resources, not a local server.
A receiver without a screen cannot display it; actual screen rendering still needs
the separate live validation gate below.

## Capability boundaries

Only reviewed clear DASH sources up to fifteen minutes per track are currently
accepted. The requested quality must match; unsupported sources stop with an error,
not a silent downgrade. Receiver output format is unknown. Local DSP and visualizer
processing do not apply to remote audio. Genuine TIDAL Connect is separate.

The private suite verifies the picker and shared queue/metadata pipeline. Earlier
approved device tests verified actual TIDAL FLAC, AAC-LC and HE-AAC pause/seeking,
and discovery now finds five devices after the narrow host firewall correction.
Those observations do not prove playback on every discovered device or replace the
remaining updated-preview, receiver artwork and multi-track audible validation.
See [native validation](cast-native-validation.md) and
[discovery investigation](cast-discovery-research.md).
