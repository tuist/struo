# C and C++ Static Libraries

Build C and C++ static libraries with Struo's `c_library` target kind, cache the compiled objects, and link the archive into Zig, Apple, or Android targets.

Struo does not read Makefiles or other native C build files. You declare each
library in `struo.toml`. To keep an existing CMake build, see
[CMake](/guide/graph/cmake).

## Prerequisites

The default toolchain requires `cc` and `ar` on the command search path. A
target with `.cc`, `.cpp`, or `.cxx` sources also requires `c++`:

```sh
cc --version
ar --version
c++ --version
```

Set `compiler`, `archiver`, or `cxx_compiler` on the target when the project
should use different executable paths.

## Declare targets

### Declare a static library

Copy the shipped starter into `native/math`, or create the files by hand:

```sh
struo edit materialize-example c_library c-library-minimal --destination native/math
```

The manifest at `native/math/struo.toml` declares one library:

```toml
[[target]]
name = "native_math"
kind = "c_library"
srcs = ["src/*.c"]

[target.attrs]
hdrs = ["include/native_math.h"]
includes = ["include"]
```

It uses this source layout:

```text
native/math/
├── struo.toml
├── include/
│   └── native_math.h
└── src/
    └── native_math.c
```

`hdrs` publishes the header to dependent targets. `includes` lets the source
and consumers include it as `#include "native_math.h"`.

### Query before building

Inspect the target and the complete `c_library` contract:

```sh
struo query targets --kind c_library
struo query capabilities native/math/native_math
struo query schema c_library
```

The target exposes `build` only.

### Build the library

Build the static archive:

```sh
struo build native/math/native_math
```

On macOS and Linux, the archive is
`.struo/out/native/math/native_math/libnative_math.a`. On Windows, it is
`.struo/out/native/math/native_math/native_math.lib`. Set `output_name` to
change the archive name without changing the target identifier.

### Add C++ sources

The same target can compile C and C++ together. Extend its source patterns when
the library gains a C++ implementation:

```toml
[[target]]
name = "native_math"
kind = "c_library"
srcs = ["src/*.c", "src/*.cpp"]

[target.attrs]
hdrs = ["include/native_math.h", "include/native_stats.hpp"]
includes = ["include"]
```

Struo compiles `.c` files with `cc` and `.cc`, `.cpp`, and `.cxx` files with
`c++`. A target that contains only C sources does not need a C++ compiler.

### Declare a header-only library

Omit `srcs` and publish `hdrs` or `header_globs`. The target then passes its
headers and compile settings to dependents without producing an archive.

## Manage dependencies

Another `c_library` can list `./native_math` as a dependency. It receives the
headers, include directories, definitions, static and dynamic libraries,
linker flags, and data declared by that library and its own dependencies.

Other ecosystems consume the same library:

- A [`zig_binary`](/reference/prelude/zig_binary) can list it in `deps` and
  receive its headers and link inputs.
- Apple link targets can link the static archive.
- Android packaging can include prebuilt dynamic libraries from the target
  when `android_abi` names their
  [Application Binary Interface](https://developer.android.com/ndk/guides/abis).

## Limitations

- `c_library` builds static libraries only. It does not link a C or C++
  executable or build a shared library from source. Let the consuming
  ecosystem target own the final executable or shared artifact.
- There is no C or C++ test target, so `struo run` and `struo test` do not
  apply to these targets.

## Target kinds

- [`c_library`](/reference/prelude/c_library): compiler selection, public
  headers, include directories, definitions, compiler and linker options,
  prebuilt dynamic libraries, data, and dependencies.

## Next steps

- [Zig](/guide/graph/zig): link the library into a Zig executable.
- [Apple](/guide/graph/apple) or [Android](/guide/graph/android): connect the
  archive to the target that owns the final app.
- [CMake](/guide/graph/cmake): wrap an existing CMake project instead of
  declaring each library.
