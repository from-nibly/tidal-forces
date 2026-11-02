# Appearance (v0.33)

Dark remains the default. Settings → Appearance adds Light, System and Custom.
System follows the theme reported by the native backend, falling back to Dark when
unavailable. It does not change the desktop's theme. Main, mini, Now Playing,
menus and error surfaces share semantic colors; theme changes do not issue any
playback, queue or account actions. Typography and geometry are shared across modes.

## Custom palettes

Expand **Custom palette**, start from Dark or Light, edit/paste the RGB-array JSON,
and choose **Apply custom palette**. **Copy current palette** exports the resolved
current colors. No file or network access is needed. Failed validation leaves the
active appearance unchanged and displays an error.

The complete schema has eleven required RGB arrays (each three integers, 0–255):
`bg`, `panel`, `card`, `border`, `muted`, `accent`, `playing`, `selected`, `text`,
`hover` and `error`. Unknown fields and input over 4 KiB are rejected. Text, muted,
accent and error colors require at least WCAG 4.5:1 contrast against every surface.
Accent-button labels choose black or white for contrast. This is a solid-surface
contrast check, not a blanket accessibility certification of arbitrary artwork.

Preferences live alongside existing local appearance settings, not credentials.
Malformed cosmetic fields fall back independently without resetting output, density
or zoom preferences. Theme controls do not enable playback, tray or recommendations.

## Validation and limits

Normal tests cover built-in/custom contrast, bounded parsing, system-mode resolution,
cosmetic fallback and switching across main/mini/lyrics/error views without changing
queue revision, playback generation, pause intent or volume. Isolated Xvfb fixtures
cover dark/light/custom at minimum, normal and zoomed sizes with synthetic metadata.
No real account, output device or running preview is touched by those fixtures.

English remains the only shipped language by operator choice; see
[localization.md](localization.md) for catalog coverage and
[classic-skins.md](classic-skins.md) for optional bounded classic mini-player skins. Native operator review, performance
checks, license review and version approval are required before publishing v0.33.
