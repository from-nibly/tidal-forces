# Cast legacy certificate compatibility investigation

## Current product policy (supersedes the historical opt-in scope below)

For the cohesive0.34 development feature, the operator explicitly approved built-in
compatibility for the reviewed signed legacy Cast certificate chains, without a
certificate checkbox. The native runtime selects that policy before connecting;
it does not reconnect or relax verification after a failure. Trusted roots, path
constraints, key size/validity, freshSHA256 nonce/TLS binding and mandatory signed
revocation remain enforced. This retains the documented weaker certificate-signature
tradeoff; it is not equivalent to strict modern chain signatures. The isolated
probe still defaults to strict policy. Version/release approval remains separate.

## Historical investigation

**The explicitly approved single legacy-certificate retry authenticated the selected
Chromecast Audio successfully.** Trusted path, fresh SHA-256 nonce/TLS binding,
mandatory signed revocation and audio-only restriction passed. The connection closed
without app launch, control, playback or volume changes. Strict mode remains default.
Further receiver contacts and any player-wide legacy policy remain separately gated.

## Findings

The initial authorized authentication exchange reached a device-auth response, but
the device certificate path was rejected for RSA/SHA-1. This is distinct from:

- the pinned root certificate's self-signature (not used to establish anchor trust);
- the fresh device-auth signature over the nonce and TLS peer certificate;
- the signed revocation list;
- the TLS handshake signatures/cipher suite.

The response passed the envelope, echoed-nonce and advertised SHA-256 checks before
path validation failed. Initially its actual device-auth signature and revocation proof were not verified,
because processing stopped at the rejected path. Both passed in the subsequently
approved legacy retry. Media delivery remains unproven.

Chromium Open Screen's pinned native verifier explicitly accepts RSA/SHA-1,
RSA/SHA-256, RSA/SHA-384 and RSA/SHA-512 **certificate** signatures, while still
verifying each signature, CA constraints and path to its Cast roots. This confirms
that a legacy certificate chain is a supported Cast interoperability case, not
proof of a bad password or unauthorized TIDAL account.

Source inspected:
[boringssl_trust_store.cc at 784bb6902132df45fad06638d4c88c273e6eed9f](https://chromium.googlesource.com/openscreen/+/784bb6902132df45fad06638d4c88c273e6eed9f/cast/common/certificate/boringssl_trust_store.cc),
certificate-signature dispatch near lines 267–295. Its separate device-auth and CRL
policies are **not** adopted wholesale: our fresh challenges and revocation policy
remain stricter.

## Private experiment

`examples/cast_auth/legacy_tests.rs` is behind `#[cfg(test)]`. Following the initial
private experiment and explicit policy approval, its adapter was moved to
`legacy_certificate.rs` for the opt-in probe. It uses ring's explicitly named
`RSA_PKCS1_2048_8192_SHA1_FOR_LEGACY_USE_ONLY` verifier through rustls-webpki.
No crypto algorithm was implemented from scratch. The live CLI requires the exact
additional `--allow-legacy-device-certificates` flag; it is not enabled implicitly.

The scoped private runner generates a synthetic certificate with a real RSA/SHA-1
issuer signature and independently generated RSA/SHA-256 authentication proof.
Assertions demonstrate:

- The experimental chain verifier accepts the correctly signed legacy chain.
- The normal strict verifier continues to reject that exact chain; the explicit
  opt-in passes full authentication over private TLS while still rejecting forged
  authentication, revoked device keys and SHA-1 fresh responses.
- Wrong trust roots, tampered signatures and expired certificates remain rejected.
- The fresh SHA-256 signature still binds the nonce to the exact TLS certificate;
  modifying its input fails.
- A separately verified SHA-256 CRL can revoke the device key regardless of which
  certificate digest encoded that key. Wrong CRL authority and expiry still fail.
- Changing the fresh response's advertised hash to SHA-1 remains rejected.

The earlier loopback TLS contract also reproduces strict rejection of the signed
legacy chain over actual TLS. No authentication results depend on accepting an
unsigned or unverified certificate. Neither experiment establishes that SHA-1 has
modern collision resistance; these are interoperability/correctness tests, not a
security audit or proof that a legacy certificate chain is as strong as SHA-256.

## Security decision, not an automatic fallback

SHA-1 certificate signatures are a legacy, weaker trust mechanism. A modern nonce
signature cannot repair a forged identity certificate. Requiring official roots,
RSA >=2048, current validity, constraints and revocation limits exposure but does
not eliminate the underlying algorithm weakness. Do not describe compatibility
mode as having equivalent security to the strict policy.

There are two defensible product choices:

1. **Retain strict policy.** Leave this legacy device unsupported by the player. A firmware
   or certificate update might change compatibility, but none has been verified or
   requested; do not modify the receiver to try to fix this automatically.
2. **Explicit legacy-device compatibility, after operator approval.** Permit verified
   RSA/SHA-1 signatures only in the selected device certificate path. Still require
   the reviewed Cast roots, valid CA/path/key usage and times, RSA >=2048, the fresh
   RSA/SHA-256 nonce/TLS binding, and the existing mandatory SHA-256 CRL signature
   and strict CRL-authority path. No SHA-1 fresh challenge, TLS downgrade, optional
   revocation, trust-on-first-use or certificate-verification bypass.

The operator approved option 2 narrowly for **one authentication-only development
retry**, and that retry succeeded. This is not a global player default or release
approval. The other receiver remains untouched. The approved single connection is
complete; do not repeat it without fresh permission. Do not relax additional checks
automatically. Authorized media delivery is still required before the native Devices
UI can advance. Neither these tests nor successful authentication prove playback.

Pinning a certificate learned solely from this untrusted connection would be
trust-on-first-use, not an independently authenticated solution. No out-of-band
trusted device-certificate fingerprint source has been established here.
