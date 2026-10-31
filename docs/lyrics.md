# TIDAL lyrics and Now Playing

The player-bar artwork, its overflow action, or **L** opens a dedicated full-window
surface. Artwork remains left and lyrics right; the browsing shell and Queue/History
panel are hidden, not replaced. **Back / Esc** restores the retained browsing state.
Explicit navigation (artist, album, output settings, or Queue) leaves this surface.
This is not OS-level fullscreen and does not change the window geometry.

Lyrics use the existing authorized `GET /v1/tracks/{id}/lyrics` compatibility route.
Reference: python-tidal `9c41fbe6b2f2cd9fa00dca11e83574fd929ec020`, `media.py`.
That reference is not proof that every sign-in or track has lyrics access. Missing,
denied, throttled and failed responses have local, honest states and never stop audio.
No live-account capability claim is made from the synthetic tests.

## Privacy and bounds

- TIDAL only; no LRCLIB or other provider lookup. HTTP redirects are disabled.
- Read-only token snapshots; background lyrics never independently refresh or persist
  credentials. Expired snapshots ask the user to refresh through normal playback.
- One request permit, cancellation on view close/replacement, an eight-second request
  deadline, 512 KiB response cap, 2,048 lines and 4,096 bytes per line.
- Results require the same account, track and request serial. Only the current document
  is retained in memory, not written into metadata caches or history.
- Provider attribution and direction are retained. Bodies, tokens and raw HTTP errors
  are not logged or used as playback errors.

## Timing and rendering

Supported LRC timing is parsed defensively, including bounded signed offsets and
multiple timestamps. Stable ordering preserves duplicate timestamps. Invalid or
mixed timing falls back to supplied plain text without inventing seek targets.
Timed rows seek through the same subsecond player action as MPRIS and CLI.

Follow tracks the confirmed source position; bounded UI extrapolation is not listening
credit. Manual scrolling suspends follow; **Follow** restores it. A new occurrence
resets follow, while identical-track metadata can be reused. Rows are virtualized;
long lines use a bounded two-line layout without hover tooltips.

**Language limitation:** the current renderer does not provide verified bidirectional
shaping. RTL lyrics, or lyrics containing glyphs absent from the bundled fonts, show an
explicit display-unavailable state instead of misleading boxes or reversed text.
Provider text/direction remain intact in memory. Native RTL fixture checks established
this limitation; this release does not claim complete language support.
