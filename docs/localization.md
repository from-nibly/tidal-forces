# English catalog support (P6)

English is the only shipped interface language, by operator choice. There is no
nonfunctional language picker, external catalog download or machine-translated copy.
Settings reports the current language. Provider titles, artists, playlist names and
lyrics are not translated.

`assets/i18n/en.json` registers source-English message IDs and their English values.
`src/i18n.rs::tr` resolves these messages once from the embedded catalog and falls
back exactly to the supplied English source when a key is absent. The current
foundation covers 353 static UI messages across the main, settings, mini, lyrics,
library, queue and guarded-edit surfaces. It deliberately does not claim a complete
non-English interface: dynamic formatted diagnostics, some enum/transport labels,
CLI and native tray text still require coverage before another language ships.

## Maintaining the catalog

- Wrap **interface literals** with `tr`, and register their exact source/value in the
  English catalog. English values must equal their keys. Preserve punctuation.
- Do not translate widget ID salts, persistence keys, CLI arguments, endpoint paths,
  media identifiers, account values, provider metadata or arbitrary user input.
- The JSON shape is `locale` plus a `messages` object. Catalogs are capped at 512 KiB
  and 1,024 entries; keys at 1,024 bytes and values at 4,096 bytes. Unknown fields,
  blank entries, unsupported control/bidi-override characters, malformed braces and
  mismatched placeholder names/counts are rejected. Double braces represent literals.
  Catalog text is plain text, never executable code or HTML.
- This parser is infrastructure, not an external-file import feature. Only the
  embedded English catalog is activated. No preference or account migration occurs.
- Tests validate all bundled entries, registration of direct literal UI calls,
  fallback, limits, placeholder retention and unchanged English text. Existing UI
  state and layout tests remain applicable.

A future language needs reviewed translations, completion of dynamic-message
coverage, an explicit selection/fallback policy, fonts/shaping support, native
minimum-size/zoom/layout/accessibility checks and appropriate translator licensing.
The existing limitations on RTL/provider lyric glyphs are not solved by this catalog.
