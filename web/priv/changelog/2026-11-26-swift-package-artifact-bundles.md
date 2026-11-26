---
title: Swift package binary targets from artifact bundles
date: 2026-11-26
---

Swift package binary targets backed by a static library `.artifactbundle` now
build. Struo reads the bundle's `info.json`, selects the variant for the
destination triple, and links its library while exposing its headers and
module map to dependents. Previously these targets were treated as
XCFrameworks and failed with a missing `Info.plist`, or, for local bundles,
were left out of the graph. See
[`apple_artifact_bundle_import`](/reference/prelude/apple_artifact_bundle_import).
