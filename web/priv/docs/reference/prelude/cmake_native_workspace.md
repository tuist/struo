# `cmake_native_workspace`

Discovered CMake project that emits ordinary CMake targets.

## Description

Struo creates this seed for a directory that holds `CMakeLists.txt` and is not
inside another discovered CMake project. When the graph is derived, its
resolver configures the project with the Ninja generator and CMake's
[file-based application programming interface](https://cmake.org/cmake/help/latest/manual/cmake-file-api.7.html),
then emits:

- one [`cmake_project`](/reference/prelude/cmake_project) for each configured
  static library, shared library, module library, and executable, named after
  the CMake target;
- one [`cmake_test`](/reference/prelude/cmake_test) per registered CTest test,
  named `ctest__` followed by the test name, which runs in the shared build
  tree of a `cmake_project` named `ctest_build`. A project whose tests use
  `RUN_SERIAL`, `RESOURCE_LOCK`, or `RESOURCE_GROUPS` gets one `cmake_test`
  named `ctest` for the whole suite instead.

The configured tree is kept between loads and configured again only when a
file CMake read while configuring, a selected compiler, the configure command,
or the list of files in the project changes.

Every product is built in one incremental tree, a `cmake_project` named
`cmake_build` with `export_build_tree`, and each emitted `cmake_project` copies
the artifact CMake reports out of it. It publishes the target's source-tree headers and
include directories, and the headers CMake generates into include directories
under the build tree, so native dependents can compile and link against it.
Each emitted library and executable takes the workspace configuration files
reported by CMake, the sources of its CMake target and everything it depends
on, and the files listed under their source-tree include directories as
inputs. Each test takes the
configuration files, the sources of the targets whose artifacts its command
runs or names and their dependencies, the source paths its command, working
directory, `REQUIRED_FILES`, and fixtures name, so `struo query affected-tests`
selects only the tests a change reaches. A test whose program is not a project
target reads every project source. A versioned shared library also stages its
SONAME link, such as `libname.so.1`. Paths are recorded relative to the source
and build directories, so the graph does not depend on the checkout location.
A project that fails to configure, or a missing CMake, CTest, or Ninja, is
reported as a diagnostic on its seed without blocking unrelated targets.

An authored `cmake_project` or `cmake_test` whose `srcs` cover the top-level
`CMakeLists.txt` suppresses discovery. Declare this target explicitly to change
the build type or configure arguments.

## Attributes

| Attribute | Type | Required | Default | Description |
| --- | --- | --- | --- | --- |
| `cmake` | string | no | `cmake` | CMake executable name or path |
| `ctest` | string | no | `ctest` | CTest executable name or path |
| `build_type` | string | no | empty | Build configuration for discovery and emitted targets. Empty keeps CMake's default flags |
| `configure_args` | list&lt;string&gt; | no | `[]` | Configure arguments for discovery and emitted targets |
| `jobs` | int | no | `4` | Concurrency of the emitted CTest suite when tests run as one suite |
| `resolver_inputs` | list&lt;string&gt; | no | `srcs` | Text globs supplied to discovery |

## Dependency Edges

| Edge | Accepts | Description |
| --- | --- | --- |
| `deps` | `cmake_project` | Resolver-generated CMake products |

## Providers and Capabilities

The target emits `cmake_native_workspace` and exposes `build`, which builds
every emitted product. `struo test` without a target selects every emitted
test.

## Example

```toml
[[target]]
name = "cmake"
kind = "cmake_native_workspace"
srcs = ["CMakeLists.txt", "**/CMakeLists.txt", "**/*.cmake"]

[target.attrs]
build_type = "Release"
configure_args = ["-DBUILD_TESTING=ON"]
```

## Limitations

- Discovery requires `cmake`, `ctest`, and `ninja` on the command search path
  and runs CMake configure while the graph loads when the configuration changed.
- Multi-configuration generators and CMake presets are not used.
- A target whose artifact CMake writes outside the build directory is not
  emitted.
