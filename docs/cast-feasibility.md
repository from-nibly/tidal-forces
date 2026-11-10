# P7a Cast feasibility — initial source audit

**Status: in progress, not a passed Cast capability gate.** Implementation/private
research is authorized. The operator separately approved bounded read-only inspection
of the two saved targets, then explicitly authorized an authentication-only connection
to one Chromecast Audio, then one explicit legacy-device-certificate retry. That
retry authenticated successfully with trusted path, fresh SHA-256 binding and
mandatory signed revocation. Default strict policy still rejects its legacy chain.
The subsequently approved [bounded synthetic tone test](cast-tone-validation.md)
confirmed audible playback at the operator-requested 100% level, followed by test-app
stop and prior-volume restoration. No LAN sweep, session takeover, account media
or relay was used. Further live test scope remains separately gated.
Installed v0.33 and its review preview are unchanged.
Development branch: `feat/p7-cast-feasibility`, package `0.34.0-dev`.

A subsequently selected HDMI receiver passed **strict** authentication. The approved
current-track preflight and alternate HIGH/AAC lookup both returned inline DASH,
not a receiver-fetchable manifest URL. No account-media playback or relay occurred;
see [current-track validation and remaining delivery decision](cast-current-track-validation.md).

The subsequent [public TIDAL receiver investigation](cast-tidal-receiver-research.md)
did not establish a supported service-specific Cast handoff. It did find the Web
SDK's inline-DASH `data:`-URL representation. Subsequently,
[explicitly approved synthetic attempts](cast-inline-dash-validation.md) culminated
in **operator-confirmed audible AAC-LC DASH playback without a local server or audio
relay**, with owned-app stop and receiver idle confirmed. The successful attempt
began at 100% volume and preserved that level; the operator was told explicitly.
Earlier launch/status timeouts and unconfirmed cleanup remain a reliability issue,
not erased by one success. These permissions are consumed. This proves the public
fixture path only; no account-media attempt, listener or production Cast capability
was cleared by that synthetic approval.

Later, separately approved [current-account music tests](cast-music-validation.md)
established operator-confirmed whole-song FLAC/44.1 kHz playback via a data-URL MPD
and direct CDN fetches, without a server/relay. Owned-app stop/idle and starting
volume preservation passed. Those whole-run final-state assertions failed, but a
subsequent private reproduction found discarded unsolicited completion events.
The corrected collector then verified natural completion of an approved 7.988-second
FLAC excerpt, including owned cleanup. The operator permits needed short repeats
under that same bounded scope at that point. Later date-scoped testing verified
[native worker controls and seeking](cast-native-validation.md) for actual TIDAL
FLAC, AAC-LC and HE-AAC at44.1kHz. Target integration is wired in development;
native preview review and broader production reliability remain open. Historical
approvals are not standing live-test authority.

## Findings

### 1. A generic receiver route exists; service-specific integration is separate

Google documents its Default Media Web Receiver as a Google-hosted audio/video
receiver requiring no developer-console registration. The sender launches it and
loads a media URL. This is a candidate for permitted fixtures and compatible,
authorized direct media—not evidence that the official TIDAL receiver accepts this
client, or that TIDAL media may be handed to any receiver without checking the
applicable authorization contract.

Google lists FLAC up to 96 kHz/24-bit, HE-AAC and LC-AAC for Chromecast Audio, Google
Home and Home Mini. That is documentation, not a measurement of the operator's
receiver or evidence for the unspecified regular Chromecast. Container, CORS,
range, URL expiry and firmware behavior must still be tested. Never use a public
CORS proxy for account media or signed URLs, even though the general Google media
guide mentions proxies as a debugging aid.

### 2. The current native stream model has two different delivery cases

`src/api.rs::parse_stream` accepts HTTPS direct URLs only for clear BTS manifests
(`encryptionType = NONE`), or parses inline DASH XML into a local manifest.

- **Direct:** a candidate receiver-fetch URL exists. Do not fetch live account
  manifests just for this audit. Receiver reachability, permitted use, content type,
  ranges/CORS and expiry remain unknown.
- **Inline DASH:** the app currently has XML/segments, not a receiver-fetchable MPD
  URL. Do not pass a segment as a whole track, treat a local filename as remotely
  reachable, or start a relay silently. Establish a supported direct delivery
  option first; otherwise return to the separately approved relay design gate.
- **DRM/encryption:** remains unsupported. No conversion, decryption workaround,
  borrowed credentials or subscription bypass is proposed.

### 3. Do not adopt `rust_cast` 0.21.0 unchanged

The MIT-licensed crate is a useful protocol/API reference, but the inspected source
is not an acceptable production transport for this player:

- `CastDevice::connect` uses platform roots and host verification; its alternate
  `connect_without_host_verification` accepts an unverified server certificate.
  Neither connection setup performs Cast's device-auth challenge verification.
  Generated authentication message types alone do not establish authentication.
- `MessageManager::read` allocates capacity from the untrusted 32-bit frame length
  without an application frame limit; it then reads to the declared limit and
  duplicates the buffer. That is not our bounded-parser contract.
- `receive_find_map` keeps unrelated messages in a vector without a bound and waits
  in a loop without an overall deadline. The public connection setup uses blocking
  `TcpStream::connect` without configured read/write timeouts.
- Debug logging includes raw messages and peer addresses; TLS key logging follows
  the environment. Neither is appropriate for secret-bearing playback sessions.

No `rust_cast` dependency has been added. A wrapper around this public API alone
would not fix the private transport internals. The subsequent offline proof uses
dev-only dependencies and attributed schema-derived wire definitions, not this
crate's transport; see [native authentication proof](cast-authentication.md).

Reviewed crate archive SHA256:
`235d09067a35143d271fdeaf28298b4f729b5da50abdbf3b9e6395262d8bc7a9`.
The archive was checked against crates.io metadata before source inspection.

### 4. Cast authentication is not ordinary public-Web-PKI hostname trust

Chromium Open Screen provides a reference for the missing trust operation:

- Parse a binary device-auth response and reject malformed/error/missing responses.
- Check the TLS peer certificate's validity and bounded remaining lifetime.
- Match the challenge nonce; verify the device certificate chain to Cast trust roots.
- Verify the signature binding the nonce to that exact TLS peer certificate.
- Apply certificate policy restrictions, including audio-only policy, and make
  revocation/digest policy explicit rather than silently weakening validation.

This permits a protocol-authenticated channel; it does **not** justify sending media
URLs over a generally certificate-unverified connection. No application control,
account data or media URL may leave a quarantined channel before authentication.
The inspected Open Screen API has optional-CRL and legacy-digest behavior; copying
its defaults without deciding and testing our policy would not close this gate.

A native offline authentication core is now implemented using rustls-webpki/ring,
prost and x509-parser, with independent synthetic signed fixtures and adversarial
tests. It enforces SHA-256, TLS-certificate/nonce binding, certificate paths, mandatory
signed revocation and inherited audio policy. See [proof scope and compatibility
limits](cast-authentication.md). The subsequent bounded TLS probe uses reviewed,
fingerprint-pinned public Cast roots. Hardware authentication passed in one explicitly
approved legacy-device-certificate retry. A separately approved public FLAC tone
also played audibly; account-media delivery remains an open gate.

## Private checks and authorized identity inspection

- `examples/cast_frame_probe.rs` is an offline Rust framing experiment, not a Cast
  sender. Seven tests pass for preallocation limits, fragmented input, every truncated
  prefix, adjacent frames, invalid outbound size, deadline propagation and partial
  write failure. Its 64 KiB cap is our provisional policy, not a measured receiver
  limit. No sockets, new dependencies or app controls were introduced.
- The v0.34-dev baseline retains 444 passing normal application tests; formatting,
  strict all-target Clippy and the seven explicit example tests pass. These checks
  do not prove receiver authentication, protobuf semantics or hardware playback.
- With explicit operator permission, each saved target received at most two bounded,
  read-only HTTP GETs on port 8008: setup metadata and device description. Redirects
  and environment proxies were disabled; response size and total request time were
  bounded. No Cast control socket, LAN broadcast or account request was made.
- The designated Chromecast Audio reports Cast firmware `1.56.467166`; its device
  description path returned 404. The designated regular Chromecast reports
  `1.56.291998` and the generic model `Eureka Dongle`. Exact generation remains unknown.
- These are unauthenticated self-reports, not cryptographically verified identity.
  Addresses and the minimized raw report remain private, outside tracked files.
  No Wi-Fi names, device friendly names, identifiers or setup public keys were retained.

## Next work and acceptance

1. Exact model/generation still needs confirmation. Read-only inspection succeeded
   only as described above; it does not authorize broader discovery/control testing.
2. Private cryptographic/framing proofs and a quarantined TLS/auth-only transport
   with reviewed public roots, whole-operation IO limits and cancellation are now
   implemented. The scoped real-TLS loopback contract also passes.
3. Initial authentication failed closed on a legacy RSA/SHA-1 certificate chain.
   After source/private-test review, the operator approved one narrowly scoped
   legacy-device-certificate retry. It authenticated successfully while retaining
   strict fresh SHA-256 authentication and mandatory signed revocation. No app
   status/control command was sent, and the other receiver was not contacted.
   That authorization is consumed; further receiver/media tests need fresh scope.
4. Only in an approved safe-volume test window: demonstrate actual fixture playback,
   then permitted service-media delivery. Record codec/container/range/expiry evidence.
5. If authorized delivery or receiver trust cannot be established, stop product UI
   work and report the exact blocker. A private test pass is not a hardware pass.

## Sources

- [Google receiver overview](https://developers.google.com/cast/docs/web_receiver),
  Default Media Web Receiver section.
- [Google supported media](https://developers.google.com/cast/docs/media), audio
  codecs, container support and adaptive-streaming/CORS sections.
- [`rust_cast` 0.21.0](https://docs.rs/crate/rust_cast/0.21.0/source/), especially
  `src/lib.rs` and `src/message_manager.rs`; source license MIT.
- [Open Screen authentication reference](https://chromium.googlesource.com/openscreen/+/784bb6902132df45fad06638d4c88c273e6eed9f/cast/sender/channel/cast_auth_util.cc)
  and [certificate policy](https://chromium.googlesource.com/openscreen/+/784bb6902132df45fad06638d4c88c273e6eed9f/cast/common/certificate/cast_cert_validator.cc),
  BSD-style source. Schema-derived wire definitions retain the upstream license;
  no upstream verifier implementation is bundled. Public trust certificates are
  pinned only in the isolated dev example, with separate provenance documentation.
- Local `src/api.rs`, `src/dash.rs`, `src/actions.rs`, `src/ui/actions.rs` at the
  v0.33 release baseline `c83b8c2554416044824afd08367c10fdcd50c119`.
