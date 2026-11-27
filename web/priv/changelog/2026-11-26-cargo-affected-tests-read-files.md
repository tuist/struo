---
title: Documentation edits no longer select a root Cargo package's tests
date: 2026-11-26
---

`struo query affected-tests` now selects a Cargo package's tests for the files
its code lives in or names, instead of every file under its directory. In a
workspace whose root is also a package, editing `docs/`, `README.md`, or a
script in `dev/` no longer selects the root package's tests.

A package's tests are still selected by its Rust files, the directories that
hold its targets (such as `src/` and `tests/`), and its manifest and Cargo
configuration. Any other file selects the tests whose Rust files name its
path, such as a file embedded with `include_str!` or a fixture a test opens,
and a path only one integration test names selects that test alone. Struo
searches for these names while answering the query, so loading the graph
reads no sources. See the [Rust guide](/docs/guide/graph/rust).
