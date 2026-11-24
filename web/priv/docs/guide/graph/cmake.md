# CMake Projects

Put Struo in front of an existing CMake build: declare its products, link them into other targets, and query the configured CMake target graph.

Struo does not replace CMake. Two target kinds cover the two halves of the
work:

- `cmake_project` runs CMake configure and build as one action, then stages the
  products you declare.
- `cmake_workspace` imports a checked snapshot of the configured CMake graph as
  `cmake_target` nodes you can query.

## Prerequisites

The starter uses CMake, Ninja, Python 3, and a C compiler:

```sh
cmake --version
ninja --version
python3 --version
cc --version
```

Set the `cmake` and `build_program` attributes when the project should use
specific installations.

## Try the starter

Materialize the complete example:

```sh
struo edit materialize-example cmake_project cmake-project-minimal
```

The starter contains a static C library, a `cmake_project` build target, a
`cmake_workspace` graph target, an annotated snapshot refresh script, and a
checked snapshot.

Inspect both layers before building:

```sh
struo query targets
struo query schema cmake_project
struo query schema cmake_workspace
struo query schema cmake_target
```

The target list includes `CMakeProject`, `CMakeGraph`, and the imported
`cmake-greeting` target.

Build the declared archive:

```sh
struo build CMakeProject
```

The product is written under `.struo/out/CMakeProject/products/`.

## Declare targets

### Declare the build boundary

A `cmake_project` target names every input and product visible to Struo:

```toml
[[target]]
name = "CMakeProject"
kind = "cmake_project"
srcs = [
  "CMakeLists.txt",
  "cmake/**/*.cmake",
  "include/**/*.h",
  "src/**/*.c",
]

[target.attrs]
source_dir = "."
generator = "Ninja"
build_type = "Release"
build_targets = ["greeting"]
products = ["{static_prefix}greeting{static_suffix}"]
hdrs = ["include/greeting.h"]
includes = ["include"]
```

Only files listed in `products` become outputs. Product paths are relative to
the generated build directory and can use these portable tokens:

| Token | macOS and Linux | Windows |
| --- | --- | --- |
| `{static_prefix}` | `lib` | empty |
| `{static_suffix}` | `.a` | `.lib` |
| `{shared_prefix}` | `lib` | empty |
| `{shared_suffix}` | `.dylib` on macOS, `.so` on Linux | `.dll` |
| `{exe_suffix}` | empty | `.exe` |

Declare several products when one CMake target produces more than one file.
The first product is the default output.

Configure and build are not cached, because CMake can pick up compilers,
toolchain files, and SDKs that Struo cannot fingerprint. Every
`struo build CMakeProject` cleans and rebuilds the native project.

### Import the logical graph

The starter refresh script reads CMake's
[file-based API](https://cmake.org/cmake/help/latest/manual/cmake-file-api.7.html)
and writes a checked JSON snapshot. Declare the imported workspace beside the
build target:

```toml
[[target]]
name = "CMakeGraph"
kind = "cmake_workspace"
srcs = [
  "CMakeLists.txt",
  "cmake/**/*.cmake",
  "cmake-snapshot.json",
]

[target.attrs]
snapshot = "cmake-snapshot.json"
resolver_inputs = [
  "CMakeLists.txt",
  "cmake/**/*.cmake",
  "cmake-snapshot.json",
]
source_dir = "."
generator = "Ninja"
build_type = "Release"
exports = ["greeting"]
```

`exports` accepts native CMake names or generated Struo names. Struo imports
each export plus its transitive dependencies, with their type, sources,
artifacts, include directories, and definitions.

Query the imported structure:

```sh
struo query targets --kind cmake_target
struo query target cmake-greeting
struo query 'MATCH (root:Target {id: "cmake-greeting"})-[:DEPENDS_ON*]->(dep:Target) RETURN dep.id, dep.kind'
```

Imported targets describe structure only. Build `CMakeProject`, not an
individual `cmake_target`.

### Refresh the snapshot

Run the annotated script whenever CMake configuration inputs or selections
change:

```sh
struo exec -- python3 scripts/refresh-cmake-snapshot.py
```

Then review and query the result:

```sh
git diff -- cmake-snapshot.json
struo query targets --kind cmake_target
struo build CMakeProject
```

The snapshot records the workspace CMake configuration files plus
`source_dir`, `generator`, and `build_type`. Graph loading rejects a stale
snapshot, so an old graph never silently describes a changed project. In
continuous integration, run the refresh script and fail the job when it changes
the checked snapshot.

## Manage dependencies

`cmake_project` exposes public headers, include directories, definitions,
linker options, static libraries, dynamic libraries, and data in the same shape
as [`c_library`](/guide/graph/c), so other native targets can link its
products. Declare these fields explicitly; Struo does not infer them from the
snapshot:

```toml
[target.attrs]
products = ["{static_prefix}engine{static_suffix}"]
hdrs = ["include/engine.h"]
includes = ["include"]
defines = ["ENGINE_PUBLIC=1"]
linkopts = ["-pthread"]
```

## Limitations

- Configure and build form one uncached action that cleans the build directory
  each time. Struo does not schedule individual compile and link commands, and
  CMake incremental state is not reused.
- Imported `cmake_target` nodes are query-only and cannot be built.
- Exact product paths must be declared. Multi-configuration generators may
  place products under configuration-specific directories.
- The snapshot lists configured CTest tests for inspection only. Declare a
  [`cmake_test`](/reference/prelude/cmake_test) target to run a suite through
  `struo test`; results are reported per suite.
- Source and header patterns still belong on `cmake_project`. The snapshot
  covers workspace CMake configuration files, not every compilation input or
  configuration file outside the workspace.

If the build does not have stable products yet, start with an
[annotated script](/guide/scripted/) and move to `cmake_project` once they are
known.

## Target kinds

- [`cmake_project`](/reference/prelude/cmake_project): the build boundary and
  native link outputs.
- [`cmake_workspace`](/reference/prelude/cmake_workspace): snapshot validation
  and graph import.
- [`cmake_target`](/reference/prelude/cmake_target): imported target metadata.
- [`cmake_test`](/reference/prelude/cmake_test): run CTest or native test
  targets.

## Next steps

- [C and C++](/guide/graph/c): declare small native libraries directly in
  Struo.
- [Testing and scheduling](/guide/graph/testing): run CMake suites with
  `struo test`.
- [Typed build graph](/guide/graph/#start-from-a-native-project): see how
  native projects fit into the Struo graph.
