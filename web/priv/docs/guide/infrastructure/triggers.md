# Triggers

Declare triggers in `struo.toml` to choose which targets CI builds, tests, or
runs when a branch changes, and let your CI service schedule the workers.

A build graph already describes what to build, how to test it, and what must run
first. A trigger selects from that graph instead of describing the same work
again as CI jobs, so changing a target's dependencies does not require editing a
second pipeline.

::: warning Experimental worker interface
Triggers are experimental. No CI service reads them natively yet; you connect a
service with your own adapter, such as the
[Buildkite example](/guide/infrastructure/buildkite). Trigger runs execute real
project code with the worker's permissions. Read the
[limitations](#limitations) before using them in CI.
:::

## Declare a trigger

Add a trigger to your root `struo.toml`:

```toml
[[trigger]]
id = "verify-main"
on = { event = "vcs.ref_changed", refs = ["refs/heads/main"] }

[[trigger.invoke]]
target = "checks/core"
capability = "test"
configuration = { os = "linux", arch = "x86_64", tokens = [] }
```

This runs the `test` capability of `checks/core` when `main` changes. Struo
handles the target's dependencies, actions, and caching; the declaration does
not repeat the commands used to build or test it.

- `id` is the trigger's stable name.
- `refs` contains exact, fully qualified refs, not branch patterns.
- `target` must already exist and provide the selected capability. Triggers
  support `build`, `test`, and `run` on analysis-backed target kinds. Use
  `struo query capabilities <target>` to check a target.
- `configuration` sets the target OS, architecture, and extra configuration
  tokens. The service must provide a compatible worker.

Add more `[[trigger.invoke]]` entries to select more work. See
[target configuration](/guide/graph/configuration) and
[test execution](/guide/graph/testing) for what the selected work does.

## Inspect and edit triggers

These commands read, validate, or edit trigger declarations without running
project modules:

```sh
struo query triggers
struo query validate-trigger --file draft.json
struo edit triggers --file changes.json
```

`query triggers` includes JSON schemas for trigger drafts and event files.
Edits use `{ "operations": [...] }` with `create`, replacement `update`, or
`delete` operations. An invalid batch leaves the manifest unchanged.

The same operations are available as [MCP tools](/reference/mcp/tools).
Editing, activation, and execution through MCP require `struo mcp --allow-run`.

## Run a trigger on a CI worker

Edited declarations take effect only once activated from a committed revision.
On a clean checkout, activate the approved revision, then run a trigger with an
event document:

```sh
struo edit activate-triggers --revision <full-commit-id>
struo run --trigger verify-main --event event.json
```

The event file holds `event`, `ref`, and the full `revision`, for example
`{"event": "vcs.ref_changed", "ref": "refs/heads/main", "revision": "<full-commit-id>"}`.
The worker's clean `HEAD` must match that revision. A missing capability or a
failed invocation fails the run.

## Connect a CI service

A service integration does three things:

1. **Register approved triggers.** Read a reviewed, committed definition
   revision separately from the commit being tested, so a candidate cannot skip
   its own tests by deleting or narrowing its trigger.
2. **Select and schedule work.** Turn the service's authenticated event into a
   source ref and full commit ID, match the registered triggers, and schedule
   compatible workers. Reading trigger data does not evaluate project modules.
3. **Run Struo and publish the result.** On a clean checkout of that commit,
   run the selected trigger. The service owns job lifecycle, isolation,
   credentials, and required checks.

These services can host an adapter today:

- **[Buildkite](/guide/infrastructure/buildkite): example adapter.** Generates
  one worker job per matching trigger and uploads it to the running build, with
  no repository pipeline YAML.
- **[Woodpecker CI](https://woodpecker-ci.org/docs/usage/extensions/configuration-extension):**
  a configuration extension returns generated worker pipelines. Exclusive mode
  needs no repository pipeline file.
- **[Jenkins](https://www.jenkins.io/doc/book/pipeline/getting-started/):** a
  centrally configured Scripted Pipeline creates parallel workers from trigger
  data, with no application `Jenkinsfile`.
- **[GitLab CI](https://docs.gitlab.com/ci/pipelines/downstream_pipelines/#dynamic-child-pipelines):**
  generate a child pipeline. A parent pipeline remains necessary.
- **[CircleCI](https://circleci.com/docs/guides/orchestrate/dynamic-config/):**
  generate continuation configuration. A setup configuration remains necessary.
- **[GitHub Actions](https://docs.github.com/en/actions/reference/evaluate-expressions-in-workflows-and-actions#fromjson):**
  expand a job matrix from JSON with `fromJSON`. A workflow file and its event
  subscriptions remain necessary.
- **[Azure Pipelines](https://learn.microsoft.com/en-us/azure/devops/pipelines/process/phases?view=azure-devops#multi-job-configuration):**
  expand a runtime JSON matrix. A pipeline file and authorization for eligible
  agent pools remain necessary.

Struo ships no adapter for these services except the Buildkite example.
Generating configuration for a service's API is fine; maintaining another copy
of the build graph is not.

Keep service credentials and privileged pipeline code outside the candidate's
control. Woodpecker extensions and Jenkins trusted libraries, for example, can
hold credentials or controller privileges, so neither should evaluate project
code to select work. For matrix-based adapters, keep a stable required check
that fails when discovery or a worker fails, and handle the case where no
trigger matches explicitly: an empty matrix does not reliably mean zero jobs.

## Limitations

- Struo does not receive webhooks, provision workers, or publish CI checks.
- The run mode is called simulation, but it executes real project code with the
  worker's permissions. The event file and local activation do not prove where
  an event came from and are not a security boundary. Isolate execution before
  checkout and tool setup, keep credentials outside it, and enforce resource
  limits in CI.
- Trigger runs use a dedicated local cache, not the configured shared cache or
  live reporter.
- Runs have no immutable replay or crash recovery. Do not use them for
  deployments or other external effects.

## Next steps

- [Buildkite Dynamic Pipelines](/guide/infrastructure/buildkite), for a
  complete adapter you can adapt to your CI.
- [Testing and Scheduling](/guide/graph/testing), to define the test capability a trigger
  selects.
- [`struo run` reference](/reference/cli/run), for the trigger and event
  options.
