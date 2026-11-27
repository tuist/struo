---
title: Zig projects load packages from zig-pkg and declare build options
date: 2026-11-27
---

`zig_dependencies` now reads packages where Zig 0.16's `zig build --fetch`
puts them: set `vendor_dir = "zig-pkg"`. Each package's module comes from the
`addModule` call in its `build.zig` instead of assuming `src/root.zig`, and
packages without a `build.zig.zon`, such as a C source tarball, no longer stop
the graph from loading.

The new `zig_options` target kind generates a module of build-time constants,
the counterpart of `b.addOptions()` in `build.zig`. The new `package_deps`
attribute on `zig_dependencies` hands such a module to a package that imports
it.

A repeated `struo test` of a Zig test now replays from the cache instead of
running the tests again.
