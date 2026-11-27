# Rust and Cargo

Build, run, test, and cache Rust projects with Struo, straight from an existing
`Cargo.toml` or from Rust targets declared in `struo.toml`.

Struo reads a Cargo workspace without any configuration and caches each
workspace package and locked dependency separately. Hand-authored targets add
standalone crates, cross-language links, and native mobile libraries.

## Prerequisites

Struo invokes `rustc` and `cargo` from the selected toolchain. Confirm that the
versions the project expects are available:

```sh
rustc --version
cargo --version
```

If the repository pins them with [mise](https://mise.jdx.dev/), install that
configuration first:

```sh
mise install
mise exec -- rustc --version
mise exec -- cargo --version
```

Host binaries and tests also need the platform linker that the Rust compiler
selects. Cross-compiled and mobile outputs need the linker and Rust target
support for their destination platform.

## Start from an existing Cargo project

From the directory that contains the root `Cargo.toml`, build and test the
workspace without creating `struo.toml`:

```sh
struo query targets
struo build
struo test
```

The build reports independently cached package and dependency actions as it
runs. Targetless `struo test` runs first-party tests and skips test targets
that belong only to external crates. Use `struo test --all` to run every test
target in the resolved graph.

Struo asks Cargo for package metadata while loading the graph. An existing
`Cargo.lock` stays authoritative and is used in locked mode. Without a
lockfile, Cargo resolves dependencies and creates one, so the first load can
access the network and fill Cargo's local source cache. Struo does not read or
modify a repository's `vendor` directory unless you set `vendor_dir` on an
explicit seed.

### Inspect the derived graph

Narrow the graph by target kind to see the first-party targets:

```sh
struo query workspace
struo query targets --kind rust_binary
struo query targets --kind rust_library
struo query targets --kind rust_test
```

These commands do not write a manifest. Discovery emits first-party libraries,
binaries, procedural macros, unit and integration tests, build-script edges,
and locked external packages. Cargo stays authoritative for workspace
membership, default members, features, and resolved versions.

The identifiers printed by `struo query targets` are the source of truth. They
carry the package and the Cargo target role: for a package named `hello` with a
binary also named `hello`, discovery normally derives `cargo_hello_bin_hello`
and `cargo_hello_bin_hello_unit_tests`.

Targets gated by Cargo `required-features` appear only when every required
feature is selected.

### Build, run, and test

Use a generated identifier to build, run, or test one target:

```sh
struo build cargo_hello_bin_hello
struo run cargo_hello_bin_hello
struo test cargo_hello_bin_hello_unit_tests
```

Ask Struo what a target supports when its role is not obvious:

```sh
struo query capabilities cargo_hello_bin_hello
```

Outputs are materialized under `.struo/out/<target>/`. Generated tests run
from their package root, receive a `CARGO_BIN_EXE_<name>` entry for every
binary in their package, and see the package's `CARGO_*` variables. Entries in
the `[env]` table of Cargo configuration reach the compiler, build scripts,
tests, and `struo run`.

### Confirm caching

Run the same build twice without changing an input:

```sh
struo build cargo_hello_bin_hello
struo build cargo_hello_bin_hello
```

The second invocation restores unchanged actions from the configured cache.
Changing a locked dependency invalidates that package and its consumers.
Changing a workspace package invalidates that package and its dependants.
Changing only a test file reruns the affected test without recompiling an
unchanged library. Build-script outputs that embed timestamps or other
changing bytes still invalidate their consumers.

Continue with [Caching](/guide/scripted/caching) to configure a shared remote
cache.

### Keep Cargo commands

Struo can sit behind the `cargo` command for one Cargo project or workspace.
Add this mise wrapper, run `mise reshim`, and activate mise in the shell that
starts the build:

```toml
[wrappers.cargo]
command = "struo"
args = ["cargo", "--"]
```

Struo then routes the default debug build and test forms into the graph:

```sh
cargo build
cargo test
```

`cargo build` builds the workspace and its resolved products. `cargo test`
runs each first-party test target once through Struo. `-q` or `--quiet`,
`--locked`, `--offline`, `--frozen`, and `--manifest-path Cargo.toml` are also
supported.

Without mise, call the compatibility surface directly and put the separator
before the Cargo arguments:

```sh
struo cargo -- test
```

Every other request, including release builds, feature or package selection,
cross-compilation targets, `cargo check`, `cargo run`, `cargo clippy`, and a
manifest path outside the workspace root, runs the system `cargo` with its
original arguments and exit status. See [`struo cargo`](/reference/cli/cargo).

### Select features or a compilation target

Declare an explicit `cargo_workspace` seed in `struo.toml` to select Cargo
features or a destination target without copying the package graph into the
manifest:

```toml
[[target]]
name = "cargo"
kind = "cargo_workspace"

[target.attrs]
features = ["workspace-package/feature-name"]
target = "aarch64-unknown-linux-gnu"
```

The [`cargo_workspace` reference](/reference/prelude/cargo_workspace) lists
every seed attribute, including `vendor_dir`, `build_script_tools`, and
`test_env_inherit`.

### Build scripts that read sibling packages

An external package's build script sometimes reads another package's source
instead of linking its compiled library. Declare those files on the
`cargo_workspace` or `cargo_dependencies` seed:

```toml
[[target]]
name = "cargo"
kind = "cargo_workspace"

[target.attrs.build_script_sources."shader-backend@0.1.0"]
"../graphics/src/scene.rs" = "graphics@0.2.2/src/scene.rs"
"../graphics/src/color.rs" = "graphics@0.2.2/src/color.rs"
```

The outer key selects an external package with a build script, as `name` or
`name@version`. Each entry maps a destination relative to that package's
`CARGO_MANIFEST_DIR` to a file in a resolved Cargo package:

- Selectors must match exactly one package in the locked graph. Ambiguous
  versions or origins are rejected, and unknown consumer selectors fail. A
  mapping for a consumer that is inactive on the current platform is skipped.
- The consumer must be an external package. The source package can be a
  workspace member.
- Destinations have the form `../sibling/path`, using ASCII letters, digits,
  `.`, `_`, and `-`. Further traversal, absolute paths, backslashes, Windows
  device names such as `NUL`, and destinations that collide after case
  normalization are rejected.
- Sources have the form `package@version/path`, without traversal. Declare
  individual files, not directories or globs.

Each mapped file is an action input, so editing it reruns the build script
even on a warm build. These declarations do not change features, dependency
aliases, or linking edges, and neither Cargo's source cache nor a vendor tree
is modified.

### What rebuilds a crate

Like Cargo, Struo rebuilds a workspace crate when a file the compiler read
changes, not when any file in its package does. The compiler reports what it
read, including files embedded with `include_str!` or `include_bytes!`, and
each cached result keeps that list with the digest of every file on it.
Editing `docs/` or CI files in a package at the workspace root leaves its
crate cached; editing a file the crate embeds rebuilds it.

A build script reruns when a path it prints with `rerun-if-changed` changes,
inside its package or anywhere else in the workspace, such as shared protobuf
definitions, `.git/HEAD`, or a sibling crate's directory. A script that prints
no `rerun-if-changed` or `rerun-if-env-changed` depends on its whole package,
as in Cargo.

This applies to local runs without a sandbox. A sandboxed or remote run keys
every file in the package, since only declared files are staged for it.

### Build scripts that read files outside their package

A build script that reads files outside its package needs them staged when it
runs in a sandbox or remotely, and `struo query affected-tests` needs to know
about them to select the right tests. Struo finds such paths in the script's
string literals: a `../` path such as `"../../proto"`, or a name joined onto
one, as in `manifest_dir.join("../../..").join("proto")`. When a script builds
a path another way, declare it on the seed, with paths relative to the Cargo
workspace root:

```toml
[[target]]
name = "cargo"
kind = "cargo_workspace"

[target.attrs]
build_script_data = { "api-types" = ["proto/**"] }
```

`struo query affected-tests` treats these files as inputs of the package
whose build script reads them, so changing one selects the tests of that
package and of every package that depends on it.

### Which files select a package's tests

`struo query affected-tests` selects a Cargo package's tests, and those of the
packages that depend on it, when one of these changes:

- a Rust file anywhere in the package;
- a file in a directory that holds one of its targets, such as `src/`,
  `tests/`, `benches/`, or `examples/`, which covers snapshots and fixtures
  kept beside the code;
- its `Cargo.toml`, `Cargo.lock`, `.cargo/` configuration, or
  `rust-toolchain` file.

The root `Cargo.toml` of a virtual workspace, which belongs to no package,
selects the tests of every workspace member.

Any other file selects a target's tests only when that target's Rust files
name it: its path, relative to the workspace, the package, or a directory
above the package, as `include_str!("../dev/example.toml")` or a test opening
`"docs/reference/cli.md"` does, or a directory it lies in, written as a path,
such as `"docs/reference"`, `"./migrations"`, or `"fixtures/**/*.json"`. A
path only one integration test names selects that test alone; a helper the
tests share should live in a file that is no target's root, such as
`tests/common/mod.rs`, so that every test sees what it names. For a package
at the workspace root, editing `docs/`, `README.md`, or a script in `dev/`
that no Rust file names leaves its tests out of the selection. A package with
a build script, or with a target whose source file sits at the package root,
keeps its whole directory, since either may read any file in it.

A file reached through a name the code never writes as a path is not seen: a
directory a procedural macro reads by convention with no argument, or a bare
name joined at run time, such as `dir.join("fixtures").join(name)`. Keep such
files in a directory that holds a target, such as `tests/`, or write the
directory as a path.

### Profiles and native build parallelism

Struo reads Cargo's `dev` profile, and the `test` profile for test targets,
including per-package overrides and `build-override`, and passes the
optimization level, debug info, debug assertions, overflow checks, and codegen
units to the compiler. A package built with `opt-level = 3` gets the same
setting under Struo, and its build script sees `OPT_LEVEL=3`. Procedural macros
stay unoptimized unless `build-override` or a per-package override says
otherwise, as in Cargo.

A build script that depends on `cc`, `cmake`, or `autotools` receives
`NUM_JOBS=4`, which those crates use for parallel native compiles, and its run
reserves four CPU slots. Other build scripts get one job and one slot. Set
`build_script_jobs` on the seed to change the count, for example when remote
workers have fewer CPUs.

## Choose how much configuration to own

Native discovery, manifests, and Starlark modules are composable layers. A
project does not need to migrate away from `Cargo.toml` to gain more control.

| Layer | Repository change | Use it when |
| --- | --- | --- |
| Native Cargo project | None | The graph derived from `Cargo.toml` already describes the build. |
| Explicit native seed | One `cargo_workspace` target in `struo.toml` | The repository should configure features, target selection, caching, or execution infrastructure. |
| Explicit typed targets | Additional targets in `struo.toml` | A package needs a custom boundary, cross-language dependency, mobile artifact, or operation that Cargo metadata does not describe. |
| Project Starlark module | A registered reusable target kind | Several targets need the same new behavior and an existing target kind cannot express it. |

Start with native discovery and stop there when it is sufficient. An explicit
seed can live beside additional targets, so Cargo keeps owning its package
graph while Struo owns only the exceptional edges. Do not restate generated
Cargo packages in `struo.toml`. The [Modules reference](/reference/modules/)
covers authoring a custom target kind.

## Declare targets

Hand-authored targets suit a standalone Rust library without Cargo,
cross-language links, native mobile outputs, or a boundary that should be
cached independently.

### Declare a library, binary, and test

Create `apps/hello/struo.toml`:

```toml
[[target]]
name = "greeting"
kind = "rust_library"
srcs = ["src/lib.rs"]

[target.attrs]
crate_name = "greeting"
edition = "2021"

[[target]]
name = "hello"
kind = "rust_binary"
srcs = ["src/main.rs"]
deps = ["./greeting"]

[target.attrs]
crate_name = "hello"
edition = "2021"

[[target]]
name = "greeting_tests"
kind = "rust_test"
srcs = ["tests/greeting_test.rs"]
deps = ["./greeting"]

[target.attrs]
crate_name = "greeting_tests"
crate_root = "tests/greeting_test.rs"
edition = "2021"
labels = ["unit"]
```

Use this source layout:

```text
apps/hello/
├── struo.toml
├── src/
│   ├── lib.rs
│   └── main.rs
└── tests/
    └── greeting_test.rs
```

The binary and test refer to the library as `greeting` in Rust source; their
`./greeting` dependency supplies the built crate.

### Query before building

Inspect the targets and their capabilities:

```sh
struo query targets --kind rust_library
struo query capabilities apps/hello/greeting
struo query capabilities apps/hello/hello
struo query capabilities apps/hello/greeting_tests
struo query schema rust_binary
```

The library exposes `build`, the binary exposes `build` and `run`, and the test
target exposes `build` and `test`.

### Build, run, and test targets

Build the binary. Struo builds `greeting` first because the binary depends on
it:

```sh
struo build apps/hello/hello
```

Run the binary and the tests:

```sh
struo run apps/hello/hello
struo test apps/hello/greeting_tests
```

The [`rust_binary`](/reference/prelude/rust_binary) and
[`rust_test`](/reference/prelude/rust_test) references list their outputs.
`rust_binary` accepts `args`, `run_env`, and `env_inherit` for runtime
configuration. `data` files are run inputs, while `compile_data` files affect
compilation.

## Manage dependencies

Skip this section when the native `cargo_workspace` graph is sufficient; it
already imports the complete locked dependency graph.

### Keep Cargo as the authority for third-party crates

Use `cargo_dependencies` when hand-authored Rust targets should consume
packages resolved by Cargo. Keep requirements in `Cargo.toml` and exact
versions in `Cargo.lock`:

```toml
[[target]]
name = "cargo_dependencies"
kind = "cargo_dependencies"
srcs = [
  "Cargo.toml",
  "Cargo.lock",
  ".cargo/config.toml",
  "apps/*/Cargo.toml",
]

[target.attrs]
manifest = "Cargo.toml"
lockfile = "Cargo.lock"
resolver_inputs = [
  "Cargo.toml",
  "Cargo.lock",
  ".cargo/config.toml",
  "apps/*/Cargo.toml",
]
vendor_dir = "third_party/rust/vendor"
packages = ["itoa"]
```

Add that target to a first-party Rust target:

```toml
[[target]]
name = "hello"
kind = "rust_binary"
srcs = ["src/main.rs"]
deps = ["./greeting", "cargo_dependencies"]

[target.attrs]
crate_name = "hello"
edition = "2021"

[target.attrs.rustc_env]
CARGO_MANIFEST_DIR = "apps/hello"
CARGO_PKG_NAME = "hello"
CARGO_PKG_VERSION = "0.0.0"
```

While loading the graph, the dependency target runs
`cargo metadata --locked --offline`. Registry and Git packages come from the
vendor directory, so configure Cargo to use the same sources and keep that
configuration in `resolver_inputs`:

```toml
[source.crates-io]
replace-with = "vendored-sources"

[source.vendored-sources]
directory = "third_party/rust/vendor"
```

Each resolved package becomes a `rust_crate` or `rust_proc_macro` target that
builds independently. Workspace and path packages stay first-party targets.
Inspect the imported packages, then build and run the consumer:

```sh
struo query targets --kind rust_crate
struo query targets --kind rust_proc_macro
struo build apps/hello/hello
struo run apps/hello/hello
```

The bundled Cargo starter uses `itoa` from the locked graph and prints `42`.

To avoid running Cargo during graph loading, check in a metadata snapshot,
add it to `srcs` and `resolver_inputs`, and set `metadata_file`. Refresh it
when dependency inputs change:

```sh
cargo metadata --format-version 1 --locked --offline > cargo-metadata.json
```

A snapshot needs `struo_snapshot` provenance, and a cross-compiled snapshot
also needs a host snapshot in `host_metadata_file`. Stale snapshots are
rejected during graph loading. The
[`cargo_dependencies` reference](/reference/prelude/cargo_dependencies)
documents the snapshot format and the feature and target filters.

### Use named dependency roles

Use named roles when a dependency has compile-time semantics that differ from
an ordinary Rust crate:

```toml
[[target]]
name = "hello"
kind = "rust_binary"
srcs = ["src/main.rs"]
deps = ["./greeting"]

[target.dependencies]
proc_macro_deps = ["./derive_greeting"]
link_deps = ["./native_support"]
```

`proc_macro_deps` accepts procedural macros built for the execution host.
`link_deps` accepts C providers such as [`c_library`](/reference/prelude/c_library)
and applies their libraries and linker options to final artifacts. Named roles
let Struo report a provider in the wrong role before analysis.

### Use build scripts and compiler settings

Rust targets can set `build_script` to compile and run a Cargo-style build
script before the main crate. Struo provides `OUT_DIR` and applies the
compiler configuration, environment, and linking directives the script prints.

Compiler flags, environment files, linker settings, crate aliases, and feature
selection are documented in each target kind reference. Query the schema before
choosing an attribute:

```sh
struo query schema rust_library
```

## Produce native mobile libraries

Use [`rust_mobile_library`](/reference/prelude/rust_mobile_library) when the
same sources feed both Apple and Android consumers:

```toml
[[target]]
name = "SharedRust"
kind = "rust_mobile_library"
deps = ["./SharedCore"]
srcs = ["src/shared/**/*.rs"]

[target.attrs]
crate_name = "shared_rust"
apple_target = "aarch64-apple-ios"
android_target = "aarch64-linux-android"
android_abi = "arm64-v8a"
android_api = 24

[[target]]
name = "SharedCore"
kind = "rust_mobile_library"
srcs = ["src/core/**/*.rs"]

[target.attrs]
crate_name = "shared_core"
apple_target = "aarch64-apple-ios"
android_target = "aarch64-linux-android"
android_abi = "arm64-v8a"
```

An Apple consumer requests a static library. An Android consumer requests a
shared library packaged for the configured
[Application Binary Interface](https://developer.android.com/ndk/guides/abis).
Android linking requires the
[Android Native Development Kit](https://developer.android.com/ndk), found
through `ANDROID_NDK_HOME` or `android_ndk`. Dependencies between
`rust_mobile_library` targets compile for whichever platform the consumer
requests.

## Troubleshooting

- If Cargo cannot resolve or download a source, fix the Cargo error and retry
  the Struo command.
- If a binary or example is absent, check its `required-features` and select
  those features on an explicit seed.
- A build script that invokes a host tool needs that tool in
  `build_script_tools`. The default list covers the tools that `-sys` packages
  conventionally use. When the tool on the search path is a mise shim, Struo
  passes the binary mise would run, since the shim does not work in the build
  script's cleared environment. A tool mise does not select for the workspace
  is left out.
- Tests and `struo run` start with a cleared environment and a private `HOME`.
  Name host variables a generated test needs in `test_env_inherit`, or in
  `env_inherit` on an explicit target.
- A test that reads `CARGO_BIN_EXE_<name>` with the `env!` macro pins the path
  at compile time, which breaks under sandboxing or remote execution. Read the
  variable at run time instead.
- Cargo configuration files take part in graph resolution. Keep the
  repository's source replacement and target configuration available when
  loading the graph.

## Limitations

- Rust tests run only host-target executables. A cross-target test can be
  built, but running it needs a platform runner that the target kinds do not
  provide.
- `struo query affected-tests` selects tests from the directories a package's
  targets live in and the paths its Rust files name, not from what the
  compiler reported reading. A file a procedural macro or a test reaches
  through a path it never writes out in full selects nothing unless it sits
  in one of those directories.
- Only the `dev` and `test` profiles are read. `--release` and custom
  profiles, `panic`, `lto`, and `strip` settings are not applied. As in Cargo,
  objects leave out LLVM bitcode unless the profile enables `lto`, which keeps
  build outputs and the cache roughly half the size.
- A `debug` setting is applied only when a profile names it. Without one,
  crates compile without debug info, while Cargo's `dev` profile includes it.
- Build scripts themselves compile without profile settings, so
  `build-override` reaches procedural macros and host crates but not the
  build script binaries.
- Compatibility attributes listed as unsupported in a target kind reference
  fail validation when set to a non-empty value.

## Target kinds

- [`cargo_workspace`](/reference/prelude/cargo_workspace)
- [`cargo_dependencies`](/reference/prelude/cargo_dependencies)
- [`rust_library`](/reference/prelude/rust_library)
- [`rust_binary`](/reference/prelude/rust_binary)
- [`rust_test`](/reference/prelude/rust_test)
- [`rust_proc_macro`](/reference/prelude/rust_proc_macro)
- [`rust_crate`](/reference/prelude/rust_crate)
- [`rust_mobile_library`](/reference/prelude/rust_mobile_library)

## Next steps

- [Caching](/guide/scripted/caching): share build results across machines
  with a remote cache.
- [Testing and Scheduling](/guide/graph/testing): select and schedule tests
  across the graph.
- [Android](/guide/graph/android) or [Apple Apps and Libraries](/guide/graph/apple):
  consume a Rust library from a mobile application once that app builds on
  its own.
- [Memory](/guide/memory/): inspect the durable context Struo records for
  builds and tests.
