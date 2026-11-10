# P7a TIDAL receiver handoff: bounded public-source investigation

Subsequent development and separately approved hardware attempts are recorded in
[inline-DASH validation](cast-inline-dash-validation.md). The final synthetic AAC-LC
test played audibly and cleaned up its app without a server/relay. Earlier control
read/cleanup failures remain unresolved, as does TIDAL account-media compatibility.

## Result

**No public, supported TIDAL-specific Cast receiver handoff was established.**
The audit did not find a documented receiver app/namespace, receiver-scoped token
exchange, launch payload and playback contract equivalent to Spotifast's Spotify
handoff. This is a bounded negative finding, **not proof that no private or partner
integration exists**.

There is a concrete additional no-server candidate: the official Web SDK wraps
inline DASH in a `data:` URL for its own player. At the time of this source audit, Cast acceptance was untested; the subsequent
public-fixture experiment linked above now proves that limited case. Inline DASH is therefore
not, by itself, proof that a local HTTP endpoint is mandatory.

This investigation used only public documentation and source downloads. No receiver
contacts, account/credential reads, playback, listener, relay, copied credentials,
package installation into the project, or player changes occurred.

## Official sources

### Public Developer Platform documentation

The current [API/SDK overview](https://developer.tidal.com/documentation/api-sdk/api-sdk-overview)
states that the SDK also contains capabilities used internally by TIDAL which are
not exposed to external developers. It specifically describes the official,
unmodified Player module as the allowed third-party playback route and mentions
previews. The [developer guidelines](https://developer.tidal.com/documentation/guidelines/guidelines-developer-guidelines)
also restrict Developer Platform playback to that module.

Consequently, an account playback-information request succeeding is not proof of
an officially supported third-party Cast integration. A guessed first-party receiver
ID or copied token payload would not resolve that authorization question. This is
an observation about the published Developer Platform contract, not a legal ruling
about every personal compatibility client.

The [Connect page](https://developer.tidal.com/documentation/connect) says TIDAL
Connect integrations are currently for device partners. **That statement concerns
TIDAL Connect, not Google Cast; these must not be conflated.** It does not establish
a Cast sender contract for this project.

The portal is a public client-rendered site. Its public documentation chunks were
retrieved anonymously rather than opening a signed-in browser. Relevant snapshots:
`overview-DXPrxxag.js`, `authorization-B4slYSfo.js`, `connect-BbVarDPj.js`, and the
current version-3 developer guidelines/terms chunks. Exact downloaded hashes are
recorded with the private research artifacts.

### SDKs and specification

Reviewed sources:

- [Web SDK at d9d0618](https://github.com/tidal-music/tidal-sdk-web/tree/d9d0618c5efa6c97d68e58d638698580de46246e):
  available textual source and player API/implementation.
- [Android SDK at 4f88e3d](https://github.com/tidal-music/tidal-sdk-android/tree/4f88e3df5c6f7203074543c1db8cb8fcaa18b921):
  repository tree, player README, public player classes and Gradle dependencies.
  The full archive exceeded the download cap; this was **not** a full Android audit.
- [iOS SDK at 412683f](https://github.com/tidal-music/tidal-sdk-ios/tree/412683f95064faf3e2b2e4939c9aabd7fa31aa09):
  available textual source and player documentation, which describes AVPlayer.
- [Shared Player specification at dd6bf9a](https://github.com/tidal-music/tidal-sdk/blob/dd6bf9a73c0dd6553309c045221fc80e4572a9c0/Player.md).

The Android player documentation describes ExoPlayer. Web uses its browser/native/
Shaka integrations. None of the inspected public API surfaces established the
receiver-scoped TIDAL authentication/handoff we were seeking. SDK functionality and
first-party app functionality must not be assumed identical.

The important inline-DASH detail is in the Web SDK's
[manifest parser, lines 338 onward](https://github.com/tidal-music/tidal-sdk-web/blob/d9d0618c5efa6c97d68e58d638698580de46246e/packages/player/src/internal/helpers/manifest-parser.ts#L338-L356):
its DASH branch forms `data:application/dash+xml;base64,...` from the supplied
manifest, then passes stream information to its own player stack. This proves how
that SDK consumes inline DASH locally. It is **not** a documented Cast receiver
API and does not prove that the Default Media Receiver accepts a `data:` content URL.
It also does not replace the need to validate absolute segment URLs, CORS, codec
support, message limits, URL expiry and account/receiver ownership.

## Public third-party implementations

### Music Assistant: serves an MPD and supplies its own output stream

At [c2501db](https://github.com/music-assistant/server/tree/c2501db1f6197189d981baf367e43c902569fc20),
the [TIDAL streaming provider](https://github.com/music-assistant/server/blob/c2501db1f6197189d981baf367e43c902569fc20/music_assistant/providers/tidal/streaming.py#L81-L135)
recognizes the same inline DASH response. It decodes the MPD and registers an
HTTP manifest route. Its comments explain that this supports FFmpeg manifest
re-fetches. That rationale is specific to its implementation; it does not establish
that `data:` URLs are universally single-use.

The [Chromecast player](https://github.com/music-assistant/server/blob/c2501db1f6197189d981baf367e43c902569fc20/music_assistant/providers/chromecast/player.py#L251-L268)
uses Music Assistant's stream URL resolver, not a TIDAL receiver credential handoff.
The repository also has a separate Sendspin Cast receiver bridge whose JS client
connects back to the server by WebSocket. Neither path establishes a reusable
TIDAL-specific Cast authentication contract.

This is evidence that local serving is an established engineering approach to this
manifest problem, **not evidence that manifest-only direct-CDN casting already
works on our receivers**, nor an endorsement of copying its access-control policy.

### Tideway: PCM-to-FLAC local streaming

At [8dd4009](https://github.com/J-M-PUNK/tideway/tree/8dd4009bba8401b181ae34330995b78382605947),
[`app/audio/cast.py`](https://github.com/J-M-PUNK/tideway/blob/8dd4009bba8401b181ae34330995b78382605947/app/audio/cast.py#L1-L24)
explicitly describes a PCM tap, FLAC encoder, ring buffer and LAN HTTP server. It
loads that stream into the Default Media Receiver with PyChromecast. It is not
using TIDAL's own receiver to fetch the service track.

That is a substantially larger audio-forwarding design than a manifest-only endpoint.
Its callback, encoding and lifecycle choices must not be copied into this player's
real-time audio path without independent design and validation.

### Corroborating sources and limits

- [PyChromecast at 08aeef0](https://github.com/home-assistant-libs/pychromecast/tree/08aeef04e0b65d813d141a936fd1db6b8241d570/pychromecast/controllers)
  has generic and several service-specific controllers, but no TIDAL controller
  in the inspected snapshot. Absence there is not proof of impossibility.
- [Tune at 673b640](https://github.com/renesenses/tune-server-rust/tree/673b640e4d4d1dd145ac98aadae1934f42ffcaf5)
  uses the Default Media Receiver and server-side TIDAL DASH segment handling;
  it does not supply the missing TIDAL receiver handoff. Its source-available
  licensing and transport policies were not adopted.
- Anonymous requests to the first-party web player and user-facing Cast support/
  partner pages returned HTTP errors. The first-party Cast payload was therefore
  **not inspected**. No login or access-control workaround was attempted.
- Public repository/issue searches were bounded, not exhaustive. SDK source and
  documentation can lag private integrations. No claim of vendor endorsement or
  complete security review follows from this audit.

## Recommended next gate

1. Keep the TIDAL-specific receiver path unproven. Use TIDAL's developer-support
   channel to clarify the supported full-track Cast integration/eligibility if an
   officially supported handoff is required. No message has been sent on the
   operator's behalf.
2. Before adding a LAN server, prepare a **synthetic DASH `data:`-URL proof**. Private
   tests should bound the encoded LOAD message and preserve all trust, ownership,
   volume and cleanup guards. A real-device experiment needs new explicit scope;
   use only public synthetic segments, no account media or credentials. Failure
   should not trigger a listener or different delivery mode automatically.
3. If that representation is unsupported, review a **manifest-only endpoint** next.
   The receiver could potentially fetch audio segments directly from their HTTPS
   CDN, without decoding/transcoding or relaying every audio byte. CORS, HTTP/TLS,
   access control and real receiver compatibility remain gates, not assumptions.
4. Full audio serving/remuxing/transcoding is a later fallback, not the default
   consequence of receiving inline DASH. Any listener/relay needs separate approval.

No implementation or live permission is implied by this recommendation. The
current-track HDMI test remains unperformed; neither the native Devices UI nor a
P7 release/install is cleared by these source findings.
