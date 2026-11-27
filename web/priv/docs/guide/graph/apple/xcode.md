# Build Xcode Projects

Point Struo at an existing Xcode project or workspace to build, test, and cache
its targets without migrating it or restating its targets in `struo.toml`.

The `.xcodeproj` stays the source of truth for targets, build settings, and
file membership. Struo reads it, derives a typed graph, and compiles what it
finds there.

## Prerequisites

Reading an Xcode project requires a macOS host with Xcode and its command-line
tools. Struo uses `plutil` to read `project.pbxproj` and `xcrun` to locate
compilers and software development kits:

```sh
xcrun --find swiftc
plutil -help
```

Struo reads the project as it stands on disk and does not run package managers
or code generators for you. Run `pod install`, fetch vendored binaries, or
execute a bootstrap script before pointing Struo at the result.

## Start from an existing project

### Inspect the derived graph

An Xcode project or workspace at the repository root is discovered
automatically, so there is nothing to write. A root workspace whose projects
all live in subdirectories is discovered as one seed for the whole workspace.
From the directory holding the `.xcodeproj` or `.xcworkspace`:

```sh
struo query workspace
struo query targets
```

Struo detects the project, supplies an `xcode_workspace` seed, and resolves the
graph. Discovery does not write `struo.toml`. The output contains the seed plus
one target per native target. A browser-style application project resolves
into something like this:

```text
targets:
  xcode (xcode_workspace) [build]
    tool xcode: plutil, xcrun
  Client (apple_application) [build, run]
  BrowserKit (apple_library) [build]
  Tabs (apple_library) [build]
  Bookmarks (apple_library) [build]
  ResourceBundle (apple_resource_bundle) [build]
  ClientTests (apple_test_bundle) [build, test]
  XCFramework_RustComponents.xcframework (apple_xcframework_import) [build]
```

Each native target becomes the Apple target kind that matches its product type
(see [Target kinds](#target-kinds)). Inspect one derived target and the
contract it satisfies:

```sh
struo query target Client
struo query capabilities BrowserKit
```

### Build and test

Build the discovered project. Application projects build their application
roots; library-only projects build their non-test products:

```sh
struo build
```

No scheme or target name is needed when the directory contains one discovered
Xcode project. If Struo discovers several project roots, pick one with an
identifier from `struo query targets`. Building one derived target builds only
that target and its dependencies:

```sh
struo build BrowserKit
```

Run the same command twice. The second run is a cache hit.

Test bundles carry the `test` capability and schedule like any other Struo test
target:

```sh
struo test ClientTests
```

Like `xcodebuild test -scheme`, a test bundle builds with the configuration of
its schemes' Test action, such as a `Test` configuration that defines `TEST`.
The bundle and everything it reaches whose settings differ under that
configuration are built as separate targets named with the configuration as a
suffix, such as `App_Test`, so `struo build` still builds the project
configuration. Targets with script phases count as differing, since scripts
receive `CONFIGURATION`. A bundle builds with the project configuration when
the schemes that test it name different configurations, when it reaches a
target in another project, or when it or a target it reaches has no sources
in the Test action configuration.

`struo test` with no target runs the first-party test bundles of the discovered
project. `struo test --all` includes tests from the complete resolved graph.
See [Testing and Scheduling](/guide/graph/testing) for selection and reporting.

### Keep xcodebuild commands

Struo can sit behind the familiar `xcodebuild` command when a project uses
[mise](https://mise.jdx.dev/) command wrappers. Add this to the project's
`mise.toml`, then run `mise reshim` and activate mise in the shell that starts
the build:

```toml
[wrappers.xcodebuild]
command = "struo"
args = ["xcodebuild", "--"]
```

The wrapper preserves the original arguments; the `--` separator belongs to
Struo and is not passed to Xcode. For example:

```sh
xcodebuild -project Client.xcodeproj -scheme Client -configuration Debug build
```

Struo builds through its graph only when the request is equivalent to one:

- one resolved Xcode workspace seed for a discovered project;
- a scheme that exactly matches one reachable Struo build target;
- an explicit `-configuration` matching the seed's `configuration` attribute
  (default `Debug`), so custom names such as `Development` work when the seed
  selects them;
- the default or explicit `build` action;
- an explicit `-project` or `-workspace`, if given, naming the selected project
  or workspace relative to the Struo workspace root.

Every other request, including tests, archives, exports, destinations, package
resolution, custom build settings, help, and version queries, invokes the
system `xcodebuild` with the same arguments and exit status. Editors,
continuous integration, and coding agents keep their existing commands.

To troubleshoot without mise, call the compatibility command directly and put
the separator before the Xcode arguments:

```sh
struo xcodebuild -- -project Client.xcodeproj -scheme Client -configuration Debug build
```

## Configure the seed

### Declare the seed explicitly

Declare the `xcode_workspace` target yourself when the project is not at the
repository root, when a directory holds more than one project, or when you want
to pin a configuration:

```toml
[[target]]
name = "App"
kind = "xcode_workspace"
srcs = ["App.xcodeproj/project.pbxproj"]

[target.attrs]
project = "App.xcodeproj"
```

`project` can be omitted when the package contains exactly one top-level
`*.xcworkspace`, which Struo resolves the way Xcode opens it, or otherwise
exactly one top-level `*.xcodeproj`. Projects nested deeper, such as demo apps
and tools, never make the choice ambiguous. When a directory holds several
top-level workspaces, or several top-level projects and no workspace, the
`xcode` seed reports a `native_entry_point_ambiguous` diagnostic whose repairs
give the exact target to add to `struo.toml` for each candidate; other seeds in
the repository still load. For a project in a subdirectory, declare the target
in the workspace root package and point `project` at the nested path.

Editing a file in `srcs` re-resolves the graph. Keep `srcs` to
`project.pbxproj` and other text files such as `.xcconfig` and `.xcscheme`: a
glob that matches binary assets fails resolution, because resolver inputs must
be UTF-8 text.

### Work from a workspace

Point `project` at an `.xcworkspace` to resolve every project it references:

```toml
[[target]]
name = "App"
kind = "xcode_workspace"
srcs = ["App.xcworkspace/contents.xcworkspacedata", "App.xcodeproj/project.pbxproj"]

[target.attrs]
project = "App.xcworkspace"
```

Struo merges the targets of every referenced `.xcodeproj` into one graph, so
dependencies that cross project boundaries are wired. A referenced project that
is not on disk yet, such as `Pods.xcodeproj` before `pod install`, is skipped
instead of failing the graph. A project you configure directly is always
attempted, so a typo surfaces as an error.

### Choose settings

The seed's attributes select which slice of the project is read:

```toml
[target.attrs]
project = "App.xcodeproj"
configuration = "Release"
sdk_variant = "device"
```

- `configuration` picks the Xcode build configuration. It defaults to `Debug`,
  with test bundles built with their schemes' Test action configuration. When
  set, it applies to test bundles too, as `xcodebuild -configuration` does.
- `sdk_variant` selects `simulator` or `device` for targets on platforms other
  than macOS.
- `xcode_developer_dir` pins a `DEVELOPER_DIR` and includes it in the derived
  targets' cache keys.
- `resolver_inputs` overrides the text globs read during resolution when they
  should differ from `srcs`.
- `explicit_modules = true` enables explicit modules across the project's Swift
  targets and resolved packages, and `dependency_check = "error"` rejects
  undeclared workspace imports. Imports that Struo infers do not count as
  declarations. Native targets also honor `SWIFT_ENABLE_EXPLICIT_MODULES = YES`.
  See [explicit modules](/guide/graph/apple#explicit-modules-and-dependency-checks)
  for requirements and limits.

## What the resolver reads

Use this list to explain a target that resolves differently than expected:

- Layered build settings from the project, the target, and any `.xcconfig`
  files, including `#include` directives, `$(inherited)`, variable expansion,
  and conditional keys such as `SWIFT_FLAGS[sdk=iphonesimulator*]`.
- File references from classic build phases and from Xcode 16 file-system
  synchronized groups, including per-target membership exceptions and
  exclusion patterns.
- Schemes, which identify testable targets so a test bundle is wired to its
  host application.
- Shell script build phases (see [Script build phases](#script-build-phases)).
- Core Data models and Intents definitions, whose generated sources are
  compiled with the target.
- Swift package dependencies, both local packages in the repository and remote
  packages, built as Apple library targets. A project with both macOS and
  iPhone targets gets separate package targets for each destination. Local
  products referenced without a package reference are discovered from the
  repository's package manifests. Remote packages are pinned by the project's
  or workspace's `Package.resolved`. When the project commits no
  `Package.resolved`, Struo resolves the project's package requirements with
  `xcodebuild -resolvePackageDependencies`, which writes `Package.resolved` the
  way opening the project in Xcode does. A committed `Package.resolved` is never
  rewritten; a remote package it does not pin is reported as a warning.

## Script build phases

Struo runs imported shell script phases, including phases without declared
outputs, with their declared shell and the selected build configuration.
Preparation and source generators run before compilation. Scripts that feed
resource phases run after linking and before packaging. Product scripts run
after assembly and before final signing. Installation-only phases do not run
during `struo build`.

Project and target settings, including custom settings from configuration
files, are expanded into the script environment and take precedence over the
caller's environment. Directory settings such as `SRCROOT`, `PROJECT_DIR`,
`TARGET_BUILD_DIR`, `BUILT_PRODUCTS_DIR`, `DERIVED_FILE_DIR`, and
`TARGET_TEMP_DIR` point to Struo's workspace and products, not an Xcode derived
data directory. Product settings such as `EXECUTABLE_PATH`, `INFOPLIST_PATH`,
and `UNLOCALIZED_RESOURCES_FOLDER_PATH` describe the bundle Struo creates.
`DWARF_DSYM_FOLDER_PATH` and `DWARF_DSYM_FILE_NAME` identify the configured
symbol-bundle location. Dependency products are staged alongside the current
product for scripts that use `BUILT_PRODUCTS_DIR`.

Scripts receive the `SCRIPT_INPUT_FILE_*`, `SCRIPT_OUTPUT_FILE_*`, and
`SCRIPT_*_FILE_LIST_*` variables described in
[Apple's script documentation](https://developer.apple.com/documentation/xcode/running-custom-scripts-during-a-build).
Their input and output paths, including file-list contents, become graph inputs
and outputs.

Scripts run in the workspace, not an isolated sandbox, so caching depends on
complete declarations. A script with workspace-contained inputs and outputs can
be cached. A script with incomplete declarations, external inputs,
`alwaysOutOfDate`, or dependency analysis disabled runs every time, and its
target reports a cache bypass, although individual compile actions can still
hit the cache. Files that a product script creates or deletes are reflected in
the final bundle.

## Debug information

Compiled objects and debug maps record source and object paths relative to the
workspace root, and Swift modules leave out the compiler's search paths, the way
rules_swift and apple_support configure them. Builds from different checkouts
therefore produce identical outputs and share cache entries. A debugger
attached to a Struo-built product resolves those paths from the workspace root.
Because Swift modules leave out search paths, give LLDB the framework and
module directories of the product when evaluating expressions, with
`target.swift-framework-search-paths` and `target.swift-module-search-paths`.
To debug from elsewhere, map the root in LLDB:

```text
settings set target.source-map . /path/to/workspace
```

## Limitations

- Struo compiles the project itself rather than delegating to `xcodebuild`. It
  does not install dependencies or bootstrap external native toolchains, so run
  tools such as `pod install` first.
- Unresolved file-list settings generated by a dependency tool do not block
  graph import, but building the affected target reports the missing setup. A
  resolved file-list path that does not exist is an import error.
- Script ordering uses preparation, pre-package, and post-package stages.
  Projects that interleave repeated copy or resource phases with scripts may
  need explicit graph declarations to preserve finer ordering.
- App extensions build as `.appex` bundles embedded under `PlugIns`
  (`Contents/PlugIns` on macOS). Supply extension-point metadata through the
  extension's `Info.plist`. Embedded watch applications are built as
  application bundles, without their native embedded layout.
- Targets with no compilable sources, such as a script-only extension, are
  skipped, and dependency edges to them are dropped.

## Target kinds

| Xcode product | Struo target kind |
| --- | --- |
| Application, application extension | [`apple_application`](/reference/prelude/apple_application) |
| Framework | [`apple_framework`](/reference/prelude/apple_framework) |
| Static library, static framework | [`apple_library`](/reference/prelude/apple_library) |
| Unit and interface test bundle | [`apple_test_bundle`](/reference/prelude/apple_test_bundle) |
| Resource-only bundle | [`apple_resource_bundle`](/reference/prelude/apple_resource_bundle) |
| Referenced `.xcframework` | [`apple_xcframework_import`](/reference/prelude/apple_xcframework_import) |

The seed itself is an [`xcode_workspace`](/reference/prelude/xcode_workspace)
target.

## Next steps

- [Apple](/guide/graph/apple): declare the same target kinds directly and ship
  macOS applications.
- [Swift Packages](/guide/graph/swift-packages): understand how package
  dependencies are resolved and cached.
- [Testing and Scheduling](/guide/graph/testing): select and report test runs.
