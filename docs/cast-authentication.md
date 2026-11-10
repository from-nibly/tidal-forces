# P7a native Cast authentication proof

**Current development integration:** this reviewed boundary is now shared by the
native Cast worker. The operator approved built-in reviewed legacy-chain compatibility
for the cohesive player feature, with no certificate UI. Strict-only policy remains
the isolated probe default. No roots, freshSHA256 authentication, validity/key/path
constraints or revocation requirements were removed. See the
[current policy](cast-legacy-compatibility.md#current-product-policy-supersedes-the-historical-opt-in-scope-below)
and [native implementation](cast-native-validation.md). The sections below describe
the original isolated probe and its historical test permissions, not new live scope.

**The operator-selected Chromecast Audio authenticated successfully in one explicitly
approved legacy-device-certificate retry.** Its trusted path, fresh SHA-256 nonce/TLS
binding, mandatory signed revocation proof and audio-only restriction passed. The
authentication connection then closed without application commands or volume changes.
A subsequent, separately approved [synthetic tone test](cast-tone-validation.md)
confirmed audible playback; that is a distinct explicitly opted-in probe mode.

This is an isolated native Rust example/test target, not an enabled Cast output.
Normal tests are offline. Network inspection requires the explicit
`--authenticate-ip-stdin` flag and one private IPv4 address on stdin. That mode can
send only one device-auth challenge, never app launch/control, media, volume or
queue commands. The separate tone mode requires explicit tone, legacy-policy and
volume flags; it is never reached by an authentication-only invocation. It closes on success or failure; no installed-player changes occur.
Strict certificate policy remains the default. The additional explicit
`--allow-legacy-device-certificates` flag permits verified RSA/SHA-1 signatures only
in the device certificate path. That is a reviewed legacy security tradeoff, not
an automatic fallback or a certificate-verification bypass. The approved retry is
complete; further receiver contacts require fresh scope/permission.

## Implemented boundary

`examples/cast_auth/proof.rs` verifies a bounded Cast protobuf authentication reply
against explicitly supplied, separate device and revocation trust anchors:

1. Bind a fresh random 16-byte nonce to the exact TLS peer DER and a single-use,
   ten-second context. Consume the context on either success or failure.
2. Require the platform device-auth namespace, receiver-to-sender route, v1.0 binary
   envelope, a response without challenge/error, and no chunking or text payload.
3. Match the nonce, require RSA PKCS#1 v1.5/SHA-256, and reject absent SHA-256,
   SHA-1, PSS and unknown algorithms. No downgrade/fallback.
4. Parse the exact peer certificate with no trailing data; enforce current validity
   and at most four days of remaining validity. This is not public-Web-PKI hostname
   authentication and does not by itself establish receiver identity.
5. Validate the device chain with rustls-webpki/ring against explicit roots, certificate
   signatures/constraints and validity. Require digital-signature key usage and
   client-auth-compatible EKU. Verify a >=2048-bit RSA signature over **nonce || exact
   TLS peer certificate DER**. Echoing the nonce or substituting a TLS certificate
   is insufficient.
6. Require a separate authenticated, valid version-0 Cast revocation list. Verify its
   signer chain and SHA-256 signature over the original TBS bytes. Enforce time/hash/
   serial-range bounds and check every certificate in the verified device path,
   including the root, for SPKI-hash and issuer/serial revocation.
7. Derive audio-only restrictions from the **verified path**, not arbitrary supplied
   certificates. Clamp proof lifetime to peer/device-path/CRL/signer expiry. Return
   only peer fingerprint, restriction and expiry, never a raw authenticated stream.

Errors are static categories, not peer-provided strings. There is no logging of
messages, keys, device addresses, certificates or URLs. No system trust store,
trust-on-first-use or public CORS proxy is used. Cast's self-signed TLS certificate
is initially quarantined, not trusted: rustls verifies handshake key-possession
signatures and peer validity, then the mandatory device-auth verifier must establish
the certificate's Cast-root binding. The raw stream is never exposed to external callers. The separately opted-in tone
protocol runs internally only after complete authentication and expiry checks.

## Bounds and conservative compatibility policy

- Envelope: 64 KiB; certificate: 16 KiB; CRL bundle: 32 KiB.
- At most four supplied intermediates and eight configured roots per trust domain.
- Exactly one supported CRL record; at most 256 revoked hashes and 256 serial ranges.
- Protobuf input size is bounded before decode; decoded field counts are validated.
- Deadline is checked before and after cryptographic work. Input/path limits bound
  work, but a synchronous crypto call is not interruptible mid-verification.
- The TLS probe also enforces a ten-second overall IO deadline, two-second connect
  limit, 100 ms cancellation/read/write polling and 512 KiB aggregate encrypted-wire
  budget. It performs no retries, name resolution or discovery. TLS resumption,
  early data, SNI and key logging are disabled; connections are never reused.
- Roots are also checked for validity. Certificate signatures default to RSA/SHA-256.
  The explicit compatibility flag adds verified RSA/SHA-1 only for the device path;
  CRL-authority paths remain strict, as do fresh authentication and CRL signatures.
  EKU must permit client authentication or be absent. Unknown critical extensions
  and multi-version CRL bundles fail validation. No peer-triggered fallback exists.

This is deliberately stricter than the inspected Open Screen defaults, which can
allow optional CRLs/legacy digest behavior. These choices may reject older devices;
they are **not** evidence that the operator's firmware supports this subset. A policy
change needs explicit review and new negative tests, not a compatibility bypass.

## Evidence and dependencies

Run the proof with:

```zsh
cargo test --locked --example cast_auth_probe
cargo test --locked --example cast_frame_probe
cargo clippy --locked --all-targets -- -D warnings
```

The original offline core passed 32 authentication tests, 7 framing tests and 444
application tests on Rust 1.95 and 1.99.1. The expanded suite adds root fingerprint,
trust-domain, address-class and explicit policy-flag checks. `python3 tests/cast-auth.py`
runs two exact ignored contracts (private TLS and legacy-chain regression) with
fresh ephemeral synthetic keys: TLS 1.2/1.3,
valid authentication, forged signature, revocation, wrong TLS binding, oversized
frame, handshake deadline and cancellation. It checks that the client sends no
further application bytes and that `SSLKEYLOGFILE` is ignored. It never reads saved
receiver targets. CI explicitly runs this scoped contract, not all ignored tests.
The expanded suite includes 44 regular authentication/root/protocol/CLI checks, the two scoped private contracts, 7 framing tests and 444 application tests,
plus strict all-target Clippy and formatting. A signed legacy SHA-1 chain fixture
reproduces strict rejection and explicit opt-in success through TLS. Forged/revoked
legacy identities and SHA-1 fresh authentication responses still fail. The key-log assertion runs
the compiled test executable directly, so Cargo's separate TLS stack cannot create
a misleading key-log file during the assertion.
No development workflow/release has been published. Existing dependency versions
and checksums remain retained; proof dependencies remain dev-only.

The authentication suite covers positive signed chains and adversarial cases for
fresh-nonce replay, altered TLS binding/signatures/certificates, wrong roots,
missing intermediates, key usage, weak RSA, critical extensions, certificate and
challenge expiry, inherited audio policy, mandatory CRLs, signer trust/expiry,
revoked leaves/intermediates/roots/serials, malformed ranges, envelope routing,
algorithms, field presence, input limits and every truncated reply prefix.

Fixtures are original synthetic data generated with OpenSSL and verified by the
independent native Rust cryptographic implementation. Only public certificates,
signatures and protocol records are committed; generation-time private keys are
removed. Offline tests use a fixed fixture clock. The loopback runner generates fresh
private fixture keys/certificates outside the repository, uses the real clock for
TLS, and removes all ephemeral keys on exit. OpenSSL is a fixture-generation tool,
not a probe/player runtime dependency. See
[`tests/fixtures/cast-auth/README.md`](../tests/fixtures/cast-auth/README.md) for
provenance and regeneration rules.

The proof uses dev-only pinned `prost` 0.14.4 and `x509-parser` 0.18.1, plus the
already-locked rustls-webpki 0.103.15 and rustls-pki-types 1.15.1. The transport
uses the already-locked rustls 0.23.45/ring 0.17.14 as direct dev dependencies. Cryptographic
signatures/path validation are library operations, not handwritten crypto. X.509
parsing alone is never treated as authentication. No `rust_cast` dependency was
adopted. These dev dependencies do not enter the normal player dependency graph.
This is not an independent security audit or a clearance of existing advisories.

## What this does not prove / next gate

- The probe's device and CRL trust stores now use three fingerprint-pinned public
  Google certificates from the reviewed Open Screen revision. Source provenance,
  expiry and the distinction between anchor self-signatures and untrusted chain
  signatures are documented in `examples/cast_auth/roots/README.md`.
- The operator explicitly authorized connecting to one selected receiver and
  identified it as the saved Chromecast Audio target. Three bounded auth-only
  connections narrowed the rejection from certificate-chain failure to unsupported
  certificate algorithm, then specifically **RSA/SHA-1**. No other saved receiver
  was contacted. The operator then approved research/private tests followed by
  **one scoped legacy-device-certificate retry**, which authenticated successfully.
  The accepted proof reported `audio_only=true`. No permission to launch/play follows.
- The [legacy compatibility investigation](cast-legacy-compatibility.md) records
  upstream behavior and the security tradeoff. Strict mode still rejects the legacy
  chain. The opt-in is limited to the dev probe; it is not an approved global player
  policy, release or installation. The approved single retry has been consumed;
  obtain fresh permission before further receiver contacts.
- One fixed public synthetic FLAC has now played audibly under separate explicit
  authorization; see [tone validation](cast-tone-validation.md). TIDAL media delivery,
  wider codec/range/expiry behavior and shared player integration remain unproven.
  The wider P7a gate is open; do not start the Devices UI, claim general Cast output,
  or publish/install v0.34 on the strength of these probes.
