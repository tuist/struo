---
title: More Xcode projects build without changes
date: 2026-11-27
---

Struo now builds several patterns found in open-source Xcode projects the same
way Xcode does:

- A project that does not commit `Package.resolved` gets its remote Swift
  packages resolved from the project's requirements, instead of silently
  leaving them out of the graph.
- Files directly inside an `.lproj` folder of a Swift package target are
  localized resources, so the package gets its `Bundle.module` accessor even
  when it declares no resources.
- A framework that defines a module and has only a private header named after
  the module uses that header as its umbrella, so its Swift code sees the
  Objective-C declarations the header imports.
- Directories that Xcode records as wrappers, such as custom document bundles,
  are copied into the product whole, like folder references.
- Objective-C sources in a Swift package target with resources get the
  `SWIFTPM_MODULE_BUNDLE` accessor that Xcode generates, so they load the
  package's resources instead of the main bundle.
- A source whose file type in the project differs from its extension, such as
  a `.m` file marked as Objective-C++, compiles in the language the project
  names, and targets with Objective-C code link Foundation the way Xcode's
  linker invocation does.
- Asset catalogs compile correctly when another project started Xcode's asset
  catalog agent first.
- Hosted unit tests of a sandboxed macOS app run: the test signs a copy of the
  host with the entitlements Xcode adds for testing, and the built app keeps
  its own.
- A test target whose host shares its product name with an app of another
  platform is hosted by the app it depends on.
- `$(RECOMMENDED_MACOSX_DEPLOYMENT_TARGET)` and its siblings for other
  platforms resolve from the selected SDK.
- Static libraries checked into the project and listed in a target's Link
  Binary With Libraries phase are linked into it.
- An import of a module that apps of several platforms produce resolves to
  the app of the importer's platform.
- Swift Testing results for a test with a display name, such as
  `@Test("Adds numbers")`, are reported on the test function instead of as an
  unknown case, including parameterized tests.
- Apple apps, test bundles, executables, and macro plugins link
  reproducibly, as Xcode does by default, so a debug build no longer produces
  a different executable every time.
- Script phases and generated framework header overlays no longer record the checkout's
  absolute path, so two checkouts of the same commit produce the same graph
  fingerprint and the same keys for those actions.
