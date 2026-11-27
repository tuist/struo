# `kotlin_jvm_test`

Host-side Kotlin tests for the Java virtual machine.

## Description

Compiles Kotlin test sources, scans the resulting classes for zero-argument
test methods, and emits normalized Struo test results. Java test sources in
`srcs` compile jointly with the Kotlin sources, as described for
[`kotlin_jvm_library`](/reference/prelude/kotlin_jvm_library#description), and
Java test classes are discovered like Kotlin ones. Methods whose names begin
with `test` run without an external test framework. The runner also recognizes
the `org.junit.Test` annotation from [JUnit](https://junit.org/junit5/) and the
`kotlin.test.Test` annotation when their libraries are on the dependency
classpath.

Static nested test classes are discovered as well as top-level classes.
Public inherited test methods run on the concrete suite, with overridden
methods selected once.
Anonymous classes, local classes, abstract classes, and non-static inner
classes are not independent test suites.
Methods or suites annotated with `org.junit.Ignore` or `kotlin.test.Ignore`
are reported as skipped without invoking them or initializing ignored suites.

Set `test_framework = "junit4"` to execute annotated tests through native
JUnit 4 instead of invoking methods directly. Declare JUnit 4 and its runtime
dependencies on the target's runtime classpath. Native execution honors
inherited setup and teardown, class lifecycle, rules, expected exceptions,
assumptions, and `@RunWith` runners, including parameterized tests. Results
remain normalized per case, with assumption violations reported as skipped
and setup or teardown failures reported as failed. `Class#method` selects all
parameter values; `Class#method[0]` selects an exact parameterized case when
the runner uses that name. Aggregate suites execute their child classes once;
a class-only suite filter selects all its children. Invalid annotated methods
are reported through native JUnit initialization failures. A class annotated
with `@Ignore` reports each of its test methods, or each parameterized case,
as skipped, matching JUnit's own counts, without initializing the class. Convention-only
methods are not JUnit tests.

Set `test_framework = "junit5"` to run tests on the JUnit Platform, as
Gradle's `useJUnitPlatform()` does. Every engine on the runtime classpath runs,
such as Jupiter, or Vintage for JUnit 4 tests. Declare the engines, the JUnit
Platform launcher, and their dependencies on the target's runtime classpath.
Each test reports as `Class.method`; a nested class keeps its binary name, such
as `Outer$Inner.method`, and overloaded test methods add their parameter types,
such as `Class.method(org.junit.jupiter.api.TestInfo)`. Jupiter parameterized
and dynamic tests report one case per invocation, such as `Class.method[2]`,
numbered from 1 as Jupiter does. Vintage keeps JUnit 4's names, such as
`Class.method[0]`. Tests from engines that do not report a method, such as
specification-style engines, are named by their display names below the class,
joined with `/`. A disabled class reports each of its tests as skipped. A
failure in class-level setup or teardown reports as a failed `Class.classMethod`
case, and an aborted class reports its tests as skipped. A class filter also
selects its nested classes; `Class#method` selects every invocation of a
parameterized or dynamic test, and `Class#method[2]` selects only that Jupiter
invocation. Tests may run in parallel through JUnit's own configuration. Per-test
coverage measures a Jupiter parameterized or dynamic test as one unit,
`Class.method`, and each Vintage parameterized case as its own unit. A failure
in class-level teardown is attached to each unit's log without failing a unit
that passed. Per-test coverage and `Class#method` filters need tests known at
discovery: engines that register tests while they run, such as Kotest, are
reported as unresolved when listing coverage units.

## Coverage-assisted Selection

Set `coverage_per_test = true`, declare product `coverage_sources`, and
provide package-relative `jacoco_agent` and `jacoco_cli` JAR paths. Use
matching JaCoCo releases compatible with your class-file version. The initial
collector requires Linux, Python 3 and strace.

Coverage measures classes compiled by targets in the graph. Classes from
`java_import`, `classpath` and `runtime_classpath` jars are not measured, so
each `coverage_sources` file must be compiled by a target.

Struo discovers current execution units without invoking test bodies and runs
each exact unit with its own JaCoCo profile. Class-file analysis preserves
zero-hit sources. Duplicate identities, additional reported cases, missing
instrumentation, subprocesses, failures and skips cannot establish complete
coverage evidence. Native parameterized cases keep their discovered names;
aggregate suites cannot substitute for individual evidence. Coverage runs
bypass execution caching. Changes to test sources and configuration retain
the target.

## Test Attributes

| Attribute | Type | Required | Default | Description |
| --- | --- | --- | --- | --- |
| `test_framework` | string | no | `reflection` | `reflection` for convention-based tests, `junit4` for native JUnit 4 execution, or `junit5` for the JUnit Platform |
| `coverage_per_test` | bool | no | `false` | Measure each exact execution unit with JaCoCo |
| `coverage_sources` | list&lt;string&gt; | no | `[]` | Product Java or Kotlin source globs required in the class-file universe |
| `jacoco_agent` | string | no | empty | Package-relative JaCoCo runtime agent JAR |
| `jacoco_cli` | string | no | empty | Package-relative matching JaCoCo command-line JAR |
| `test_class` | string | no |  | Fully qualified class or `Class#method` filter |
| `args` | list&lt;string&gt; | no | `[]` | Additional class or `Class#method` filters |
| `jvm_flags` | list&lt;string&gt; | no | `[]` | Flags passed to the Java virtual machine before the test classpath. Tests run with `-ea`, as in Gradle; pass `-da` to disable assertions |
| `env` | map&lt;string,string&gt; | no | `{}` | Environment variables applied before `test_env` |
| `env_inherit` | list&lt;string&gt; | no | `[]` | Host environment variable names to inherit |
| `test_env` | map&lt;string,string&gt; | no | `{}` | Environment variables applied last |
| `labels` | list&lt;string&gt; | no | `[]` | Labels exposed during test discovery |
| `timeout_ms` | integer | no |  | Optional test timeout in milliseconds |
| `java` | string | no | host tool | Override the Java runtime path |
| `javac` | string | no | host tool | Override the Java compiler used for Java sources and the generated runner |

The compiler attributes from
[`kotlin_jvm_library`](/reference/prelude/kotlin_jvm_library#attributes) also
apply.

## Dependency Edges

| Edge | Accepts | Description |
| --- | --- | --- |
| `deps` | `kotlin_jvm_library`, `java_library` | Libraries used for compilation and runtime |
| `associates` | `kotlin_jvm_library` | Friend modules whose internal declarations are visible during compilation |
| `exported_deps` | `kotlin_jvm_library`, `java_library` | Libraries propagated to compile and runtime classpaths |
| `provided_deps` | `kotlin_jvm_library`, `java_library` | Libraries available only while compiling |
| `runtime_deps` | `kotlin_jvm_library`, `java_library` | Libraries available only while running tests |

## Providers

The target emits `kotlin_jvm_test` and `struo_test_info`.

## Capabilities

| Capability | Output groups |
| --- | --- |
| `test` | `default`, `test_results`, `logs` |

## Outputs

| Output | Location |
| --- | --- |
| Normalized results | `.struo/out/<target>/test/test_results.json` |
| Test log | `.struo/out/<target>/test/kotlin-jvm-test.log` |
| Native results | `.struo/out/<target>/test/native_results.txt` |

## Example

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
test_class = "dev.struo.greeting.GreetingTest"
```

Run all selected methods with `struo test GreetingTests`. Pass a
`Class#method` value through `test_class` or `args` for a narrower run.

## Limitations

The reflection runner does not implement test framework lifecycle hooks,
parameterized tests, sharding, retries, or coverage collection. Frameworks can
still supply assertions and annotations through normal dependencies.
