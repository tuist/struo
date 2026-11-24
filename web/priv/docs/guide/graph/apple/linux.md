# Build iOS Applications on Linux

Cross-compile, sign, and cache iOS libraries, frameworks, and applications on
Linux with Struo, using Swift and the Darwin SDK installed by xtool.

Struo owns compilation, linking, resource packaging, caching, and scheduling.
It does not run `xtool dev build` behind the scenes. Declare targets in
`struo.toml`; reading Xcode projects requires macOS.

## Prerequisites

The [omarchy-apple-dev](https://github.com/joshuaswarren/omarchy-apple-dev)
project provides an installer and Linux replacements for Apple resource tools.
Its installer supports Omarchy's Arch-based environment on ARM64 and x86_64,
but Struo only needs the installed tools and SDK. Follow its installation guide
to get:

- A Linux Swift toolchain matching the Swift version in the selected Xcode.
- An [xtool](https://xtool.sh) Darwin SDK built from your own Apple-provided
  `Xcode.xip` download.
- `rcodesign` for bundle signing.
- The Linux `actool`, `momc`, and `ibtool` compatibility tools when your targets
  use asset catalogs, Core Data models, storyboards, or xibs. omarchy-apple-dev,
  not Struo, determines which resource formats they support.
- `llvm-lipo` on `PATH` if you build multi-architecture libraries. The xtool
  toolset does not always include it.

Struo does not download or redistribute Apple's SDK. Obtain Xcode from Apple
with an Apple ID and install the SDK with xtool:

```sh
xtool sdk install /path/to/Xcode.xip
swift sdk list
```

The SDK list must include `darwin`. Run the omarchy-apple-dev installer with
your downloaded Xcode archive to also install the compatibility tools:

```sh
XCODE_XIP=/path/to/Xcode.xip ./install-toolchain.sh
```

Use the Swift toolchain's own Clang when installing the SDK. Host Clang headers
from a different compiler version can make SwiftUI fail to compile. If your
toolchain needs a compatibility `LD_LIBRARY_PATH`, Struo passes it through to
compiler and resource actions.

## Declare targets

Materialize the Linux starter in an empty directory and build it:

```sh
struo edit materialize-example apple_application apple-application-linux
struo build Hello
```

The starter links a Swift library into an iOS application. Both targets use:

```toml
[target.attrs]
platform = "ios"
sdk_variant = "device"
minimum_os = "17.0"
```

Set `sdk_variant = "device"` on dependencies as well as the application. The
default remains `"simulator"`, and Struo does not change it on Linux. An iOS
device target defaults to ARM64 even on an x86_64 host; library `archs`
overrides still apply.

### Pin the SDK

Changes to the SDK, tool binaries, or compiler environment invalidate the
affected cached actions. To pin one installed SDK, set `xcode_developer_dir` to
the SDK bundle's `Developer` directory on every relevant target:

```toml
xcode_developer_dir = "/home/me/.swiftpm/swift-sdks/darwin.artifactbundle/Developer"
```

This selects SDK metadata and tools, not macOS executables extracted from Xcode.
After switching Swift installations, re-register the matching SDK with
omarchy-apple-dev's `install-toolchain.sh --repair`.

## Sign for a device

By default Struo produces an ad-hoc signed application bundle, which a physical
iPhone does not accept. For an installable development build, give the
application an Apple Development identity and a development provisioning
profile that includes the device:

```toml
[target.attrs]
platform = "ios"
sdk_variant = "device"
minimum_os = "17.0"
bundle_id = "dev.struo.hello"
signing = "development"
signing_identity = "/secure/AppleDevelopment.p12"
signing_identity_password_file = "/secure/AppleDevelopment.password"
provisioning_profile = "/secure/Hello_Development.mobileprovision"
```

Struo embeds the profile, derives the signing entitlements from it, and signs
embedded frameworks and then the application with rcodesign. The identity stays
where it is on disk; see
[development signing](/reference/prelude/apple_application#development-signing)
for how it is validated and kept out of the cache.

Struo does not request identities or profiles from Apple or install the bundle
on a device. Use xtool or omarchy-apple-dev for account provisioning, USB
deployment, and App Store signing and uploads. A Struo build is not an App
Store-ready archive.

## Limitations

- Available destinations are limited to those in your Darwin SDK. Mac Catalyst
  is not supported on Linux.
- Apple binaries cannot run on Linux. Simulator runs, XCTest and Swift Testing
  bundles, Xcode app thinning, and Intent definition code generation require
  macOS.
- `swift_macro` builds require macOS. Prebuilt compiler plugins supplied through
  `binary_swift_plugins` must be Linux executables. SDK-provided macros use the
  SDK's OpenAppleMacros server when installed.
- Xcode project discovery requires macOS. Declare Apple targets in
  `struo.toml`.

## Target kinds

These target kinds build with the Linux toolchain:

- [`apple_library`](/reference/prelude/apple_library)
- [`apple_framework`](/reference/prelude/apple_framework)
- [`apple_application`](/reference/prelude/apple_application)
- [`apple_executable`](/reference/prelude/apple_executable)

## Next steps

- [Apple](/guide/graph/apple): declare applications, libraries, and tests, and
  choose values by configuration.
- [Development signing](/reference/prelude/apple_application#development-signing):
  check how identities and profiles are validated.
- [Ecosystems](/guide/graph/ecosystems): combine Apple targets with other
  toolchains in one graph.
