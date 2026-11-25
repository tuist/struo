---
title: Cargo profiles, outside build-script inputs, and tighter package bounds
date: 2026-11-25
---

Cargo workspaces imported without configuration now build closer to Cargo:

- The `dev` profile, and the `test` profile for test targets, apply from the
  root manifest and Cargo configuration, including per-package overrides,
  `package."*"`, and `build-override`. A package with `opt-level = 3` compiles
  optimized, its build script sees `OPT_LEVEL=3`, and procedural macros stay
  unoptimized as in Cargo.
- A workspace member's build script can read files outside its package
  through `build_script_data` on `cargo_workspace`. A change to one reruns the
  script. A script that names an untracked path with `rerun-if-changed` fails
  and prints the `struo.toml` to add, instead of caching output that could go
  stale.
- A package no longer includes the local packages nested inside it, its
  `target` directory, or, at the workspace root, `.git`. Running `cargo build`
  next to Struo keeps the cache warm, and `struo query affected-tests` maps
  root files to the root package's tests.
- Build scripts that depend on `cc`, `cmake`, or `autotools` receive
  `NUM_JOBS=4` and reserve four CPU slots, so native builds such as
  `protobuf-src` compile in parallel. `build_script_jobs` changes the count.
- Integration tests link to their package's binaries instead of copying them,
  and each test binary is stored once.
- `.struo/` writes its own `.gitignore`.

See the [Rust guide](/docs/guide/graph/rust). The Getting Started guide now
has a section on building Struo from source.
