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
`cc` uses to remap C and C++ objects. `struo query graph-fingerprint` also leaves out the
files a target excludes, such as `.git` and Cargo's `target` directory.
