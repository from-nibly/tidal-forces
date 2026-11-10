# Bundled AAC codec

Tidal Forces uses `fdk-aac` 0.8.0 (MIT Rust bindings) and `fdk-aac-sys` 0.5.0
for native AAC-LC, HE-AAC and HE-AAC v2 decoding. The codec is linked into the
single executable; no external player, ffmpeg executable or separately installed
FDK library is needed at runtime.

The Fraunhofer FDK AAC codec has its **own license**, not the application's MIT
license. Its complete notice is in `assets/licenses/FDK-AAC.txt` and embedded in
the executable. Run `tidal-forces --licenses` to read it. That license does not
grant patent rights; see its full terms rather than treating this document as a
patent-license grant or legal advice.

## Complete source availability

The unmodified `fdk-aac-sys-0.5.0.crate` source archive, including the complete
codec implementation, Rust FFI and build script, is provided free of charge in
`assets/sources/` **and inside every release executable**. Extract it without
network access (Nushell):

```nu
tidal-forces --export-fdk-source fdk-aac-sys-0.5.0.crate
tar -xzf fdk-aac-sys-0.5.0.crate
```

Extraction refuses to overwrite an existing file. A regression test checks that
the embedded archive exactly matches the registry checksum in `Cargo.lock`.
We have not modified the codec sources packaged by that dependency.

Upstream source and bindings:

- https://crates.io/crates/fdk-aac-sys/0.5.0
- https://crates.io/api/v1/crates/fdk-aac-sys/0.5.0/download
- https://github.com/haileys/fdk-aac-rs

Native Linux routing uses libpulse-binding 2.30.1 (MIT OR Apache-2.0), with a
local cancellation-lifetime fix documented in `vendor/README.md`. Its original
licenses remain in `vendor/libpulse-binding/`; the MIT notice is embedded in
`--licenses`. The executable dynamically links the operating system's libpulse
client library (LGPL-2.1-or-later); no PulseAudio daemon is bundled.

Classic skin parsing uses zip 9.2.2 (MIT), with only stored/deflate support and the
already-used Rust zlib backend. The iterable-reader API is pinned exactly because
it is upstream-unstable; it allows entry-count checks before allocating an archive
index and preserves duplicate names for rejection. New transitive nix 0.31.3 (MIT)
and typed-path 0.13.0 (MIT option) notices, plus zip's notice, are embedded in
`--licenses` and retained in `assets/licenses/`. BMP support uses the existing image
crate; Unix regular-file/no-follow checks use the already-locked libc crate.
No third-party skin art, Winamp executable, plugin, script or shader is bundled.
Users must have permission to use their own skin assets. Synthetic fixture artwork
is original test code, excluded from release builds. Sprite-coordinate compatibility
was checked against Webamp's documented classic main-panel layouts; no Webamp code
or artwork is incorporated.

The 0.34 development native Cast runtime shares the reviewed authentication proof.
Its wire definitions follow Chromium Open Screen's Cast schemas under the BSD
notice retained in `assets/licenses/cast/OPENSCREEN.txt` and displayed by `--licenses`.
Private fixtures remain original synthetic public certificates/signatures; no private
keys or real receiver captures are included. Public Cast/Eureka/CRL trust certificate
provenance/fingerprints are in `examples/cast_auth/roots/`. The isolated probe defaults
to strict policy; the operator-approved product compatibility policy supports reviewed
signed legacy device chains while retaining fresh SHA256 nonce/TLS binding, trusted
roots, validity/key/path constraints and signed revocation.

Production prost (Apache-2.0) and x509-parser (MIT option) perform protobuf/X.509
parsing; rustls, rustls-pki-types, rustls-webpki and ring perform TLS/cryptographic
verification. Their upstream notices, including ring's component notices, are
retained in `assets/licenses/cast/` and embedded in `--licenses`. Pinning, bounds,
quarantined transport and validation limits are documented in
`docs/cast-authentication.md` and `docs/cast-native-validation.md`. The installed
v0.33 executable remains unchanged until separate version approval and deployment.

Native device discovery uses mdns-sd0.21.7 (MIT option) without its default logging
or async features. Its notice is in `assets/licenses/cast/MDNS-SD.txt` and embedded
in `--licenses`; browsing does not establish device trust or open receiver sessions.

Other project dependencies and pinned versions are recorded in `Cargo.lock`.
Inter font notices are in `assets/fonts/OFL.txt` and are also shown by
`--licenses`. Application source remains MIT licensed under `LICENSE`.
