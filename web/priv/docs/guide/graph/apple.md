# Apple Apps and Libraries

Build, run, test, and cache iOS and macOS libraries, frameworks, applications,
and test bundles with Struo, declared in `struo.toml` or derived from Xcode.

This guide declares one iOS application and a library it depends on, then
queries, builds, runs, and tests that project.

## Prerequisites

On macOS, Apple targets use the developer tools selected by `xcrun`, `swiftc`,
and `clang`. Verify that the Swift compiler is available:

```sh
xcrun --find swiftc
```

Running the iOS application in this guide also requires an installed Simulator
runtime. Building a library does not require a running simulator.

Linux can cross-compile native Apple targets using Swift, xtool's Darwin SDK,
and Linux resource and signing tools. See [Build iOS Applications on
Linux](/guide/graph/apple/linux) for installation and a runnable starter.

## Start from an existing project

You do not need to declare targets by hand for an existing project:

- [Xcode Projects](/guide/graph/apple/xcode) points Struo at an `.xcodeproj` or
  `.xcworkspace` and derives the same Apple targets.
- [Swift Packages](/guide/graph/swift-packages) derives targets from
  `Package.swift`.

## Declare targets

### Declare the application

Create `apps/Hello/struo.toml` with a reusable library and an application that
depends on it:

```toml
[[target]]
name = "AppCore"
kind = "apple_library"
srcs = ["Sources/AppCore/**/*.swift"]

[target.attrs]
platform = "ios"
minimum_os = "17.0"

[[target]]
name = "Hello"
kind = "apple_application"
srcs = ["Sources/HelloApp/**/*.swift"]
deps = ["./AppCore"]

[target.attrs]
platform = "ios"
bundle_id = "dev.struo.Hello"
minimum_os = "17.0"
families = ["iphone"]
```

`./AppCore` resolves in the same package. The dependency is typed:
`apple_application` accepts the linkable output produced by `apple_library`.

The smallest useful source layout is:

```text
apps/Hello/
├── struo.toml
└── Sources/
    ├── AppCore/
    └── HelloApp/
```

The application source needs an entry point, such as a Swift type marked with
`@main`. The library contains code imported by the application.

### Query before building

Inspect the targets declared above:

```sh
struo query targets
struo query capabilities apps/Hello/AppCore
struo query capabilities apps/Hello/Hello
struo query schema apple_application
```

`AppCore` exposes `build`. `Hello` exposes `build` and `run`. The schema query
shows the application attributes, accepted dependencies, capabilities, and
outputs without starting a compiler.

### Build and run

Build the application bundle:

```sh
struo build apps/Hello/Hello
```

Struo builds `AppCore` first because `Hello` depends on it. Outputs are
materialized under `.struo/out/<target>/`; the
[`apple_application` reference](/reference/prelude/apple_application) lists
the exact output groups.

Launch the application in an iOS simulator:

```sh
struo run --visible apps/Hello/Hello
```

Struo picks the best simulator that already exists for the platform, preferring
a booted one, or creates one when none fits. It then boots the simulator,
installs the application bundle, and launches its bundle identifier. The launch runs on every invocation instead
of replaying from the action cache. Omit `--visible` when Simulator does not
need to be brought to the foreground. macOS applications use the host
application launcher.

### Add a test target

Add a test target to the same `apps/Hello/struo.toml`:

```toml
[[target]]
name = "AppCoreTests"
kind = "apple_test_bundle"
srcs = ["Tests/AppCoreTests/**/*.swift"]
deps = ["./AppCore"]

[target.attrs]
platform = "ios"
minimum_os = "17.0"
swift_testing = true
labels = ["unit"]
```

Place tests that use Swift Testing under `Tests/AppCoreTests/`, then inspect
and run the new capability:

```sh
struo query capabilities apps/Hello/AppCoreTests
struo test apps/Hello/AppCoreTests
```

Apple tests support macOS logic tests and iOS simulator bundles. An
application host discovered through dependencies is supported. See the
[`apple_test_bundle` reference](/reference/prelude/apple_test_bundle) for the
full contract.

## Manage dependencies

Apple targets can consume native outputs from other ecosystems through normal
`deps` entries:

- [`kotlin_apple_framework`](/reference/prelude/kotlin_apple_framework)
  produces a framework that an application or test can link and embed.
- [`rust_mobile_library`](/reference/prelude/rust_mobile_library) produces an
  Apple static library for an Apple consumer.
- [`apple_xcframework_import`](/reference/prelude/apple_xcframework_import)
  selects the matching slice of a prebuilt XCFramework.
- [`apple_artifact_bundle_import`](/reference/prelude/apple_artifact_bundle_import)
  selects the matching variant of a prebuilt static library artifact bundle.

Add these only after the application builds on its own, then query the
consumer again to confirm that the dependency contract is satisfied. For Swift
package dependencies, see [Swift Packages](/guide/graph/swift-packages).

## Choose values by configuration

Configurable attributes can use `select`. For example, a library shared by an
iOS and macOS target can choose the platform framework it links:

```toml
[target.attrs]
sdk_frameworks = { select = { ios = ["UIKit"], macos = ["AppKit"] } }
```

Apple configuration keys include platform names, architectures, `simulator`,
`device`, `mac_catalyst`, combined keys such as `ios:simulator`, and
`default`. When more than one branch matches, the most specific branch wins.

Attributes that determine the configuration, including `platform`,
`sdk_variant`, `archs`, and `mac_catalyst`, must remain literal. The target
kind schema identifies any other non-configurable attributes.

See the [configurations guide](/guide/graph/configuration) for how
`--config` overrides these tokens per invocation and how the effective
configuration scopes outputs.

## Explicit modules and dependency checks

Set `explicit_modules = true` to build imported Swift interfaces and Clang
modules as separate cacheable actions, so identical modules can be reused
across targets. Set `dependency_check = "error"` to fail the build when a
source file imports a workspace module that its target does not declare as a
direct dependency.

```toml
[[target]]
name = "Core"
kind = "apple_library"
srcs = ["Sources/Core/**/*.swift"]
deps = ["Models"]

[target.attrs]
platform = "macos"
explicit_modules = true
dependency_check = "error"
```

The same attributes are available on Swift macros, frameworks, applications,
executables, and test bundles. Set them on a `swift_package_workspace` or
`xcode_workspace` seed to propagate them to resolved Swift targets. Native
Xcode targets also honor `SWIFT_ENABLE_EXPLICIT_MODULES = YES`. Existing
projects keep implicit modules by default.

`dependency_check` defaults to `"off"`. With `"error"`, an import must name a
direct dependency even when a transitive or inferred dependency makes the
module available. System modules are exempt. The error names the consuming
target, the missing module, and a suggested manifest repair.

Requirements and current limits:

- Checking requires explicit modules and a Swift compiler whose dependency scan
  reports source imports. Unsupported compilers fail with an error.
- This is not a downloadable hermetic Apple toolchain. Builds still require the
  selected Xcode installation on macOS or the installed Darwin SDK on Linux.
- Module inputs outside the workspace and the selected SDK and toolchain
  directories are rejected. So are compiler scans that need auxiliary files
  created only in their temporary cache. Move those interfaces and headers into
  the workspace, or leave explicit modules disabled for those targets.
- Each module action reserves the default memory estimate of 250 mebibytes.
  This is not a hard limit, and large module builds can need more.

## Create a device-specific size-analysis archive

Size-analysis services can report confusing results when they receive a
universal application. Sentry, for example, does not thin an uploaded
application and [recommends thinning it before
upload](https://docs.sentry.io/platforms/apple/guides/ios/size-analysis/#app-thinning).

Add a device application and a package target for the device model you want to
measure:

```toml
[[target]]
name = "HelloDevice"
kind = "apple_application"
srcs = ["Sources/HelloApp/**/*.swift"]
deps = ["./AppCore"]

[target.attrs]
platform = "ios"
sdk_variant = "device"
bundle_id = "dev.struo.Hello"
minimum_os = "17.0"
families = ["iphone"]

[[target]]
name = "HelloSizeAnalysis"
kind = "apple_thinned_package"
deps = ["./HelloDevice"]

[target.attrs]
device_model = "iPhone17,1"
```

Build the device-specific archive:

```sh
struo build apps/Hello/HelloSizeAnalysis
```

The target writes one or more `.ipa` archives and a manifest that maps them to
the requested device model. Declare another `apple_thinned_package` target for
each additional model.

These archives use ad-hoc signing and are intended for size analysis, not for
installation on a device. See the
[`apple_thinned_package` reference](/reference/prelude/apple_thinned_package)
for outputs and validation rules.

## macOS applications and distribution

Materialize the macOS starter in an empty directory, then build and run it:

```sh
struo edit materialize-example apple_application apple-application-macos
struo build Hello
struo run Hello
```

The starter loads a text resource through `Bundle.main`. macOS applications
use the native `Contents/MacOS`, `Contents/Resources`, and `Contents/Frameworks`
layout, and framework dependencies receive the `@executable_path/../Frameworks`
runpath. Declare system frameworks with `sdk_frameworks`, not custom linker or
SDK search-path scripts. Native Xcode script phases receive matching
`EXECUTABLE_PATH`, `INFOPLIST_PATH`, and `UNLOCALIZED_RESOURCES_FOLDER_PATH`
settings.

Versioned prebuilt frameworks keep their symlinks and nested code; Struo signs
the embedded copy without changing the imported framework. Do not add a root
`_CodeSignature` directory or signature-forwarding symlink to a framework.

### Keep release credentials outside cached builds

`struo build` produces an ad-hoc signed bundle for local use. Developer ID
signing and notarization are separate release steps, not `postbuild_actions`,
annotated `struo exec` scripts, or remotely cached target actions. Keep private
keys, keychain passwords, provisioning profiles, and notary credentials outside
the workspace and Struo's declared inputs, environment, outputs, and logs.

Build first without release credentials. In a separate release job, copy the
bundle into a private temporary directory. Import the authorized Developer ID
Application certificate into an isolated keychain and store notarization
credentials in a `notarytool` keychain profile. Avoid changing the machine's
default keychain or search list. Disable shell tracing, delete temporary
keychains on failure as well as success, and never publish the temporary
directory as a build artifact.

For the starter, which has no nested code or restricted entitlements, the
following script signs and notarizes only its private copy. Supply
`DEVELOPER_ID_SHA1` as the certificate fingerprint, `SIGNING_KEYCHAIN` as the
isolated keychain path, and `NOTARY_PROFILE` as its credential profile name.
These selectors are not passwords. The final ZIP is written to a new `dist`
directory after notarization and local policy checks succeed:

```sh
#!/bin/sh
set -eu
umask 077
: "${DEVELOPER_ID_SHA1:?Select a Developer ID Application certificate}"
: "${SIGNING_KEYCHAIN:?Provide an isolated signing keychain}"
: "${NOTARY_PROFILE:?Provide a notarytool credential profile}"
test ! -e dist
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
app="$work/Hello.app"
ditto .struo/out/Hello/Hello.app "$app"
codesign --force --sign "$DEVELOPER_ID_SHA1" --keychain "$SIGNING_KEYCHAIN" \
  --options runtime --timestamp "$app"
codesign --verify --deep --strict "$app"
ditto -c -k --sequesterRsrc --keepParent "$app" "$work/submission.zip"
xcrun notarytool submit "$work/submission.zip" --wait \
  --keychain "$SIGNING_KEYCHAIN" --keychain-profile "$NOTARY_PROFILE" \
  --output-format json > "$work/notarization.json"
test "$(plutil -extract status raw -o - "$work/notarization.json")" = Accepted
xcrun stapler staple "$app"
xcrun stapler validate "$app"
codesign --verify --deep --strict "$app"
spctl --assess --type execute "$app"
mkdir dist
ditto -c -k --sequesterRsrc --keepParent "$app" dist/Hello-macos.zip
```

Run this as an ordinary shell script, outside Struo. Its caller owns keychain
creation and cleanup. Never sign, change a property list, or staple the bundle
under `.struo/out` in place. Create update signatures and checksums from the
final stapled ZIP, not the submission archive.

For applications with nested helpers, frameworks, XPC services, or extensions,
sign each nested executable and bundle inside out before the enclosing
application, with application-specific entitlements for each component. Do
not use `codesign --deep` to sign. Apps with restricted capabilities such as
push notifications must validate the profile's team, application identifier,
expiration, signing certificate, and authorized entitlements before signing;
do not strip those entitlements to make validation pass. The starter script
does not implement provisioning or nested-code signing. See Apple's
[code signing guidance](https://developer.apple.com/library/archive/technotes/tn2206/_index.html)
and [notarization workflow](https://developer.apple.com/documentation/security/customizing-the-notarization-workflow)
for distribution requirements.

## Limitations

- `struo run` launches on a simulator or the macOS host. Launching directly on
  an iPhone or iPad is not supported.
- Apple test bundles do not support direct `test_host` attributes, custom
  destinations, test plans, or device runners. Non-empty unsupported
  attributes fail during graph analysis instead of being ignored.
- Signing modes other than `ad_hoc` and `development` are not supported. See
  [development signing](/reference/prelude/apple_application#development-signing)
  for installable device builds.
- Prebuilt framework archives other than XCFrameworks and static library
  artifact bundles need a project module,
  and the built-in `archive_download` accepts ZIP archives, not tar.

## Target kinds

- [`apple_library`](/reference/prelude/apple_library)
- [`apple_framework`](/reference/prelude/apple_framework)
- [`apple_application`](/reference/prelude/apple_application)
- [`apple_resource_bundle`](/reference/prelude/apple_resource_bundle)
- [`apple_thinned_package`](/reference/prelude/apple_thinned_package)
- [`apple_test_bundle`](/reference/prelude/apple_test_bundle)
- [`apple_xcframework_import`](/reference/prelude/apple_xcframework_import)
- [`apple_artifact_bundle_import`](/reference/prelude/apple_artifact_bundle_import)
- [`swift_macro`](/reference/prelude/swift_macro)

## Next steps

- [Xcode Projects](/guide/graph/apple/xcode): derive these targets from an
  existing Xcode project instead of declaring them.
- [Testing and Scheduling](/guide/graph/testing): select, filter, and report
  test runs across the graph.
- [Ecosystems](/guide/graph/ecosystems): add Android or Rust code alongside
  the application, one integration at a time.
- [Memory](/guide/memory/): record durable context about graph work.
