# Zig Builds and Tests

Build, run, test, and cache Zig modules, executables, and static or shared
libraries with Struo, including locked `build.zig.zon` packages and C interop.

Zig targets are declared in `struo.toml`. This guide uses one math module from
a binary and a test target, then adds locked packages and libraries.

## Prerequisites

Install the repository's pinned Zig compiler through mise:

```sh
mise install
mise exec -- zig version
```

Zig targets resolve `zig` from the command search path by default. Set the
`zig` attribute to use a different executable. Set `zig_version` to reject a
compiler whose reported version does not match the project.

The chosen compiler must include LLVM support for the requested target,
because Zig compilation uses the LLVM backend by default.

## Declare targets

### Declare a module, binary, and test

Create `tools/hello/struo.toml`:

```toml
[[target]]
name = "math"
kind = "zig_library"
srcs = ["src/**/*.zig"]

[target.attrs]
main = "src/math.zig"
import_name = "math"

[[target]]
name = "hello"
kind = "zig_binary"
srcs = ["src/**/*.zig"]
deps = ["./math"]

[target.attrs]
main = "src/main.zig"

[[target]]
name = "math_tests"
kind = "zig_test"
srcs = ["src/**/*.zig"]
deps = ["./math"]

[target.attrs]
main = "src/math_test.zig"
labels = ["unit"]
```

Use this source layout:

```text
tools/hello/
├── struo.toml
└── src/
    ├── main.zig
    ├── math.zig
    └── math_test.zig
```

The binary and test import the module with `@import("math")`. The
`import_name` and `deps` entry make that name explicit.

### Query before building

Inspect the targets and their capabilities:

```sh
struo query targets --kind zig_library
struo query capabilities tools/hello/math
struo query capabilities tools/hello/hello
struo query capabilities tools/hello/math_tests
struo query schema zig_binary
struo query schema zig_test
```

The module exposes `build` but does not compile an artifact by itself; its
consumers compile it as part of their whole program. The binary exposes
`build` and `run`, and the test exposes `build` and `test`.

### Build and run

Build the executable and its module:

```sh
struo build tools/hello/hello
```

The executable appears at `.struo/out/tools/hello/hello/hello`, with `.exe`
added on Windows, and Zig documentation at
`.struo/out/tools/hello/hello/hello.docs`.

Run the same binary:

```sh
struo run tools/hello/hello
```

The binary can receive `args`, `env`, and declared `data` inputs. Run output,
including `stdout.log` and `run.json`, is stored under
`.struo/out/tools/hello/hello/run/`. Runs are not replayed from the cache, so
each invocation executes the program again.

### Run the test

Build and execute the host test target:

```sh
struo test tools/hello/math_tests
```

Results and logs appear under `.struo/out/tools/hello/math_tests/test/`,
including `test_results.json` and `zig-test.log`.

## Manage dependencies

### Import locked Zig packages

Use [`zig_dependencies`](/reference/prelude/zig_dependencies) when the project
already declares packages in `build.zig.zon`. Zig's package manifest stays the
source of truth for dependency aliases, local paths, lazy packages, source
locations, and content hashes.

```toml
[[target]]
name = "zig_dependencies"
kind = "zig_dependencies"
srcs = [
  "build.zig.zon",
  "third_party/zig/**/build.zig.zon",
]

[target.attrs]
manifest = "build.zig.zon"
resolver_inputs = ["build.zig.zon", "third_party/zig/**/build.zig.zon"]
vendor_dir = "third_party/zig"

[[target]]
name = "hello"
kind = "zig_binary"
srcs = ["src/**/*.zig"]
deps = ["zig_dependencies"]

[target.attrs]
main = "src/main.zig"
```

Struo emits one queryable `zig_package` target per package instance. Local path
dependencies resolve relative to the manifest that declares them. Remote
packages are expected under the vendor directory, keyed by their Zig content
hash. Verify them with Zig's native fetch workflow before committing them;
ordinary builds never fetch or update packages. `zig build --fetch` writes
packages to `zig-pkg/` in this layout, so `vendor_dir = "zig-pkg"` uses them
where Zig put them.

A package's public module is the one its `build.zig` exports with `addModule`
under the dependency alias or package name; add the packages' `build.zig`
files to `resolver_inputs` so Struo can read them. Without one, the module is
`src/root.zig`. Use `module_paths` when a package exposes another module:

```toml
[target.attrs.module_paths]
special_package = "third_party/zig/special/source/main.zig"
```

Use `package_paths` to map an alias or content hash to a different source
directory when an existing vendoring layout cannot change.

Inspect the imported packages, then build and run the consumer:

```sh
struo query targets --kind zig_package
struo build hello
struo run hello
```

The bundled dependency starter imports a locked `math` package and prints its
answer, `42`.

### Connect C and C++ libraries

Zig binary, test, module, static-library, and shared-library targets accept a
[`c_library`](/reference/prelude/c_library) dependency and receive its headers,
definitions, archives, dynamic libraries, and linker options.

[`zig_c_library`](/reference/prelude/zig_c_library) exposes C headers as an
importable Zig module and needs at least one C header from its dependencies.
Add it only when Zig source needs translated C declarations, not just a
linked archive.

## Build libraries and configured variants

Use [`zig_static_library`](/reference/prelude/zig_static_library) or
[`zig_shared_library`](/reference/prelude/zig_shared_library) when another
native target needs a linkable artifact. Both consume the `math` module
through `deps` and can emit documentation.

Configured variants keep build choices such as `mode`, `threaded`, and
`zig_version` on the target:

- [`zig_configure`](/reference/prelude/zig_configure) builds a static or shared
  library.
- [`zig_configure_binary`](/reference/prelude/zig_configure_binary) builds an
  executable.
- [`zig_configure_test`](/reference/prelude/zig_configure_test) builds a test.

Use the base kinds until the project needs one of those settings. To generate a
Zig module with a host tool at build time, use
[`zig_generated_library`](/reference/prelude/zig_generated_library).

For constants that `build.zig` passes through `b.addOptions()`, declare a
[`zig_options`](/reference/prelude/zig_options) module and depend on it. A
package that imports such a module gets it through `package_deps` on
`zig_dependencies`.

## Cache Zig builds

Zig preserves debug information by default, and Struo never strips it
automatically. Unstripped compilations bypass the cache, because LLVM debug
metadata can differ between otherwise identical builds. Set `strip = true` or
`strip_debug_symbols = true`, or pass `-fstrip` for the root module, to make
compilation cacheable:

```toml
[target.attrs]
main = "src/main.zig"
strip = true
```

A `-fno-strip` override keeps caching disabled, and an explicit `-fno-llvm`
flag in `zigopt`, `zigopts`, or `linkopts` also bypasses the cache.

A stable cache key does not prove byte-for-byte reproducibility. Check the
declared outputs of a target with:

```sh
struo build tools/hello/hello --verify-reproducible
```

## Limitations

- Test execution is host-only. A cross-target test binary can be built, but
  running it requires a platform runner outside `zig_test`.
- Compiler errors from a missing LLVM backend are reported as is; Struo does
  not fall back to another backend.
- Module import renames must identify exactly one dependency. Unknown or
  ambiguous names fail validation.
- Package resolution fails when a graph exceeds 10,000 package instances.

## Target kinds

- [`zig_library`](/reference/prelude/zig_library)
- [`zig_binary`](/reference/prelude/zig_binary)
- [`zig_test`](/reference/prelude/zig_test)
- [`zig_dependencies`](/reference/prelude/zig_dependencies)
- [`zig_package`](/reference/prelude/zig_package)
- [`zig_static_library`](/reference/prelude/zig_static_library)
- [`zig_shared_library`](/reference/prelude/zig_shared_library)
- [`zig_c_library`](/reference/prelude/zig_c_library)
- [`zig_generated_library`](/reference/prelude/zig_generated_library)
- [`zig_options`](/reference/prelude/zig_options)
- [`zig_configure`](/reference/prelude/zig_configure),
  [`zig_configure_binary`](/reference/prelude/zig_configure_binary), and
  [`zig_configure_test`](/reference/prelude/zig_configure_test)

## Next steps

- [C and C++](/guide/graph/c): declare the native library a Zig target links
  or translates.
- [Testing and Scheduling](/guide/graph/testing): select and schedule tests
  across the graph.
- [Memory](/guide/memory/): inspect the durable context Struo records for
  builds and tests.
