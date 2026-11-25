# Runloop Devboxes

[Runloop Devboxes](https://docs.runloop.ai/docs/devboxes/overview) are hosted
Linux machines. Struo runs each uncached action remotely in a fresh Devbox from
your Blueprint, then shuts it down.

Struo uploads declared inputs, runs the command, retrieves declared outputs
after an accepted exit code, and shuts down the Devbox.

## Capabilities

| Capability | Support |
| --- | --- |
| Shared cache | No; use a [cache provider](/guide/infrastructure/#available-providers) |
| Remote execution | Hosted Devboxes from a Blueprint |
| Artifact publication | No |
| Project setup | Manual [configuration](#configuration) |
| Authentication | `RUNLOOP_API_KEY` |

Local protocol tests cover this provider; live account access, execution and
teardown are unverified.

## Prerequisites

- A Runloop account with Devbox creation, listing, execution, file access and
  shutdown permissions.
- A Blueprint containing your runtime, Python 3 with isolated-mode support and
  its standard library (`python3` on the default `PATH`), `sh`, `env`, `rm`,
  `mkdir`, and `tar` supporting `--hard-dereference`. The default user needs a
  writable `/home/user`. Keep credentials out of the Blueprint.

## Configuration

Bind a prepared Blueprint ID, not a mutable Blueprint name, in the repository
root `struo.toml`:

```toml
[infrastructures.remote_tests]
kind = "runloop"
blueprint = "bpt_your_node_blueprint"

[infrastructure.execution]
provider = "remote_tests"
```

The built-in `--compute runloop` selection uses Runloop's mutable default image.
Prefer a named infrastructure with a Blueprint ID for cacheable workflows.

### Authentication

Set `RUNLOOP_API_KEY` outside your scripts. `STRUO_RUNLOOP_API_KEY` takes
precedence. Struo does not inject the key into the command environment.

`STRUO_RUNLOOP_URL` overrides the API base, which defaults to
`https://api.runloop.ai/v1`. HTTPS is required except for loopback tests.

## Usage

Run a literal command on the provider:

```sh
struo exec --remote --compute remote_tests -- node --version
```

Omit `--compute remote_tests` to use the workspace's default execution provider.

### Run an annotated test script

Save this script as `scripts/test.sh`:

```sh
#!/bin/sh
# struo remote "remote_tests"
# struo cwd ".."
# struo input "../node_modules"
# struo input "../package.json"
# struo input "../tests"
# struo output "../reports"

node node_modules/vitest/vitest.mjs run \
  --reporter=json --outputFile=reports/vitest.json
```

Run it:

```sh
struo exec --script -- sh scripts/test.sh
```

Declare any additional source, configuration, or setup files your tests read.
Struo includes declared `node_modules` even when Git ignores it, preserving
executable modes and safe symbolic links. Native dependencies must match the
Devbox's Linux architecture and runtime. To produce `node_modules` in a
separate cacheable action, see
[Remote Execution](/guide/infrastructure/remote-execution).

## Limits and cleanup

- Stdout and stderr are preserved as binary within a combined capture limit and
  appear after the command completes, not live.
- One action deadline covers preparation, provisioning, transfers, execution
  and output installation, ten minutes by default. The Devbox keep-alive is the
  action timeout plus two minutes, within Runloop's 48-hour limit.
- Runloop uses the Blueprint's launch defaults for machine size; Struo's
  resource declarations do not resize the Devbox. Choose a Blueprint with
  enough memory and disk for the action and its dependencies.
- Network denial, explicit account/project scoping, output path redirection,
  explicit filesystem setup and output-idle timeouts are unsupported and fail
  instead of running with weaker guarantees.

After success, failure, timeout or cancellation, Struo forces a shutdown and
waits for it to finish. Shutdown ends active compute but does not promise
erasure of retained records. Struo does not create snapshots or suspend
Devboxes. The finite keep-alive stops the Devbox if the client disappears.

Failed shutdowns and uncertain creates stay recorded in the workspace's
`.struo` runtime state for a later run. Keep that state and the original
account access available for cleanup.

## Next steps

- [Remote Execution](/guide/infrastructure/remote-execution): split dependency
  installation and tests into separate cacheable actions.
- [Infrastructure](/guide/infrastructure#available-providers): pick a cache
  provider so other machines reuse remote results.
- [Execution provider contract](/reference/infrastructure/execution): see what
  every execution provider guarantees.
