---
title: Umbrella children build their path dependencies from the root lockfile
date: 2026-11-27
---

A Mix umbrella child can depend on a project outside the umbrella's apps
directory with `path:`, and Struo now builds it the way Mix does: against
the packages the umbrella root's `mix.lock` and `deps/` provide, without a
lockfile of its own. Before, the graph failed to load. A change in the path
dependency selects the child's tests, and building that project on its own
still fails until it has a lockfile, as `mix compile` does.

Mix compiles also stay keyed per checkout again. Mix records absolute
source paths in bytecode, so a second clone of a project now compiles its
own code instead of restoring bytecode that points into the first clone.
