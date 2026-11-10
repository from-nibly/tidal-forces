# Synthetic Cast authentication fixtures

These are original, locally generated **public certificates, signatures and protocol
records**, not captured receiver traffic or account data. There are no Google Cast
private keys, production roots, TIDAL credentials, media URLs, device addresses or
private keys here. Public certificate subjects explicitly identify synthetic fixtures.

`generate.py` uses the local OpenSSL CLI to create independent RSA signatures and
X.509 chains; the Rust proof verifies them with rustls-webpki/ring. Temporary private
keys live only in the generator's private temporary directory and are removed on
exit. The generator makes no network requests. OpenSSL is needed for manual
fixture regeneration and the explicit `tests/cast-auth.py` loopback runner, not for
normal offline Rust tests or application/probe runtime. The runner uses `--out`
and `--transport-keys` to keep fresh synthetic TLS/device signing keys outside the
repository for the test's lifetime, then removes its entire private directory.
The generator refuses to export keys into the repository.

`clock.txt` fixes the verification instant so tests are independent of wall time.
`SHA256.json` records fixture hashes. Regeneration changes keys, certificates,
signatures and the clock; review the complete diff. Do not automatically regenerate
fixtures in CI and then claim they are unchanged reference inputs.

```zsh
python3 tests/fixtures/cast-auth/generate.py
cargo test --locked --example cast_auth_probe
```

- `root`, `intermediate`, `device`: a trusted synthetic Cast-style chain.
- `audio-*`: audio-only certificate policy on leaf, intermediate or root.
- `crl-root`, `crl-signer`: separate synthetic revocation authority.
- `untrusted-*`: unrelated authority, never included in successful test trust stores.
- `tls*`: synthetic self-signed TLS peer certificates bound by device signatures.
- `short-*`, `weak-device`, `wrong-usage-device`, `critical-device`: expiry, key size,
  digital-signature usage and unknown critical-extension failures.
- `.reply`: protobuf Cast envelope/device-auth responses with fixed fixture nonces.
- `.crl`: signed version-0 revocation bundles, including validly signed negative
  cases for revoked SPKI hashes, serial ranges, time bounds and malformed policy.

## Protocol provenance

The wire field definitions in `examples/cast_auth/proto.rs` and this generator
follow Chromium Open Screen at commit
`784bb6902132df45fad06638d4c88c273e6eed9f`:

- `cast/common/channel/proto/cast_channel.proto`
- `cast/common/certificate/proto/revocation.proto`

Copyright 2019 The Chromium Authors. The accompanying `OPENSCREEN-LICENSE` preserves
the BSD notice and terms for those schema-derived definitions. The cryptographic
verification implementation is original Rust using maintained library APIs, not a
copy of Open Screen's C++ verifier. Policy differences are documented in
`docs/cast-authentication.md`.
