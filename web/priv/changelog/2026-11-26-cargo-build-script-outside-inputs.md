---
title: Rust crates rebuild for the files they read
date: 2026-11-26
---

Cargo workspaces now rebuild like Cargo does, from what the compiler and build
scripts report reading instead of every file in a package:

- A crate rebuilds when a file the compiler read changes, including files
  embedded with `include_str!`. Editing `docs/` or CI files in a package at
  the workspace root leaves its crate cached.
- Reverting an edit reuses the build from before it, for the last eight
  versions of the files a crate or build script read.
- A build script reruns when a path it prints with `rerun-if-changed`
  changes, anywhere in the workspace. Scripts that generate code from shared
  protobuf definitions outside their package build without configuration, and
  a script no longer fails for naming an undeclared path.
- A build script tool that resolves to a mise shim runs as the binary mise
  selects, so build scripts that call `protoc` through mise work.
- `struo query affected-tests` attributes files a build script names in its
  string literals, or that `build_script_data` declares, to that package, so a
  change to a shared `.proto` selects the tests of every crate that depends on
  the generated code.
- A changed path that a target excludes, such as a file under Cargo's `target`
  directory, no longer selects every test.

Rule authors can use the same mechanism through `run_action`'s new
`input_discovery` option. See the [Rust guide](/docs/guide/graph/rust).
