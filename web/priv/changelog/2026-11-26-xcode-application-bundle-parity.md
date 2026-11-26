---
title: Application bundles match Xcode's PkgInfo and Info.plist build keys
date: 2026-11-26
---

Application bundles now include the `PkgInfo` file Xcode writes, and their
Info.plist carries the build environment keys Xcode adds: `DTSDKName`,
`DTSDKBuild`, `DTPlatformVersion`, `DTPlatformBuild`, `DTCompiler`, `DTXcode`,
`DTXcodeBuild`, and `BuildMachineOSBuild`. Values come from the selected SDK,
the Xcode that contains it, and the build machine, and replace stale copies in
an authored Info.plist. App extensions get the keys without `PkgInfo`, as with
Xcode.

The [Swift packages guide](/guide/graph/swift-packages) now explains why
package executables build as application bundles.
