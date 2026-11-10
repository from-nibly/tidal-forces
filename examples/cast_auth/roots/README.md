# Public Cast trust anchors for the isolated authentication probe

These are public certificates, not credentials/private keys. They are used only by
`cast_auth_probe`, a dev-only example, not the installed player or its trust store.

Source: Chromium Open Screen commit
`784bb6902132df45fad06638d4c88c273e6eed9f`, directory
`cast/common/certificate/`, downloaded over HTTPS from chromium.googlesource.com.
The DER arrays were extracted from the named headers in `provenance.json`; a fresh
fetch was compared byte-for-byte with the previously inspected source. Tests pin
all three DER SHA-256 fingerprints and ensure synthetic fixture chains are rejected
by this root store. No network fetch/update of roots occurs inside the probe.

- `cast.der`: Cast Root CA; expires 2034-03-28.
- `eureka.der`: Eureka Root CA; expires 2032-12-12.
- `crl.der`: separate Cast CRL Root CA; expires 2036-07-27.

The root certificates' self-signatures can use SHA-1: trust-anchor authority comes
from the reviewed/pinned bytes, not verification of their self-signature. This does
**not** automatically enable SHA-1 signatures on leaf/intermediate certificates.
The separately reviewed, explicit legacy-device flag permits verified RSA/SHA-1
only in device paths, never in CRL-authority paths or fresh authentication. Root validity is checked too. Updating/replacing these anchors is
an explicit source/license/security review, never an insecure automatic fallback.

Copyright 2019 The Chromium Authors. BSD license:
`tests/fixtures/cast-auth/OPENSCREEN-LICENSE` (relative to the repository root).
