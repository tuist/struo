# Remote Execution

Run Struo commands, annotated scripts, and build graph actions in fresh sandboxes
from an execution provider, with inputs, outputs, and caching declared in your
repository.

The action stays the boundary: your repository declares what an action reads,
runs, and produces, and the provider only supplies the environment it runs in.
Cache hits never reach the provider.

## Run a command remotely

Name an execution provider in the repository root `struo.toml` and select it as
the default:

```toml
[infrastructures.remote_tests]
kind = "e2b"
template = "vitest-node-22"

[infrastructure.execution]
provider = "remote_tests"
```

Then run a literal command through it:

```sh
struo exec --remote -- node --version
```

Pick the provider whose environment fits your work from the
[provider list](/guide/infrastructure/#available-providers); each guide shows
its `kind` and required fields.

## Run scripts remotely

Annotated scripts select a provider with `# struo remote "<name>"`. Treat
dependency installation and testing as separate actions. This install script
uses [npm](https://www.npmjs.com/), reads the package manifests, and produces
`node_modules`:

```sh
#!/bin/sh
# struo input "../package.json"
# struo input "../package-lock.json"
# struo output "../node_modules"
# struo cwd ".."
# struo remote "remote_tests"

npm clean-install --no-audit --no-fund
```

The test script depends on that action and consumes its output:

```sh
#!/bin/sh
# struo needs "./install.sh"
# struo input "../src"
# struo input "../tests"
# struo output "../reports"
# struo cwd ".."
# struo remote "remote_tests"

./node_modules/.bin/vitest run \
  --reporter=json \
  --outputFile=reports/vitest.json
```

The provider environment supplies Node.js, npm, and basic operating-system
tools; the install action supplies the exact repository dependencies, including
package-manager symbolic links and executable modes. A lockfile change reruns
installation. A source or test change reruns only the test action.

## Run build graph actions remotely

Declared build graphs dispatch each uncached command action to the selected
provider:

```sh
struo build --remote --compute <provider>
```

`struo run --remote` does the same for a target's `run` capability. Struo keeps
dependency scheduling and caching on your computer, and graph analysis must
select tools that exist in the provider's environment. Start with
[Docker](/guide/infrastructure/docker) to validate this locally in fresh
containers, or use [Tuist workers](/guide/infrastructure/tuist) for a hosted
fleet.

## Choose a provider per invocation

`--compute <provider>` with `--remote` selects a different named execution
provider for one command. Omit `--remote` to run on the current computer.

A named infrastructure with `kind = "local"` selects native execution with its
usual sandbox and network policies. Provider names are part of action identity,
so results from different providers, or from a `local` binding, are cached
separately.

## What happens to each action

For every uncached remote action, Struo:

1. Creates a fresh sandbox from the configured image, template, or snapshot.
   Use an immutable reference when reproducibility matters; a tag alone does
   not pin the environment.
2. Transfers only the declared input files and directories, with executable
   modes and symbolic links that resolve inside declared inputs.
3. Runs the declared arguments with the declared working directory, selected
   environment variables, resources, and timeout.
4. After a successful exit, downloads only the declared outputs and validates
   them before installing them locally.
5. Deletes the sandbox, whether the action succeeded or failed.

Provider access keys configure the transport only. They are not placed in the
action environment or included in the action identity.

E2B, Daytona, and Runloop transfer each direction as one archive, so their
image or template must provide `tar`. These adapters upload the complete set of
declared inputs on every cache miss. The Tuist adapter uploads only the input
blobs the project cache is missing and reads outputs back by digest, as
[Bazel remote execution](https://bazel.build/remote/rbe) does.

## Budget memory

`--memory-limit` bounds how much work Struo admits at once on your computer.
Each local action reserves its declared memory estimate, 250 MiB by default.
Microsandbox guests receive that reservation as their memory and CPU
allocation. A hosted action reserves a fixed amount of local memory for
packaging and transfers; the provider schedules the remote machine's own CPU
and memory. These budgets apply within one Struo invocation, not across
invocations or accounts.

## Limitations

- Sandbox providers cannot enforce `--network deny`, stdout or stderr path
  redirection, or a non-default output symlink policy. Struo rejects actions
  that require them instead of ignoring the requirement.
- Explicit execution account or project requirements are rejected; the
  configured access key selects the hosted account.
- Struo never falls back to another executor when a command or the
  infrastructure fails.
- Selecting a provider does not add a warm worker pool or a distributed
  artifact service.
- Transfers that make no progress for 30 seconds fail. A quiet command is not a
  stalled transfer.
- If a provider refuses to delete a sandbox after a successful action, the
  action fails. Interrupted runs, rotated credentials, or a lost workspace can
  still leave a sandbox behind, so keep the provider's own lifetime or cleanup
  policy enabled. Struo retries cleanup of sandboxes it created on a later
  hosted action. Local Microsandbox VMs have no such policy and may need manual
  cleanup after an interrupted run.
- Locally, `--network deny` needs Linux 5.11 or later with seccomp; Struo
  refuses the action when it cannot enforce the denial.

## Next steps

- [Artifacts](/guide/infrastructure/artifacts), to publish the immutable
  products of your builds.
- [Docker](/guide/infrastructure/docker), to validate remote graph dispatch on
  your own computer.
- [Execution provider contract](/reference/infrastructure/execution), for the
  exact boundary a provider must satisfy.
