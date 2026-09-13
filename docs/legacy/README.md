# Retained Flux build experiments

`build.rs.txt` preserves the original build script, including its earlier
Protobuf-only fallback and external-type mapping notes. The fallback was inside
a Tonic-only compile gate; normal Cargo builds selected the Tonic branch.
The active build script now expresses that feature boundary directly.
