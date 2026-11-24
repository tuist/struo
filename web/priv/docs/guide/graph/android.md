# Android Apps and Libraries

Build, run, test, and cache Android resources, Java and Kotlin libraries, and
application packages with Struo, declared as targets in `struo.toml`.

This guide grows one application, so each new target has a visible relationship
to the previous one.

## Prerequisites

Android targets require:

- a [Java Development Kit](https://docs.oracle.com/en/java/javase/21/install/overview-jdk-installation.html)
  with `java`, `javac`, and `jar`;
- the [Android Software Development Kit command-line tools](https://developer.android.com/tools),
  an installed platform, and an installed build-tools package;
- `kotlinc` and `kotlin-stdlib.jar` when a target contains Kotlin sources.

In the Struo repository, a setup task accepts Android licenses, installs the
pinned packages and emulator system image, creates the `struo_example`
emulator, and creates a local debug keystore when one is missing:

```sh
mise install
mise run android:install-sdk
```

Running an application also requires platform tools, including the
[Android Debug Bridge](https://developer.android.com/tools/adb), plus a
connected device or configured emulator. Native Swift or Rust dependencies
also need the [Android Native Development Kit](https://developer.android.com/ndk).

### Configure tool discovery

Android targets find the development kit through the `android_sdk` attribute,
`ANDROID_HOME`, or `ANDROID_SDK_ROOT`. When a platform or build-tools version
is omitted, Struo selects the highest installed version.

Java-backed targets use `javac`, `jar`, and `java` from the host unless their
paths are overridden. Kotlin-backed targets find `kotlinc` on `PATH` unless
the `kotlinc` attribute is set. Set `kotlin_home` or `kotlin_stdlib` when the
standard library is not next to the compiler installation.

Debug signing uses a package-relative `debug_keystore`,
`ANDROID_DEBUG_KEYSTORE`, or `~/.android/debug.keystore`. Struo does not ship
private key material. Set `signing = "none"` when an unsigned package is
enough.

## Declare targets

### Declare one application graph

Create `apps/hello/struo.toml` with resources, a reusable library, and an
application that connects them:

```toml
[[target]]
name = "HelloResources"
kind = "android_resource"

[target.attrs]
package = "dev.struo.hello"
manifest = "ResourcesManifest.xml"
resource_files = ["res/**"]
min_sdk_version = 23

[[target]]
name = "Greeting"
kind = "android_library"
srcs = ["src/main/**/*.kt"]
deps = ["./HelloResources"]

[target.attrs]
namespace = "dev.struo.greeting"
manifest = "LibraryManifest.xml"
resource_files = []
min_sdk_version = 23

[[target]]
name = "Hello"
kind = "android_binary"
srcs = ["src/app/**/*.kt"]
deps = ["./Greeting"]

[target.attrs]
application_id = "dev.struo.hello"
manifest = "AndroidManifest.xml"
resource_files = []
min_sdk_version = 23
version_code = 1
version_name = "1.0"
```

The three targets form one chain:

```text
HelloResources -> Greeting -> Hello
```

`HelloResources` owns shared resources. `Greeting` compiles the reusable
Kotlin code and consumes those resources. `Hello` owns the final application
manifest and package identity.

Keep `ResourcesManifest.xml` focused on the resource package. Put launcher
activities, application metadata, and version details in
`AndroidManifest.xml`. Small targets can keep resources and assets inline,
but an explicit `android_resource` dependency makes shared ownership easier to
query.

### Query before building

Inspect the target chain and the final application's contract:

```sh
struo query targets --kind android_binary
struo query capabilities apps/hello/HelloResources
struo query capabilities apps/hello/Greeting
struo query capabilities apps/hello/Hello
struo query schema android_binary
```

The resource and library targets expose `build`. The application exposes
`build` and `run`.

### Build and run

Build the Android application package:

```sh
struo build apps/hello/Hello
```

Struo builds the resource and library dependencies first, then compiles,
packages, aligns, and signs the application. Outputs are materialized under
`.struo/out/<target>/`; the
[`android_binary` reference](/reference/prelude/android_binary) lists the
exact output groups.

Run the package on a connected device or emulator:

```sh
struo run apps/hello/Hello
```

The run waits for a device through `adb`, installs the package, resolves its
launcher activity, and starts it. Set `launch_activity` when the manifest does
not provide an unambiguous launcher. Set `adb_serial` when more than one
device is connected.

If `emulator_device` names an Android Virtual Device, request its visible
interface while running:

```sh
struo run --visible apps/hello/Hello
```

Installation and launch happen on every invocation because they depend on
external device state.

### Add a local test

Add a host-side test for `Greeting` to the same manifest:

```toml
[[target]]
name = "GreetingTests"
kind = "android_local_test"
srcs = ["src/test/**/*.kt"]
deps = ["./Greeting"]

[target.attrs]
labels = ["unit"]
```

The local runner finds zero-argument methods whose names begin with `test`. It
can also run methods annotated by JUnit or Kotlin test libraries when those
libraries are added to `classpath` and `runtime_classpath`.

Query and run the new target:

```sh
struo query capabilities apps/hello/GreetingTests
struo test apps/hello/GreetingTests
```

Local tests run on the host Java Virtual Machine and can replay from cache.
Tests that need Android framework or device behavior should use
[`android_instrumentation_test`](/reference/prelude/android_instrumentation_test),
which installs the application and test packages on a connected device or
emulator and does not replay from cache.

## Manage dependencies

An `android_binary` can consume native shared libraries through normal
dependencies:

- [`swift_android_library`](/reference/prelude/swift_android_library)
  packages a Swift library for Android. It statically links the Swift standard
  library and includes the matching C++ shared runtime from the selected Swift
  SDK.
- [`rust_mobile_library`](/reference/prelude/rust_mobile_library) produces an
  Android shared library for the application.

Native Swift targets use `ANDROID_NDK_HOME`, `android_ndk`, or
`tools_directory` to find native linker tools. Native Rust targets use
`ANDROID_NDK_HOME` or `android_ndk` to choose a default linker. The
`native-mobile-shared-code-e2e` starter loads Swift and Rust libraries from
Kotlin and displays values returned by both.

Add native code after the Java or Kotlin application builds on its own, so
development-kit and linker setup stay separate from the first application
graph.

## Choose values by configuration

Configurable Android attributes accept `select`. Active configuration tokens
include `android`, installed and minimum platform-level tokens such as
`compile_sdk_35` and `min_sdk_23`, `debug`, and `default`:

```toml
[target.attrs]
javac_opts = { select = { android = ["-Xlint:deprecation"], default = [] } }
```

Attributes that choose the configuration, including `compile_sdk` and
`min_sdk_version`, must remain literal. The target kind schema identifies any
other non-configurable attributes.

See the [configurations guide](/guide/graph/configuration) for how
`--config` overrides these tokens per invocation and how the effective
configuration scopes outputs.

## Use a coding harness

A connected coding harness can start from the request "build an Android app
with Struo." It discovers `android_binary`, fetches its runnable starter,
validates the workspace, builds the target, and checks the application package,
using the same discovery and validation loop as every other target kind.

The `android_resource`, `android_library`, and `android_binary` schemas also
link to the corresponding
[Bazel Android rules](https://bazelbuild.github.io/rules_android/),
[Buck2 Android rules](https://buck2.build/docs/prelude/rules/android/), and
[Android Gradle plugin](https://developer.android.com/build) concepts, so a
harness can reproduce only the slice of an existing build that you ask Struo to
own. When the source build uses another rule or plugin, the harness can write a
project-local target kind instead. See [Coding harnesses](/guide/harness) for
the workflow.

## Limitations

The target kinds support Java and Kotlin sources, resources, assets, library
packaging, native shared libraries, application packaging, debug signing, local
tests, and instrumentation tests. Data binding, annotation processors, code
shrinking, density filtering, production signing, and several advanced
packaging controls are not implemented. Non-empty unsupported attributes fail
validation instead of being ignored.

## Target kinds

- [`android_resource`](/reference/prelude/android_resource)
- [`android_library`](/reference/prelude/android_library)
- [`android_binary`](/reference/prelude/android_binary)
- [`android_local_test`](/reference/prelude/android_local_test)
- [`android_instrumentation_test`](/reference/prelude/android_instrumentation_test)

## Next steps

- [Kotlin](/guide/graph/kotlin): build JVM libraries and tests shared with the
  application.
- [Testing and Scheduling](/guide/graph/testing): select and report local and
  instrumentation test runs.
- [Ecosystems](/guide/graph/ecosystems): add Apple or Rust code one
  cross-platform dependency at a time.
- [Memory](/guide/memory/): record durable context about graph work.
