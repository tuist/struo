# Typed Build Graph

Struo's typed build graph models a workspace as targets you can query, build,
run, test, and lint with caching. Start from an existing native project or
declare one target in `struo.toml`.

Each target has a kind that defines its attributes, dependencies, outputs, and
capabilities. Start small, inspect what Struo sees, and add dependencies or
specialized target kinds only when the project needs them.

## Start from a native project

If the repository already uses Cargo, Swift Package Manager, Xcode, Bazel, or
another supported build system, run Struo in its root without creating a file:

```sh
struo query workspace
struo query targets
struo build
struo test
struo lint
```

The query commands show what Struo derived from the native project. Before
loading it, follow the ecosystem guide to make the native lockfile and locked
dependency sources available, since loading may run the native resolver.

Without a target argument, `struo build`, `struo test`, and `struo lint` act on
the workspace's own targets, not the ones pulled in through resolved
dependencies. `struo build` stops at the first failing build. `struo lint` runs
every lint target to completion so you see every finding.

The default scope can miss a target that lives in the workspace but does not
feed its main products, such as a standalone macro plugin. Pass that target's
id explicitly, or pass `--all` to act on every capable target in the loaded
graph, resolved dependencies included:

```sh
struo build <target>
struo test --all
struo lint --all --fail-on error
```

`struo lint --all` also lints vendored third-party code and fails when any
finding meets `--fail-on`, so `--fail-on error` is a reasonable starting point
for a whole-graph sweep.

Discovery does not write `struo.toml`. The native project description and
lockfile stay authoritative for dependencies, products, tests, and releases. A
discovered target can sit beside explicit targets you declare for exceptional
build boundaries. An explicit target of the same kind takes precedence in its
package; unrelated targets do not hide the native integration.

An Xcode project or Swift package resolves into Apple applications,
frameworks, libraries, resource bundles, and test bundles, so you can build a
native target by name:

```sh
struo build MyApp
```

Continue with the guide for your build system:
[Rust](/guide/graph/rust), [Swift Packages](/guide/graph/swift-packages),
[Xcode Projects](/guide/graph/apple/xcode), [Bazel](/guide/graph/bazel), or
[Container Images](/guide/graph/containers) for an existing Dockerfile.

## Start with one target

Declare a target when the derived graph needs a project-specific boundary or
when no native project exists. Targets live in package-level `struo.toml`
files. This example declares an Apple library in `apps/ios/struo.toml`:

```toml
[[target]]
name = "AppCore"
kind = "apple_library"
srcs = ["Sources/**/*.swift"]

[target.attrs]
platform = "ios"
minimum_os = "17.0"
```

The manifest location and target name form the target id `apps/ios/AppCore`.
Query it before running any work:

```sh
struo query targets
struo query capabilities apps/ios/AppCore
struo query schema apple_library
```

The first command lists the workspace. The second shows what `AppCore` can do.
The third explains which attributes, dependencies, outputs, and capabilities
an `apple_library` accepts.

Build the target:

```sh
struo build apps/ios/AppCore
```

Outputs are materialized under `.struo/out/<target>/`. The
[target kind reference](/reference/prelude/) lists the output groups for each
kind.

Every built-in target kind also ships a complete starter with manifests and
source files. List the kinds, then materialize a starter and build it:

```sh
struo query target-kinds
struo edit materialize-example rust_binary rust-binary-minimal
struo build crates/hello/hello
```

Use `struo query example <kind> <slug> --format json` to inspect a starter's
files before writing them.

## Connect targets with dependencies

A dependency says that one target consumes the typed output of another. This
target can live beside `AppCore` in the same manifest:

```toml
[[target]]
name = "App"
kind = "apple_application"
srcs = ["AppSources/**/*.swift"]
deps = ["./AppCore"]

[target.attrs]
platform = "ios"
bundle_id = "dev.struo.App"
minimum_os = "17.0"
families = ["iphone"]
```

`./AppCore` resolves from the package that owns the manifest. `../` moves to a
parent package. References without either prefix resolve from the workspace
root. Struo validates each dependency against the target kind's contract, so
incompatible edges fail before a compiler or runner starts.

Restrict a target when only selected packages should consume it:

```toml
[[target]]
name = "InternalSupport"
kind = "rust_library"
visibility = ["package:apps/ios", "subtree:tests"]
```

An empty visibility list is public. `private` allows only the same package,
`package:` grants one package, `subtree:` grants a package hierarchy, and an
exact target id grants one consumer. `struo query validate-workspace` reports
a repair when a dependency crosses that boundary.

## Build, run, test, and lint

A capability is an operation a target exposes. The command is the same in
every ecosystem:

```sh
struo build apps/ios/AppCore
struo run apps/ios/App
struo test apps/ios/AppTests
struo lint quality/swift
```

- [`build`](/reference/cli/build) materializes an artifact.
- [`run`](/reference/cli/run) builds required outputs and starts the target,
  for example installing and launching an Apple or Android application on a
  simulator or device.
- [`test`](/reference/cli/test) builds and executes a test target.
- [`lint`](/reference/cli/lint) runs an analyzer and reports normalized
  findings.

Ask `struo query capabilities <target>` which operations a target supports
instead of guessing from its kind. Build actions replay from cache when their
declared inputs match; launch and device-test actions can opt out of replay
when each invocation must happen again.

For workspaces with several test targets, see
[Testing and scheduling](/guide/graph/testing). For analyzers, see
[Linting](/guide/graph/linting).

## Choose an ecosystem

A target's `kind` connects it to a typed contract for a language or platform.
Each ecosystem guide continues with runnable examples:

- [Apple](/guide/graph/apple)
- [Android](/guide/graph/android)
- [C and C++](/guide/graph/c)
- [Bazel](/guide/graph/bazel)
- [CMake](/guide/graph/cmake)
- [Container Images](/guide/graph/containers)
- [Elixir](/guide/graph/elixir)
- [Kotlin](/guide/graph/kotlin)
- [Go](/guide/graph/go)
- [Rust](/guide/graph/rust)
- [Zig](/guide/graph/zig)

The [Ecosystems guide](/guide/graph/ecosystems) compares these choices,
covers shared mobile code, and helps you decide when a typed target fits
better than a script.

## Select configuration-specific values

Some attributes accept `select`, which picks a value from the active
configuration. An Apple library can choose a framework by platform without
duplicating the target:

```toml
[target.attrs]
sdk_frameworks = { select = { ios = ["UIKit"], macos = ["AppKit"] } }
```

[Configurations](/guide/graph/configuration) covers workspace defaults,
per-invocation overrides, and dependency selection.

## Compare two fresh builds

Use `--verify-reproducible` to compare two fresh executions of a build instead
of checking whether a normal build can reuse its cached result:

```sh
struo build --verify-reproducible apps/service/Service
```

Struo snapshots the workspace and builds the target and its dependencies in two
private trials, each with an empty local cache. Your workspace outputs are not
replaced. Both trials must succeed, and differences in declared artifacts fail
the check. Differences only in captured logs are advisory when artifacts
match; when no artifacts are declared, logs decide the comparison.

The result prints a verification id. Inspect the retained trials:

```sh
struo query verification <ID>
struo query verification <ID> --format json
```

A passing check means the two trials agreed, not that the build is hermetic.
Host toolchains, absolute paths, network services, clocks, and randomness are
not isolated or perturbed. Only use this option when running the whole build
twice is acceptable. Reports and trial artifacts stay under
`.struo/verifications/<ID>/` and are not published. See
[Inspect retained trials](/guide/scripted/caching#inspect-retained-trials) and
the [verification report reference](/reference/mcp/#verification-reports).

## Extend the graph with local modules

Use a local module when a project needs a target kind that is not built in.
The root `struo.toml` can load checked-in [Starlark](https://starlark-lang.org/)
modules:

```toml
[modules]
paths = ["modules/*.star"]
```

Public symbols in those files become target kinds with the same schema,
dependency, capability, and validation surfaces as built-in kinds. Only the
root manifest can declare `[modules]`; paths resolve from the workspace root.
Confirm that a local kind loaded before declaring targets that use it:

```sh
struo query target-kinds
struo query schema my_target_kind
```

See the [Modules reference](/reference/modules/) to author a target kind.

## Give agents read access first

The [Model Context Protocol](https://modelcontextprotocol.io/) server lets an
agent discover targets, query schemas, and inspect target kind contracts
without changing workspace state:

```sh
struo mcp
```

Editing manifests, building, running, and testing are side-effectful, so Struo
lists those tools only when the server starts with an explicit opt-in:

```sh
struo mcp --allow-run
```

With the opt-in, agents can call `struo_build_target`, `struo_run_target`, or
`struo_start_target` with the same target id the command line accepts. The
start call returns a runtime session id, which the agent follows or stops with
`struo_runtime_status`, `struo_runtime_logs`, and `struo_stop_runtime`. See the
[Model Context Protocol tools reference](/reference/mcp/tools) for every
operation.

## Next steps

- [Ecosystems](/guide/graph/ecosystems): pick the target kinds for your
  language or platform.
- [Testing and scheduling](/guide/graph/testing): run tests, select affected
  work, and distribute batches on CI.
- [Linting](/guide/graph/linting): add cacheable analyzers with normalized
  findings.
- [Coding harnesses](/guide/harness): give an agent a full create, validate,
  and verify loop.
