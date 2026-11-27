# `cmake_test`

Build and test an existing CMake project through CTest or explicit native test
targets. This is separate from
[`cmake_project`](/reference/prelude/cmake_project), which stages declared
products and exposes a native link provider rather than running tests.

```toml
[[target]]
name = "Tests"
kind = "cmake_test"
srcs = ["CMakeLists.txt", "include/**/*.h", "src/**/*.c", "tests/**"]

[target.attrs]
configure_args = ["-DBUILD_TESTING=ON"]
labels = ["unit"]
```

Run `struo build Tests` to configure and build, then `struo test Tests` to run
CTest. Native test failures and empty suites fail the command. Test results
are retained even when CTest fails. Use `struo query tests`,
`struo query test-manifest Tests`, and `struo query test-results Tests` to
inspect the runner contract and normalized outcome.

For projects that expose custom test targets instead of registering CTest
entries, set `test_targets = ["check"]` to the upstream target names. Struo runs
`cmake --build <build-dir> --config <build-type> --parallel <jobs> --target check`
in the test phase, after configure and build. `build_targets` only selects the
preparatory build; it does not replace the test runner. Use `env` for upstream
runner options. Explicit test targets own test discovery, empty-selection
handling, and exit status. Struo reports their nonzero exits and timeouts as
failures, but cannot infer case counts from arbitrary native output. It does
not fall back automatically when CTest finds no tests.

## Coverage-assisted selection

Set `coverage_per_test = true` to discover registered CTest invocations and
measure each in a fresh process. A registered invocation is an atomic execution
unit. Its profile is never attributed to individual framework cases inside
the executable. CTest remains responsible for environment, working directory,
timeouts and native execution semantics.

Compile and link the test executables with Clang's
`-fprofile-instr-generate` and compile with `-fcoverage-mapping`. Declare
instrumented product sources in `coverage_sources`, and set `llvm_cov` and
`llvm_profdata` to tools matching that compiler. Declare test sources and
configuration in `srcs` or `data`, outside `coverage_sources`, so changes to
them invalidate coverage history. Linux, Python 3 and `strace` are currently
required. Explicit `test_targets` cannot use this mode.

Discovery uses CTest's JSON inventory without running test bodies. Exact
selection uses the complete registered name. Extra subprocesses, fixture
invocations, missing instrumentation, skipped invocations and failures cannot
produce complete exclusion evidence. Native shared libraries not represented
in the exported executable's coverage also prevent complete observations.

## Attributes

| Attribute | Type | Default | Description |
| --- | --- | --- | --- |
| `cmake` | string | `cmake` | CMake executable name or path |
| `ctest` | string | `ctest` | CTest executable name or path, version 3.21 or newer |
| `source_dir` | string | `.` | Package-relative CMake source directory |
| `generator` | string | `Ninja` | Generated-build backend |
| `build_program` | string | `ninja` for Ninja | Generated-build executable name or path |
| `build_type` | string | `Debug` | Native build configuration; debug information is preserved |
| `configure_args` | list&lt;string&gt; | `[]` | Configure arguments, each passed as one argument, including upstream test-enabling options |
| `build_targets` | list&lt;string&gt; | `[]` | Native build targets; defaults to all, including tests |
| `tests` | list&lt;string&gt; | `[]` | CTest test names to run, matched exactly; empty runs every test |
| `test_targets` | list&lt;string&gt; | `[]` | Native targets run as the test suite; empty uses CTest with empty suites treated as errors |
| `jobs` | int | `1` | Positive native build and test concurrency |
| `data` | list&lt;string&gt; | `[]` | Additional runtime and configuration input globs |
| `env` | map&lt;string, string&gt; | `{}` | Declared build and test environment |
| `coverage_per_test` | bool | `false` | Isolated registered CTest invocation coverage |
| `coverage_sources` | list&lt;string&gt; | `[]` | Instrumented product sources, excluding test/configuration inputs |
| `llvm_cov` | string | `llvm-cov` | Compiler-matching LLVM coverage exporter |
| `llvm_profdata` | string | `llvm-profdata` | Compiler-matching LLVM profile merger |
| `labels` | list&lt;string&gt; | `[]` | Test discovery labels |
| `timeout_ms` | int | none | Positive suite timeout in milliseconds, excluding configure and build |

List all configuration, source, header, fixture, and runtime inputs in `srcs`
or `data`. No native source discovery is performed. Configure upstream tests
using their native options and include every required native test executable
when narrowing `build_targets`. `jobs` reserves CPU slots and 1 GiB per job
for build and test execution within Struo's budget, since native CTest suites
can themselves compile integration fixtures. It also defaults
`CMAKE_BUILD_PARALLEL_LEVEL` for nested CMake builds. Explicit environment
overrides remain the caller's responsibility.

## Dependency Edges

| Edge | Accepts | Description |
| --- | --- | --- |
| `deps` | `cmake_project` | A `cmake_project` with `export_build_tree = true`. CTest runs in its build tree instead of a tree configured and built for this target. Cannot be combined with `test_targets` or `coverage_per_test` |

## Cache and execution boundary

The complete native build tree is a declared output, but configure and build
always bypass the cache. CMake can select compilers, toolchain files, SDKs,
and other external configuration that Struo cannot safely identify from the
declared inputs. Reusing an older executable after those selections change
would be incorrect. Each invocation cleans, configures, and rebuilds the
native tree, including after output deletion; there is no native build-tree
cache restoration or incremental reuse.

The tree contains absolute source and build paths. Its action identity still
includes a hash of the absolute workspace root, but that identity is not a
complete compiler fingerprint or a cross-workspace cache contract.

The driver verifies that the actual execution root equals the original
workspace root. Relocated sandboxes and remote workers are unsupported and
fail explicitly before configure or test execution. Run locally with the
default sandbox policy. No compiler stripping or debug-information removal
is performed. Native build-tree metadata is not claimed byte-reproducible.

CTest and custom test target execution are also always uncached, so tests with
undeclared external state cannot be silently replaced by an older passing result.

## Results and inventory

The target exposes `build`, `test`, and `struo_test_info`. Normalized results
contain exactly one suite case, not one case per native CTest entry. The log
and native JUnit report retain native case names, counts, failures, and output.
Custom test targets retain `cmake-test.log` instead of `ctest.log` and expose no
native JUnit artifact; their `native_results` list is empty.
When `timeout_ms` expires, Struo retains a normalized failed suite and a timeout
reason in the log; CTest may not finish writing its native JUnit report.
Without `coverage_per_test`, exact native case filtering, case listing,
sharding, retries, and coverage are unsupported. An exact test-unit request is rejected rather than silently
running a different selection. Use separate targets for independently
configurable upstream suites.
