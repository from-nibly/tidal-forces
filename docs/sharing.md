# Copy TIDAL links (v0.34.2)

Copy catalog links directly, without fetching or exposing media sources.

- **Track menus:** choose **Copy TIDAL link** from a track row, queued track, the
  playing-track menu, or the Now Playing artwork context menu. **Copy album link**
  and **Copy artist link** are also available when the track provides those IDs.
- **Now Playing:** a visible **Copy TIDAL link** button copies the current song.
- **Branded mini player:** the options menu includes the current track's link.
- **Albums, artists, playlists and mixes:** use the header's **Copy TIDAL link**
  button. Album/artist cover context menus, playlist folder menus and pinned
  playlist menus also expose copying.
- **Multiple tracks:** existing **Copy links** and table-focused Ctrl+C continue
  to copy the selected occurrences in order, preserving duplicates. Text fields
  keep their normal clipboard shortcuts.

Links use canonical `https://tidal.com/browse/...` catalog URLs, without tracking
parameters, credentials or signed audio URLs. Missing/invalid IDs disable the action.
Copying does not contact TIDAL, change playback/navigation/queue, modify favorites,
or change sharing permissions. A copied private playlist remains private; access
and availability are still determined by TIDAL and the recipient's account.
