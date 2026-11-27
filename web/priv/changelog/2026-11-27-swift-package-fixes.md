---
title: Swift packages re-resolve stale lockfiles and select tests on manifest changes
date: 2026-11-27
---

A Swift package whose committed `Package.resolved` does not pin a remote
package its targets use, as when the lockfile was written for another
`Package@swift-*.swift` manifest, is now resolved again before Struo builds it,
the way Swift Package Manager does, instead of failing with a missing module.
The lockfile of a package below the workspace root is now read from that
package's directory.

`struo query affected-tests` now selects a package's first-party tests when its
native manifest changes and no generated target claims it, such as a
`Package.swift`, a virtual Cargo workspace's root `Cargo.toml`, or `nx.json`.
It used to select none.

A Swift-only `apple_framework` now passes its `defines`, `swift_defines`, and
`clang_defines` to the compiler, as an `apple_library` does, so an Xcode
framework target that relies on conditional compilation builds.
