---
title: Apple simulators are chosen like the Apple Bazel rules
date: 2026-11-26
---

`struo test` no longer runs on whichever iPhone or iPad simulator happens to be
booted. Like the Apple Bazel rules' test runner, each test run uses a simulator
Struo owns, named after its device type and runtime, such as
`Struo Apple Watch Series 9 (45mm) (watchOS 26.5)`. Struo picks the runtime
that matches the SDK the bundle was built with, creates the simulator the first
time it is needed, and reuses it afterwards. watchOS, tvOS, and visionOS test
bundles now run on a simulator of their own platform instead of failing to
install on an iPhone.

`struo run` follows `bazel run`: it uses the best existing simulator for the
platform, preferring a booted one, an iPhone over an iPad, and newer device
types, and creates one only when none fits.

Set `device_type` and `os_version` on `apple_test_bundle` or
`apple_application` to choose the simulator, such as an iPad on iOS 18.2, or
set `STRUO_APPLE_SIMULATOR_UDID` to run on a specific simulator. See the
[`apple_test_bundle` reference](/reference/prelude/apple_test_bundle).
