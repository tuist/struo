---
title: Smaller Cargo outputs and large Bazel workspaces
date: 2026-11-26
---

- Rust crates from a Cargo workspace compile without embedded LLVM bitcode
  unless the profile enables `lto`, as Cargo does. Build outputs and cache
  entries are roughly half their previous size.
- Bazel targets whose action graph exceeds the analysis limit no longer fail.
  Struo decides whether to run a target itself from a smaller action graph
  first, and delegated `bazel build` runs keep the workspace's own
  configuration, so they reuse Bazel's caches instead of rebuilding.
- A host command that exceeds the analysis output limit now says so in the
  error instead of only naming the program.
