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

Other project dependencies and pinned versions are recorded in `Cargo.lock`.
Inter font notices are in `assets/fonts/OFL.txt` and are also shown by
`--licenses`. Application source remains MIT licensed under `LICENSE`.
