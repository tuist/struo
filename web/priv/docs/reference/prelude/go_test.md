# `go_test`

Compiled Go test package with exact filtering, normalized results, native event
records, and optional coverage.

The target accepts every [common Go attribute](/reference/prelude/go_source#common-go-attributes),
the link attributes from [`go_binary`](/reference/prelude/go_binary#attributes),
and these test-specific attributes:

| Attribute | Type | Default | Description |
| --- | --- | --- | --- |
| `args` | list&lt;string&gt; | `[]` | Arguments passed to the compiled test binary |
| `test_env` | map&lt;string, string&gt; | `{}` | Test environment values |
| `env_inherit` | list&lt;string&gt; | `[]` | Host environment names inherited by the runner |
| `rundir` | string | empty | Package-relative working directory for the test binary |
| `labels` | list&lt;string&gt; | `[]` | Labels exposed through generic test discovery |
| `timeout_ms` | int | empty | Optional test timeout in milliseconds |
| `cover_packages` | list&lt;string&gt; | `[]` | Import path patterns included in coverage instrumentation |
| `coverage_per_test` | bool | `false` | Collect isolated top-level test coverage for opt-in Tuist selection; defaults instrumentation to `./...` when `cover_packages` is empty |
| `fail_fast` | bool | `false` | Stop after the first failing test |
| `short` | bool | `false` | Request Go short test mode |
| `count` | int | `0` | Run selected tests this many times when greater than zero |
| `parallel` | int | `0` | Maximum tests run concurrently inside the test binary |
| `shuffle` | string | empty | `on`, `off`, or a shuffle seed |

## Dependency Edges

The target accepts the normal `deps`, `embed`, and `cdeps` roles. Its
`target_under_test` role accepts `go_package` and composes the Buck2-compatible
library or binary sources into the test package.

## Providers, Capabilities, and Outputs

The target emits `go_package` and `struo_test_info`.

| Capability | Output groups | Requires |
| --- | --- | --- |
| `build` | `binary` | none |
| `test` | `default`, `test_results`, `logs`, `coverage` | `binary` |
| `test-discovery` | `test_inventory` | `binary` |

The test action writes `test_results.json`, `go-test.log`, and
`native_results.jsonl` under `.struo/out/<target>/test/`. With coverage enabled,
it also writes `coverage.out` and records it in the normalized artifacts map.
Benchmarks are discovered as skipped units during a normal test run and can be
selected exactly later. Test execution requires the host operating system and
architecture.

With `coverage_per_test = true`, Struo runs each top-level test in a separate
process and attributes its profile to that invocation's opaque unit ID. It
does not attribute a shared profile to dynamic subtests. Dynamic subtests,
skipped tests, benchmarks, failed tests, missing profiles, and unresolved
import paths prevent complete selection evidence. Profiles from subprocesses
and dependencies outside the local module do not establish complete evidence.
Complete observations require Linux with `strace` and permission to trace the
test process. Subprocess execution, missing or incomplete traces, and unsupported
hosts prevent complete evidence. Struo probes tracing before executing the test.
When the tracer is missing or unusable, tests still run once and report partial
coverage, retaining graph-selected work. Tracing ends when the primary test
process exits, without waiting for or terminating background helpers.
Benchmark selectors and exact benchmark filters use the ordinary runner, preserving
benchmark execution and failures without publishing complete per-test evidence.
Compilation can use the cache; Struo measures coverage again on each run.

Use `struo test --all --coverage-report` to publish complete target observations
to a configured Tuist project. See [coverage-assisted selection](/guide/graph/testing#coverage-assisted-selection)
for the opt-in planner and its safety limits.
