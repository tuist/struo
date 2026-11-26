# Build Swift Packages

Build, test, and cache Swift Package Manager packages with Struo by reading
`Package.swift` directly, with no `struo.toml` and no migration step.

Swift Package Manager remains authoritative for manifests, version selection,
and `Package.resolved`. Struo builds the first-party targets and pinned
dependencies it derives from them as Apple targets.

## Prerequisites

Install Xcode and its command-line tools, then verify the selected Swift
toolchain:

```sh
xcrun --find swift
xcrun swift --version
```

Struo selects the Swift compiler through `xcrun`. A Swiftly `.swift-version`
file does not change that selection. If a package requires a separately
installed Swift toolchain, select its bundle identifier for the build:

```sh
TOOLCHAINS=your.toolchain.bundle.identifier struo build
```

Check the selection with `TOOLCHAINS=your.toolchain.bundle.identifier xcrun
swift --version`. The selected compiler must support the package's
`swift-tools-version`. A toolchain installed beside Xcode supplies the compiler,
macros, and Swift Testing, while Xcode still supplies the linker and SDK.

If a first-party Apple target does not build yet, start with the
[Apple guide](/guide/graph/apple). Package problems are easier to diagnose once
the compiler, SDK, linker, and code signing work.

## Start from an existing project

### Build and test

Struo recognizes `Package.swift` automatically. From the package directory:

```sh
struo query workspace
struo query targets
struo build
struo test
```

Struo supplies a `swift_package_workspace` seed and derives first-party
libraries, executables, macros, binary targets, and tests. Discovery skips the
generated `.build` and `.swiftpm` directories and does not write `struo.toml`.

Binary targets become
[`apple_xcframework_import`](/reference/prelude/apple_xcframework_import)
targets, or
[`apple_artifact_bundle_import`](/reference/prelude/apple_artifact_bundle_import)
targets when their path or download URL names a static library
`.artifactbundle`.

Executable products are built as `apple_application` targets, so `struo build`
produces `<Name>.app/Contents/MacOS/<Name>` rather than the bare `<Name>`
binary that `swift build` writes to `.build/debug`. The bundle lets a test
target `@testable import` the executable by running hosted in it, and it
carries the executable target's resources. Run the tool from inside the bundle.

`struo build` selects the package when it is the only discovered build root.
`struo test` runs first-party test bundles only. Use `struo test --all` when
you want the complete resolved test graph, including dependencies' tests.

### Keep swift commands

Struo can sit behind the `swift` command for one package. Add this
[mise](https://mise.jdx.dev/) wrapper, then run `mise reshim` and activate mise
in the shell that starts the build:

```toml
[wrappers.swift]
command = "struo"
args = ["swift", "--"]
```

Struo routes the default debug build and test forms into its graph:

```sh
swift build
swift test
```

`swift build` builds the package and its resolved products. `swift test` runs
each first-party test bundle once. `-q` or `--quiet`, `-c debug` or
`--configuration debug`, and `--package-path .` are also supported.

Every other request, including release builds, filters, package subcommands,
plugins, and a package path outside the current directory, invokes the system
`swift` with its original arguments and exit status.

To troubleshoot without mise, call the compatibility command directly and put
the separator before the Swift Package Manager arguments:

```sh
struo swift -- test
```

## Manage dependencies

### Local packages

Local path dependencies inside the workspace are followed transitively,
including sibling packages and shared dependencies reached through several
products. An entirely local graph needs no `Package.resolved` and does not run
dependency resolution.

A `.package(path:)` dependency outside the workspace root fails graph loading
with `swift_package_path_outside_workspace` instead of omitting that package's
products. Run Struo from a directory that contains both packages, for example
`struo -C ..` from a package whose sibling it depends on.

### Remote packages

Struo checks out each source-control dependency at the revision pinned in
`Package.resolved` and compiles its targets directly; Swift Package Manager does
not build a separate dependency graph. When the lockfile is not committed,
Struo asks Swift Package Manager to resolve it first. If a local dependency
introduces a remote package, it is resolved through the root package's
lockfile.

Registry dependencies work the same way. Struo reads the registry pin from
`Package.resolved` and compiles the package Swift Package Manager downloaded
for that version. When the download is missing, Struo runs
`swift package resolve` once to fetch it. Configure the registry and its
credentials with Swift Package Manager as usual, for example with
`swift package-registry set` and `swift package-registry login`.

There is no initialization step:

```sh
struo query targets
struo build SwiftPackage_MyPackage_MyLibrary
```

A first build can access the network when `Package.resolved` must be created or
a pinned source-control or registry dependency must be fetched. Struo reuses a checkout that already
contains the pinned commit.

### Traits

Struo applies package traits requested by dependencies, including traits that
enable other traits and conditional requests. An explicit empty trait list
disables defaults for that dependency declaration; requests from other packages
are combined according to Swift Package Manager's rules.

## Explicit modules and dependency checks

Add `explicit_modules = true` to the `swift_package_workspace` seed's
attributes to build modules as separate cacheable actions. Optional
`dependency_check = "error"` checks source imports against declared package
dependencies. Struo propagates both settings through the resolved package
graph, while `Package.swift` remains the source of target and dependency
declarations. See
[explicit Apple modules](/guide/graph/apple#explicit-modules-and-dependency-checks)
for requirements and limits.

## Build-tool plugins

Build-tool plugins run before compilation. Swift Package Manager compiles the
plugins and their host tools and runs prebuild commands; Struo then schedules
the declared build commands. Generated sources, headers, and resources become
inputs to the consuming target. Commands without outputs still run for Swift
tools version 6 or newer, so validation tools can fail the build.

Plugin planning uses the consuming package's pinned dependencies, and missing
pins fail planning instead of selecting new versions. Planning and plugin
commands inherit the host environment, run locally without sandboxing, and are
not cached; compilation and packaging are cached as usual. Swift Package
Manager may also evaluate plugins on other targets in the package while
planning, including test targets. See
[`swift_package_build_tools`](/reference/prelude/swift_package_build_tools) for
its contract.

## Limitations

- Any change to the inherited environment invalidates cached manifest
  resolution, even when the changed variable is unrelated, because manifests
  can read the environment.
- Build-tool plugin support is tested against Apple Swift 6.3.3 and relies on
  Swift Package Manager interfaces whose formats may change between toolchains.
- When an Xcode project scans for local package products, unreadable manifests
  such as templates with placeholders are skipped with a warning. Manifests the
  graph references must still load.

## Target kinds

- [`swift_package_workspace`](/reference/prelude/swift_package_workspace): the
  package seed and its complete contract.
- [`swift_package_build_tools`](/reference/prelude/swift_package_build_tools):
  build-tool plugin commands.
- [`swift_package_dependencies`](/reference/prelude/swift_package_dependencies)
  and [`swift_package_pin`](/reference/prelude/swift_package_pin): resolved
  dependencies.
- Derived targets use the [Apple target kinds](/guide/graph/apple#target-kinds).

## Next steps

- [Apple](/guide/graph/apple): declare Apple targets directly and ship
  applications.
- [Xcode Projects](/guide/graph/apple/xcode): build an Xcode project that
  depends on these packages.
- [Testing and Scheduling](/guide/graph/testing): select and report test runs.
