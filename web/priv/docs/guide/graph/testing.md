# Testing and Scheduling

Run tests with Struo across pytest, Jest, Vitest, RSpec, Rust, and more, select
affected tests from changed paths, and split stable batches across local
workers or CI shards.

Struo keeps the native runner in charge of collecting and executing tests. It
records stable test identities around the runner, so it can rerun one unit,
divide a target into batches, and replay the same work on another machine.

## Run your first tests

Choose the target kind that matches your runner and make its prerequisite
available:

| Runner | Target declaration | Prerequisite | Default automatic batch |
| --- | --- | --- | --- |
| pytest | [`pytest_test`](/reference/prelude/pytest_test) | The selected Python interpreter can import pytest | One test file |
| [Ruby Specification](https://rspec.info/) | [`rspec_test`](/reference/prelude/rspec_test) | The selected Ruby interpreter can load the runner | One specification file |
| Minitest | [`minitest_test`](/reference/prelude/minitest_test) | The selected Ruby interpreter can load Minitest | One test file |
| Vitest | [`vitest_test`](/reference/prelude/vitest_test) | The package-local Vitest entry point is installed | One test file |
| Jest | [`jest_test`](/reference/prelude/jest_test) | The package-local Jest entry point is installed | One test file |

Build-system ecosystems such as Rust, Go, Android, Apple, and Elixir declare
their test targets in their own [ecosystem guides](/guide/graph/ecosystems).

Each target kind ships a runnable starter. Retrieve the one that matches your
runner for a complete manifest and source layout:

```sh
# Python with pytest
struo query example pytest_test pytest-test-minimal --format json

# Ruby Specification
struo query example rspec_test rspec-test-minimal --format json

# Ruby with Minitest
struo query example minitest_test minitest-test-minimal --format json

# JavaScript or TypeScript with Vitest
struo query example vitest_test vitest-test-minimal --format json

# JavaScript or TypeScript with Jest
struo query example jest_test jest-test-minimal --format json
```

The bundled starters name their target `tests`. Query the kind's schema, run
the target once, inspect what it discovered, then run it again across workers:

```sh
struo query schema pytest_test
struo test tests --format json
struo query test-manifest tests --format json
struo test tests --jobs 4 --format json
```

Replace `pytest_test` with your kind. The first complete run discovers stable
units and writes the manifest. Its JSON response shows the executed work in
`plan` and the batches available to the next run in `next_plan`. The next run
schedules those units as independent file batches across the requested
workers.

Python, Ruby, and Node.js executable attributes accept names on the executable
search path, absolute paths, and workspace-relative paths. A pytest target
with no `python` attribute tries `.venv/bin/python` before the executable
search path. Set a project path explicitly when the repository uses a
different runtime layout.

In a discovered native workspace, `struo test` with no target runs the
workspace's first-party tests, and `struo test --all` runs every discovered
test target through the dynamic scheduler.

## Discover the test surface

List test targets before choosing one:

```sh
struo query tests --format json
```

Run a complete target once to establish current discovery data:

```sh
struo test tests/unit --format json
struo query test-manifest tests/unit --format json
```

The manifest reports:

- `source`, which distinguishes normalized discovery from a whole-target
  fallback;
- `listing_supported`, which states whether the target kind can expose stable
  units;
- `case_filtering`, which is `runner_args` only when exact execution is safe;
- `sharding`, which states whether Struo may create automatic batches and
  whether their granularity is `file`, `case`, or `target`;
- `discovery_fingerprint`, which ties the unit list to the declared inputs
  that can change discovery; and
- `units`, the stable identifiers observed in the latest complete run.

A filtered run does not replace the complete manifest, so running one case for
quick feedback does not forget the other cases.

## Run one exact unit

Choose an identifier returned by the current complete manifest:

```sh
struo query test-manifest tests/unit --format json

struo test tests/unit \
  --test-unit 'tests/unit::returns_greeting' \
  --format json
```

Struo runs the unit only when the target declares
`case_filtering = "runner_args"` and the identifier is in the persisted
complete manifest. Otherwise the request fails during planning. Run the whole
target again when discovery is missing or stale, and do not construct
runner-specific filters from names Struo did not return.

## Rerun one batch

A batch is a stable group of test units, not a worker number. Inspect the
current target plan and copy a batch's `id`:

```sh
struo query test-plan --target tests/unit --format json
struo test tests/unit --test-batch '<batch-id>' --format json
```

`--test-batch` requires one explicit target and cannot be combined with
`--test-unit`, `--all`, or changed-path selection. A missing or obsolete id
fails with current batch suggestions. Changed discovery inputs can make an old
batch unavailable until a whole-target run refreshes the manifest.

Replaying a file batch keeps every case in that file together, and the worker
count does not change the batch id. A rerun reproduces the scope of the work,
not necessarily its outcome after code or environment changes.

## Inspect retained results

Read a target's latest normalized report, or only its totals:

```sh
struo query test-results tests --format json
struo query test-results tests --summary-only --format json
```

Inspect one case, and optionally one attempt, without returning every case:

```sh
struo query test-results tests --case 'suite::case_name' --format json
struo query test-results tests --case 'suite::case_name' --attempt 1 --format json
```

These reads do not evaluate the graph or run tests, so they work even when the
current manifests are broken. `--attempt` is a one-based position in the
case's retained attempts and defaults to the last one; it requires `--case`,
and `--case` cannot be combined with `--summary-only`. The response includes a
suggested rerun command for the whole target under the current configuration.
An unknown case or attempt fails with suggestions for valid ids or the valid
attempt range.

Review recent attempts and measured durations:

```sh
struo query test-attempts --limit 20 --format json
```

## Plan affected tests before running

Inspect which tests a change affects before running them:

```sh
struo query test-plan \
  --changed-path src/library.rs \
  --format json
```

Affected selection is conservative. Struo starts from the changed inputs,
walks reverse dependencies, and selects complete test targets, each with a
reason. Paths with no known owner also select tests instead of skipping them.
When a selected target has a current manifest and supports exact filtering,
the plan divides it into file or case batches; it never omits a discovered
case.

Run the same selection across workers:

```sh
struo test \
  --changed-path src/library.rs \
  --jobs 4 \
  --format json
```

`--jobs` caps concurrent local workers. It does not change selection, plan
identity, or batch identity.

## Coverage-assisted selection

With a [Tuist project](/guide/infrastructure/tuist) configured, you can opt
into approximate selection that excludes whole test targets whose recorded
coverage does not touch the changed paths. Graph-based selection stays the
default.

Set `coverage_per_test = true` on a supported test target: Go, pytest, RSpec,
Minitest, Jest, Vitest, Rust libtest, JVM or Android host tests, ExUnit,
CTest, Zig, Bazel, ShellSpec, or Apple host bundles. Cargo, SwiftPM, Xcode, Nx,
and Turborepo workspaces forward the opt-in to their generated test targets.
Each target kind reference documents its collector and prerequisites.

Record a full run, then export and run a plan for a clean, committed change:

```sh
struo test --all --coverage-report --format json
struo query test-plan --changed-path lib/source.go --coverage-selection --format json > plan.json
struo test --plan plan.json --format json
```

Keep plan files outside tracked inputs or in a gitignored directory. Struo
removes a target only when Tuist proves the change misses its whole current
inventory; it never excludes individual cases. Missing or incompatible
history, coverage hits, changed test inputs, added, deleted, or renamed paths,
dirty checkouts, and provider failures keep the graph-selected work.

Run from the repository root with a consistent `origin` URL, and fetch enough
Git history on CI to contain the baseline; Struo does not deepen shallow
clones. Running a selected plan does not refresh the baseline, so publish a
new full run with `--coverage-report` when you want to update history, and
keep regular full test runs to catch dependencies coverage did not observe.

## Tune batching

Struo orders batches by recent durations and lets idle workers pull the next
one, so the plan does not depend on the worker count.

Dynamic-language runners default to file batches because tests in one file
often share imports, fixtures, and setup. Set `batching = "case"` on a pytest,
Ruby Specification, Vitest, or Jest target when its fixtures and startup costs
make case-level batches worthwhile. Set `batching = "target"` when repeated
startup or shared fixtures cost more than parallel execution saves. More
workers do not force smaller batches, and more batches do not guarantee a
faster run.

Concurrent batches write results and logs to isolated directories, and Struo
merges them into one result per target. A batch fails when its runner ignores
a filter, reports a unit twice, or exits successfully without valid passing
results. Periodic whole-target runs help catch tests the runner stopped
reporting.

## Replay saved plans on CI workers

Export the plan after a complete discovery run:

```sh
struo test tests/unit --format json
struo query test-plan --target tests/unit --format json > plan.json
```

Distribute the file unchanged through your CI coordinator. Each fresh worker
runs its assigned batch ids from `plan.batches`:

```sh
struo test --plan plan.json \
  --batch '<first-batch-id>' --batch '<second-batch-id>' \
  --jobs 4 --format json
```

Omit `--batch` to run the complete plan. A repeated or unknown id fails before
execution. `--jobs`, `--memory-limit`, `--cpu-limit`, and `--sandbox` work as
usual, but `--plan` cannot be combined with target selectors or `--config`;
the worker uses the configuration recorded in the plan.

Struo rejects a plan whose sources, configuration, dependencies, environment
values, runner versions, or test arguments changed since export, so export a
new plan after such changes. Plans detect changes, not authorship: fetch them
from a trusted, authenticated coordinator and treat them like repository
configuration.

Replay prints a `struo.portable_test_execution.v1` document:

```json
{
  "schema": "struo.portable_test_execution.v1",
  "plan_id": "<portable-plan-id>",
  "selected_batch_ids": ["<batch-id>"],
  "complete": true,
  "success": false,
  "execution": {"plan": {}, "next_plan": {}, "schedule": {}, "runs": []}
}
```

Each `runs` entry includes its `batch_id`, `target`, `exit_code`, `success`,
and normalized `results`. Every assigned batch runs even when tests fail.
`complete` means every selected batch finished; it does not mean tests passed.
Pass a CI partition only when the plan id and batch ids match, `complete` and
`success` are both `true`, and the process exits successfully. Validation
errors or interruptions can leave no final document and must fail the
partition.

To attribute runs to a hosted test report, add
`--external-group <UUID> --partition-index <N>`, where the index is a
nonnegative integer. The coordinator then owns one consolidated report per
partition.

## Distribute tests through Tuist

Tuist can store the plan and assign shards for you. Configure the Tuist
provider in the root `struo.toml`:

```toml
[infrastructure.cache]
provider = "tuist"

[infrastructures.tuist]
kind = "tuist"
account = "acme"
project = "app"
```

The default server is `https://tuist.dev`; set `url` on the provider for
another server. Struo reuses Tuist OAuth credentials, `TUIST_TOKEN`, or
supported CI OIDC authentication. No Tuist executable is needed.

After complete discovery, the planning job uploads the plan and prints the
shard matrix:

```sh
struo query test-plan --shards 4 --reference "$CI_EXECUTION_ID" --format json
```

The output is `{"shard_plan_id":"<UUID>","shards":[0,1,2,3]}`. Build your CI
matrix from the returned indices; an empty plan returns an empty id and
array. `--target` and `--changed-path` select work as usual. The reference
defaults to `TUIST_SHARD_REFERENCE` or the detected CI run; pass it explicitly
outside supported CI environments.

Each worker runs its shard with the exact plan id:

```sh
struo test --shard-plan "$SHARD_PLAN_ID" --shard-index "$SHARD_INDEX" --jobs 4
```

`TUIST_SHARD_PLAN_ID` and `TUIST_SHARD_INDEX` can supply these values. Struo
downloads and validates the plan, runs every assigned batch, and submits one
report. An empty assignment runs nothing. Missing results, failed processes,
and interrupted work fail the shard.

To rerun a shard, increase `--shard-attempt`. It defaults to
`TUIST_SHARD_ATTEMPT`, then the GitHub run attempt, then 1. Increase
`--shard-report-revision` (default 0) to submit a corrected report within the
same attempt. These counters are separate from retries within a test case.

## Add selection to a project module

Project-local Starlark test kinds get the same discovery, exact-unit,
affected-plan, and scheduling commands as built-in kinds. Query the live
contract instead of copying a provider shape:

```sh
struo query module-contract --format json
```

The returned `test_starter`, `test_target_starter`, and
`normalized_test_result_example` show the provider, manifest table, and
result format. The adapter receives:

- `--struo-results <path>`;
- `--struo-log <path>`;
- `--struo-target <target>`; and
- one repeated `--struo-test-unit <identifier>` for each exact filter.

The adapter writes `struo.test_results.v1`, uses stable target-qualified case
identifiers, honors every requested filter, and exits unsuccessfully for a
failed or unknown selection. Validate the module and workspace before the
first run:

```sh
struo query validate-module modules/testing.star --format json
struo query validate-workspace --format json
```

See the [modules reference](/reference/modules/) for the full authoring
contract.

## Use tests from coding agents

With `--format json`, a failed command prints a `struo.error.v1` record on
standard output and keeps logs on standard error, so agents can use the exit
status for control flow and `error.message` for repairs. Read normalized
results and attempts instead of scraping runner output.

The same operations are available as
[Model Context Protocol](https://modelcontextprotocol.io/) tools, including
`struo_query_test_plan`, `struo_run_tests` (which accepts `test_batch`), and
the read-only `struo_query_test_results`. The
[tool reference](/reference/mcp/tools) maps each tool to its command.

## Limitations

Unit discovery and exact execution vary by target kind:

| Target kind | Stable unit discovery | Exact unit execution | Automatic granularity |
| --- | --- | --- | --- |
| [`pytest_test`](/reference/prelude/pytest_test) | Yes | Yes | File by default, optional case |
| [`vitest_test`](/reference/prelude/vitest_test) | Yes | Yes | File by default, optional case |
| [`jest_test`](/reference/prelude/jest_test) | Yes | Yes | File by default, optional case |
| [`rspec_test`](/reference/prelude/rspec_test) for [Ruby Specification](https://rspec.info/) | Yes | Yes | File by default, optional case |
| [`minitest_test`](/reference/prelude/minitest_test) | One unit per file | Yes, by file | File |
| [`rust_test`](/reference/prelude/rust_test) | Yes | Yes | Target |
| [`android_local_test`](/reference/prelude/android_local_test) | Yes | Yes | Target |
| [`android_instrumentation_test`](/reference/prelude/android_instrumentation_test) | Yes, from a completed device run | Yes, through the instrumentation `class` argument | Target |
| [`apple_test_bundle`](/reference/prelude/apple_test_bundle) with Swift Testing | Yes | No | Target |
| [`shellspec_test`](/reference/prelude/shellspec_test) | Yes | No | Target |
| [`zig_test`](/reference/prelude/zig_test) | Suite-level fallback | No | Target |
| [`elixir_test`](/reference/prelude/elixir_test) | Summary counts only | No | Target |

Kinds without exact execution still take part in affected selection and
target-level scheduling.

Coverage-assisted selection has its own limits:

- Android device instrumentation has no coverage collector and keeps
  graph-selected work.
- Apple coverage is validated on macOS host bundles, not simulators or
  devices.
- CTest, Zig, Bazel, ShellSpec, and Apple bundles measure whole invocations,
  not individual native cases.
- Nx and Turborepo tasks reuse the Jest or Vitest collector only for a single
  native command without task outputs, shell expansion, or plugin executor
  wrapping.
- Collecting coverage bypasses the execution cache.

## Next steps

- [Tuist](/guide/infrastructure/tuist): configure the project used for
  coverage history and hosted shards.
- [`struo test` reference](/reference/cli/test): see every selection and
  scheduling flag.
- [Linting](/guide/graph/linting): add cacheable analyzers beside your tests.
- [Ecosystems](/guide/graph/ecosystems): declare test targets for your build
  system.
