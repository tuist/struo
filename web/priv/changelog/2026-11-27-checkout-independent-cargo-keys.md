---
title: Cargo builds share cache results across checkout paths
date: 2026-11-27
---

Two clones of the same Cargo project at different paths now produce the same
action cache keys, so a shared cache serves both. Previously a crate built
from a dependency outside the workspace, a build script that printed its
`OUT_DIR`, or a `-sys` crate compiling C code through `cc` embedded the
checkout path in its outputs, and every crate that depended on it rebuilt.
The compiler now records the checkout as `.`. On macOS and Linux,
build-script output is stored without the checkout path, and build scripts
receive Cargo's `CARGO_TRIM_PATHS_SCOPE` and `CARGO_TRIM_PATHS_REMAP`, which
`cc` uses to remap C and C++ objects.

Consumers of a build script now key only on the `cargo:` directives it
prints, so a parallel `make` log that interleaves differently on each run, as
in `tikv-jemalloc-sys`, no longer rebuilds them. A crate that writes a path
into its output through `env!`, such as `env!("CARGO_MANIFEST_DIR")`, is reused
only in the checkout that built it when it compiles locally without a sandbox.
`struo query graph-fingerprint` also leaves out the files a target excludes,
such as `.git` and Cargo's `target` directory.
