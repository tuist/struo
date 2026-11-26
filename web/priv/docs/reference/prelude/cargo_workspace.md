# `cargo_workspace`

Native Cargo project seed.

## Description

`cargo_workspace` runs Cargo metadata and emits ordinary first-party and
external Rust targets. An existing `Cargo.lock` is used in locked mode. When it
is absent, Cargo resolves dependencies and creates it. First-party packages become
`rust_library`, `rust_binary`, `rust_test`, or `rust_proc_macro` targets.
Locked external packages use the same fine-grained lowering as
`cargo_dependencies`.

Cargo remains authoritative for workspace membership, targets, features,
renamed dependencies, build scripts, versions, and checksums. By default, Struo
snapshots locked external package trees from Cargo's local cache into each
target's output. The repository stays unchanged. A pre-existing vendored source
directory can be selected explicitly.
Legacy lockfiles retain their checksum metadata without requiring a lockfile
format upgrade. Registry packages still require a locked checksum.
Feature resolution ignores Cargo terminal coloring, so presentation settings
do not change the imported feature set or action keys.
Targets whose `required-features` are not selected are omitted, matching
Cargo's target selection. Generated tests, benchmarks, and examples include
development dependencies. Procedural-macro unit tests retain access to the
compiler-provided `proc_macro` crate. Cargo doctests are not imported; run them
with Cargo separately. Each generated test runs from its own package root
and receives a `CARGO_BIN_EXE_<name>` entry for every binary in its package,
matching what Cargo gives an integration test. Entries in the `[env]` table of
Cargo configuration reach the compiler, the build scripts, the test processes,
and `struo run`.

Each package's sources are its directory without the local packages Cargo
resolved inside it, its `target` directory, Cargo's configured target
directory, and, for a package at the workspace root, `.git`. A crate inside the
package that no manifest refers to, such as a test fixture, stays a source.

Packages are compiled with Cargo's `dev` profile, and test targets with the
`test` profile, read from the root manifest and Cargo configuration. Overrides
apply as in Cargo: `build-override` for procedural macros and the crates built
for the execution host, then `[profile.dev.package."*"]` for packages outside
the workspace, then `[profile.dev.package.<name>]` and
`[profile.dev.package."<name>@<version>"]`. The `opt-level`, `debug`,
`debug-assertions`, `overflow-checks`, and `codegen-units` settings become
compiler flags. Build scripts see the matching `OPT_LEVEL`, `DEBUG`, and
`CARGO_CFG_DEBUG_ASSERTIONS`, and, as in Cargo, profile settings stay out of
`CARGO_ENCODED_RUSTFLAGS`.

Packages are compiled once for the execution host. A second, host-only build
of a package appears only when it would differ: when `target` names something
other than the host, or when `dep_rustc_flags` carries a panic strategy that
compiler plugins and build scripts must not inherit. Cargo target names are normalized to valid Rust crate
identifiers while binary names retain their manifest spelling. Multi-output
libraries emit one target per declared Rust library crate type. Cargo's
workspace member list determines which packages are first-party, so local path
dependencies outside that list remain ordinary dependency targets.

## Attributes

| Attribute | Type | Required | Default | Description |
| --- | --- | --- | --- | --- |
| `manifest` | string | no | `Cargo.toml` | Package-relative Cargo manifest |
| `lockfile` | string | no | `Cargo.lock` | Package-relative authoritative lockfile |
| `resolver_inputs` | list&lt;string&gt; | no | `srcs` | Text inputs available while deriving the graph |
| `metadata_file` | string | no |  | Optional checked Cargo metadata snapshot |
| `host_metadata_file` | string | no |  | Optional host metadata snapshot for cross-compilation |
| `vendor_dir` | string | no | Struo-managed | Optional package-relative pre-vendored external sources |
| `build_script_sources` | map&lt;string, map&lt;string, string&gt;&gt; | no | `{}` | External consumer selector to sibling destination to locked package source file; see [sibling sources](/guide/graph/rust/#build-scripts-that-read-sibling-packages) |
| `build_script_data` | map&lt;string, list&lt;string&gt;&gt; | no | `{}` | Workspace package name to globs, relative to the Cargo workspace root, of files outside the package that its build script reads, added to the paths Struo finds in the script itself; needed for sandboxed and remote runs and test selection only; see [build scripts that read files outside their package](/guide/graph/rust/#build-scripts-that-read-files-outside-their-package) |
| `features` | list&lt;string&gt; | no | `[]` | Selected Cargo features |
| `all_features` | bool | no | `false` | Select every Cargo feature |
| `no_default_features` | bool | no | `false` | Disable default Cargo features |
| `target` | string | no | host | Destination Rust target triple |
| `dep_rustc_flags` | list&lt;string&gt; | no | `[]` | Additional flags for external packages |
| `build_script_jobs` | int | no | `4` | `NUM_JOBS` for package build scripts that depend on `cc`, `cmake`, or `autotools`, reserved as CPU slots for their runs; other build scripts get one |
| `build_script_tools` | list&lt;string&gt; | no | `cmake`, `nasm`, `perl`, `pkg-config`, `protoc`, `python3` | Host tool names package build scripts may invoke; each is resolved on the search path during graph loading |
| `test_env` | map&lt;string, string&gt; | no | `{}` | Environment variables applied to generated tests after inherited variables |
| `test_env_inherit` | list&lt;string&gt; | no | `[]` | Host environment variable names explicitly inherited by generated tests |
| `test_cacheable` | bool | no | `true` | Cache successful generated test results; disable for tests depending on undeclared host tools or configuration |
| `coverage_per_test` | bool | no | `false` | Instrument first-party products and enable isolated coverage on generated tests with declared product sources |
| `coverage_sources` | list&lt;string&gt; | no | `[]` | Product Rust source globs forwarded to generated test targets |
| `llvm_cov` | string | no | compiler llvm-tools | Compiler-matching coverage exporter |
| `llvm_profdata` | string | no | compiler llvm-tools | Compiler-matching profile merger |

Coverage selection is opt-in and approximate. Keep product dependency sources
separate from test-registration inputs. A generated test's own crate sources
remain compatibility inputs, including product code declared beside its tests.
Changing those files retains work rather than assuming that coverage can prove
test registration unchanged. See [coverage selection](/guide/graph/testing#coverage-assisted-selection).

## Providers

The target emits `cargo_workspace`.

## Capabilities

| Capability | Output groups |
| --- | --- |
| `build` | none |

## Direct Use

On a fresh clone, the first graph load can access the network while Cargo
resolves or downloads dependencies. Struo then imports the pinned graph from
`Cargo.lock` and Cargo's local source cache.

Inspect the automatically derived graph:

```sh
struo query workspace
struo query targets
```

No `struo.toml` is required. To configure the resolver explicitly, author a
target equivalent to:

```toml
[[target]]
name = "cargo"
kind = "cargo_workspace"
srcs = ["Cargo.toml"]

[target.attrs]
resolver_inputs = ["Cargo.toml", "Cargo.lock", "**/Cargo.toml", ".cargo/config", ".cargo/config.toml"]
```

Keep this seed as the only Struo target while Cargo metadata describes the
complete build. Add explicit targets beside it for exceptional cross-language
or packaging boundaries. Use a project Starlark module only when those
exceptions share reusable behavior that the built-in target kinds cannot
express.

### Host-dependent tests

Generated tests use a restricted environment by default. Tests that invoke
Cargo or other host tools can opt into the required environment on the seed:

```toml
[target.attrs]
test_env_inherit = ["PATH", "CARGO_HOME", "RUSTUP_HOME"]
test_cacheable = false
```

Use `test_env` for explicit values, which take precedence over inherited
variables. Inheriting a tool search path does not declare the tools or their
external state as inputs, so keep test-result caching disabled for these
host-dependent suites. Compilation remains cacheable. These settings apply
to both integration tests and generated unit tests.

## Sources

- [Cargo metadata](https://doc.rust-lang.org/stable/cargo/commands/cargo-metadata.html)
  defines the native integration graph.
- [The Cargo lockfile](https://doc.rust-lang.org/cargo/reference/lockfile.html)
  defines resolved versions and checksums.
