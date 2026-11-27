---
title: Go modules build and test without a struo.toml
date: 2026-11-27
---

Struo now recognizes a Go module from its `go.mod` and asks the Go command for
its packages, tests, and dependencies. `struo build` and `struo test` work on
an existing Go repository with no configuration, including repositories with
several modules, local `replace` directives, vendored dependencies, and cgo.

Builds read dependencies from the vendor directory, a local replacement, or a
snapshot of the Go module cache, and never fetch during the build. All Go
actions share one Go build cache, so the standard library compiles once per
workspace. A changed file selects only the tests of the packages that build
it, embed it, read it from their package directory or `testdata`, or name it.
A changed cgo header rebuilds the packages that include it.
