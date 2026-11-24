# Ecosystems and Target Kinds

Choose the Struo target kinds for your language, platform, or build system,
import locked third-party dependencies, and adopt typed, cached builds one
boundary at a time.

An ecosystem is a set of target kinds for one language, platform, or build
domain. It gives Struo enough information to validate dependencies, explain
what a target can do, and cache work from declared inputs and outputs.

## Choose between a typed target and a script

Use an ecosystem target kind when Struo should understand the artifact and its
relationships. This is a good fit when you want to:

- query targets and capabilities before doing work;
- validate dependencies and attributes early;
- reuse build outputs across targets or machines;
- expose focused build, run, and test operations to coding agents.

Use a [script target](/guide/scripted/) when the workflow is still exploratory
or the native feature you need has not been modeled. Scripts still participate
in the graph, so a project can begin there and adopt typed targets one
boundary at a time.

An existing build system can stay in charge behind a typed target. For CMake,
[`cmake_project`](/reference/prelude/cmake_project) wraps configure and build
as one coarse action, while
[`cmake_workspace`](/reference/prelude/cmake_workspace) imports a checked
configured graph for queries. CMake remains authoritative in both cases.

## Find your ecosystem guide

Each full guide starts from a first project, queries before building, and
lists current limitations:

- [Bazel](/guide/graph/bazel) builds and tests an existing Bazel workspace
  without a Struo manifest while Bazel keeps its native graph and cache.
- [Apple](/guide/graph/apple) covers libraries, frameworks, applications, and
  tests written in Swift, Objective-C, C, and C++.
- [Xcode Projects](/guide/graph/apple/xcode) derives Apple targets from an
  existing `.xcodeproj` or `.xcworkspace` instead of declaring them.
- [Swift Packages](/guide/graph/swift-packages) covers locked package import,
  static products, and Apple consumers.
- [Android](/guide/graph/android) covers resources, Java and Kotlin libraries,
  application packages, and host or device tests.
- [C and C++](/guide/graph/c) covers headers, source compilation, static
  libraries, and native consumers.
- [CMake](/guide/graph/cmake) covers coarse native execution, exact products,
  checked graph snapshots, and incremental adoption.
- [Container Images](/guide/graph/containers) builds an existing Dockerfile as
  a discovered image target.
- [Elixir](/guide/graph/elixir) covers compiled applications and ExUnit tests.
- [Kotlin](/guide/graph/kotlin) covers Java virtual machine libraries and
  binaries, Android Kotlin sources, and Kotlin/Native Apple frameworks.
- [Go](/guide/graph/go) covers package archives, executables, tests, cgo,
  cross-compilation, and locked vendored modules.
- [Rust](/guide/graph/rust) starts from an existing `Cargo.toml`, then covers
  libraries, binaries, tests, procedural macros, Cargo dependencies, and
  native mobile outputs.
- [React Native](/guide/graph/react-native) covers New Architecture
  dependencies, native modules, JavaScript bundles, Apple and Android
  applications, and Fast Refresh.
- [Nx](/guide/graph/nx) reads an existing Nx workspace and runs its
  `nx:run-commands` and `nx:run-script` tasks through Struo.
- [Turborepo](/guide/graph/turborepo) reads an existing Turborepo workspace
  and runs each package script through Struo, locally or on remote workers.
- [Zig](/guide/graph/zig) covers modules, binaries, tests, libraries, and C or
  C++ dependencies.

Test-only integrations have focused reference pages with declarations,
starters, and first-run commands:

- [pytest](/reference/prelude/pytest_test) for Python.
- [Ruby Specification](/reference/prelude/rspec_test) and
  [Minitest](/reference/prelude/minitest_test) for Ruby.
- [Vitest](/reference/prelude/vitest_test) and
  [Jest](/reference/prelude/jest_test) for JavaScript and TypeScript.

The [target kind index](/reference/prelude/) lists the schema, dependencies,
capabilities, outputs, and limitations of every kind.

## Import locked third-party graphs

Dependency targets do not replace package managers. The native manager owns
manifest semantics, version selection, registry policy, and lock updates.
Struo imports the locked result, creates one queryable target per package
instance, and connects those targets to first-party consumers.

| Ecosystem | Import target | Locked input | First-party consumer |
| --- | --- | --- | --- |
| Go | [`go_dependencies`](/reference/prelude/go_dependencies) | `go.mod`, `go.sum`, and `vendor/modules.txt` | `go_library`, `go_binary`, or `go_test` |
| Rust | [`cargo_dependencies`](/reference/prelude/cargo_dependencies) | `Cargo.lock` and Cargo metadata | `rust_library`, `rust_binary`, or `rust_test` |
| Elixir | [`mix_dependencies`](/reference/prelude/mix_dependencies) | `mix.lock` and the active Mix graph | `elixir_library` |
| Swift | [`swift_package_dependencies`](/reference/prelude/swift_package_dependencies) | `Package.resolved` and a package graph snapshot | Apple libraries, frameworks, applications, or tests |
| React Native | [`react_native_dependencies`](/reference/prelude/react_native_dependencies) | `package.json`, an npm, pnpm, Yarn, or Bun lockfile, and an optional native-module snapshot | Metro bundles or Apple and Android applications |
| Zig | [`zig_dependencies`](/reference/prelude/zig_dependencies) | `build.zig.zon` package records | Zig modules, binaries, libraries, or tests |

Updating dependencies is separate from an ordinary build:

1. Use the native manager to update the manifest and lock file.
2. Fetch or vendor the exact sources and verify their native integrity values.
3. Query the imported package targets before building a consumer.
4. Build or run a first-party target whose `deps` includes the dependency set.

Ordinary builds never update lock files, and source contents always remain
part of action keys. Cargo, Mix, and Zig expect exact sources to be present
before the build. Swift supports local packages or a complete vendored
package-manager scratch tree, which keeps network access out of compile
actions. Inspecting an unvendored remote Swift package requires an explicit
`allow_network = true`, and Struo does not sandbox Swift Package Manager
network access when a vendored scratch tree is incomplete.

## Adopt an ecosystem incrementally

1. Pick the smallest artifact that has stable inputs and outputs.
2. Declare one target and run `struo query schema <kind>`.
3. Run `struo query capabilities <target>` and build that same target.
4. Connect one consumer through `deps` and query the resulting graph.
5. Add run or test targets only after the artifact build is reliable.
6. Keep unsupported edges behind scripts until a typed target kind covers
   them.

The native package manager or build system stays in place for the parts Struo
does not own, so a project does not need to move an entire ecosystem at once.

A coding harness can also read an external rule or plugin and maintain a
narrow local target kind for the project. See
[Coding harnesses](/guide/harness#adopt-an-unfamiliar-external-rule).

## Share code across mobile platforms

Some target kinds cross platform boundaries while keeping normal graph
dependencies:

- [`swift_android_library`](/reference/prelude/swift_android_library) packages
  Swift code for an Android application.
- [`kotlin_apple_framework`](/reference/prelude/kotlin_apple_framework)
  produces a Kotlin/Native framework for an Apple target.
- [`rust_mobile_library`](/reference/prelude/rust_mobile_library) produces the
  Apple or Android native library variant requested by its consumer.

The `native-mobile-shared-code-e2e` starter wires an Android application to
Swift and Rust native libraries, and an Apple application to a Kotlin/Native
framework and the same Rust library. Add shared code only after the
application builds without it, so toolchain and linking problems stay separate
from the first graph setup.

## Check the boundary before adopting

Feature coverage varies by ecosystem. Before moving a production workflow,
check the target kind reference for:

- supported source, resource, and dependency shapes;
- ownership of third-party dependency resolution;
- required compilers and platform tools;
- cacheable artifacts and non-cacheable runtime effects;
- unsupported attributes and whether they fail validation;
- editor or language-service integration the existing workflow still needs.

Unsupported does not have to mean blocked. Keep that behavior in a script,
define a checked-in local module, or contribute the missing typed behavior.

## Report a native graph that loads wrong

When a graph derived from native manifests fails to load, or its targets,
sources, or dependency edges disagree with the native build system, export a
redacted reproduction:

```sh
struo query graph-repro
```

The export lands in `.struo/graph-repro`:

- `workspace/` mirrors every discovered native project. Manifests and
  lockfiles keep their contents; every other file is empty at the same path.
  Credentials in URLs are replaced with `<redacted>`.
- `repro.json` records the Struo version, host platform, and the loaded graph
  or loading error, with workspace and home paths and environment attribute
  values redacted.
- `ISSUE.md` drafts an issue for the
  [Struo repository](https://github.com/tuist/struo/issues), and
  `graph-repro.tar.gz` packs the export to attach to it.

The command prints a link that opens the issue prefilled; nothing is shared
until you open it. Review the copied files and `repro.json` first, since a
manifest can still name a private host or hold a secret the export did not
recognize. Then describe what the native build system does differently and
attach the archive in the browser. With the GitHub CLI:

```sh
gh issue create --repo tuist/struo --title "..." --body-file .struo/graph-repro/ISSUE.md
```

Agents connected through `struo mcp --allow-run` can run the same export with
the `struo_export_graph_repro` tool, and resolver errors tell them to offer it
instead of changing your project to work around the gap.

## Next steps

- [Typed Build Graph](/guide/graph/): declare a first target and connect
  dependencies.
- [Testing and scheduling](/guide/graph/testing): run tests across these
  ecosystems with affected selection and batching.
- [Target kind index](/reference/prelude/): look up the exact contract of a
  kind.
