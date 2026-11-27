# Kotlin JVM Builds and Tests

Build, run, test, and cache Kotlin and mixed Java libraries and applications on
the JVM with Struo, and produce Kotlin/Native frameworks for Apple apps.

Kotlin targets are declared in `struo.toml`. Android Kotlin sources use the
[Android target kinds](/guide/graph/android).

## Prerequisites

Install the pinned Kotlin compiler and
[Java Development Kit](https://docs.oracle.com/en/java/javase/21/install/overview-jdk-installation.html)
through mise:

```sh
mise install
mise exec -- kotlinc -version
mise exec -- java -version
```

## Declare targets

### Declare a library and binary

```toml
[[target]]
name = "Greeting"
kind = "kotlin_jvm_library"
srcs = ["src/main/kotlin/dev/struo/greeting/**/*.kt"]

[target.attrs]
module_name = "greeting"
jvm_target = "17"

[[target]]
name = "Hello"
kind = "kotlin_jvm_binary"
srcs = ["src/main/kotlin/dev/struo/hello/**/*.kt"]
deps = ["./Greeting"]

[target.attrs]
main_class = "dev.struo.hello.MainKt"
args = ["Struo"]
```

### Query, build, and run

Inspect the graph, then build and run the binary:

```sh
struo query schema kotlin_jvm_library
struo query target ./Hello
struo build ./Hello
struo run ./Hello
```

The library and binary compile to Java archives. `struo run` builds the archive
first, then starts the main class with the complete runtime classpath.

### Run host-side tests

Declare `kotlin_jvm_test` with normal compile dependencies and optional
runtime-only dependencies:

```toml
[[target]]
name = "GreetingTests"
kind = "kotlin_jvm_test"
srcs = ["src/test/kotlin/**/*.kt"]
deps = ["./Greeting"]

[target.dependencies]
runtime_deps = ["./TestRuntime"]

[target.attrs]
labels = ["unit"]
```

Run it:

```sh
struo test GreetingTests
```

The runner discovers zero-argument methods whose names begin with `test`, and
also recognizes [JUnit](https://junit.org/junit5/) and Kotlin test annotations
when those libraries are on the classpath. Results use the same test record as
other ecosystems, so affected-test queries and MCP tools work unchanged.

### Mix Java sources

List `.java` files in `srcs` next to the Kotlin sources. Kotlin and Java can
reference each other in either direction, and Java test classes in a
`kotlin_jvm_test` are discovered like Kotlin ones:

```toml
[[target]]
name = "Greeting"
kind = "kotlin_jvm_library"
srcs = ["src/main/kotlin/**/*.kt", "src/main/java/**/*.java"]

[target.attrs]
jvm_target = "1.8"
javac_opts = ["-Xlint:none"]
```

### Compile multiplatform source sets

A Kotlin Multiplatform module keeps shared code in source sets such as
`commonMain`, with `expect` declarations that the JVM source set implements
with `actual` ones. Declare the shared source sets in `source_sets` and keep
the JVM sources in `srcs`:

```toml
[[target]]
name = "Result"
kind = "kotlin_jvm_library"
srcs = ["src/jvmMain/kotlin/**/*.kt"]

[target.attrs]
source_sets = { commonMain = ["src/commonMain/kotlin/**/*.kt"], nonJsMain = ["src/nonJsMain/kotlin/**/*.kt"] }
source_set_depends_on = { nonJsMain = ["commonMain"] }
```

Each source set compiles as its own fragment, and the JVM sources refine
every declared source set, so `expect` and `actual` declarations match the
way the Kotlin Gradle plugin compiles a JVM target. `source_set_depends_on`
mirrors `dependsOn` between intermediate source sets. Tests declare
`commonTest` the same way on a `kotlin_jvm_test`.

## Manage dependencies

### Import prebuilt Java archives

Use [`java_import`](/reference/prelude/java_import) for archives built
elsewhere, such as Maven artifacts resolved by another tool:

```toml
[[target]]
name = "TestLibraries"
kind = "java_import"

[target.attrs]
jars = ["third_party/junit-4.13.2.jar", "third_party/hamcrest-core-1.3.jar"]
```

Any Kotlin or Android role that accepts `java_library` accepts the import.

### Model classpath roles

Use `deps` for normal compile and runtime libraries. More precise relationships
belong under `[target.dependencies]`:

```toml
[target.dependencies]
associates = ["./FriendModule"]
exported_deps = ["./PublicTypes"]
provided_deps = ["./CompileApi"]
runtime_deps = ["./RuntimeSupport"]
```

- `associates` passes Kotlin friend paths so internal declarations are visible.
- `exported_deps` also propagates to downstream compile and runtime classpaths.
- `provided_deps` appears only while compiling.
- `runtime_deps` appears only while running.

Each role is validated before Kotlin runs, so a runtime-only artifact in a
compile-only role fails early.

## Use compiler plug-ins

Pass package-relative compiler plug-in Java archives and their options through
attributes:

```toml
[target.attrs]
compiler_plugin_jars = ["tools/serialization-plugin.jar"]
compiler_plugin_options = [
  "plugin:org.example.serialization:enabled=true",
]
```

Changing the archive contents or options invalidates the cached compile.

## Use Kotlin from Android and Apple

Use [`android_library`](/reference/prelude/android_library) or
[`android_binary`](/reference/prelude/android_binary) for Android Kotlin
sources, because those target kinds also own resources and application
packaging. Use
[`kotlin_apple_framework`](/reference/prelude/kotlin_apple_framework) when an
Apple target should consume Kotlin/Native code as a framework.

## Limitations

- Generated source directories and Kotlin symbol processing are not supported.
  Keep those pipelines in a script target.
- Resource packaging, annotation processing, incremental compilation,
  interface archive generation, and test framework lifecycle integration are
  not part of the Kotlin JVM target kinds. Android local tests are available
  through [`android_local_test`](/reference/prelude/android_local_test).

## Target kinds

- [`kotlin_jvm_library`](/reference/prelude/kotlin_jvm_library)
- [`kotlin_jvm_binary`](/reference/prelude/kotlin_jvm_binary)
- [`kotlin_jvm_test`](/reference/prelude/kotlin_jvm_test)
- [`kotlin_apple_framework`](/reference/prelude/kotlin_apple_framework)
- [`java_import`](/reference/prelude/java_import)

## Next steps

- [Android](/guide/graph/android): package Kotlin code into an Android
  application.
- [Apple](/guide/graph/apple): link a Kotlin/Native framework into an Apple
  application.
- [Testing and Scheduling](/guide/graph/testing): select and report test runs.
