# Together Code Sandbox

[Together Code Sandbox](https://docs.together.ai/docs/together-code-sandbox)
runs hosted sandboxes from prepared snapshots. Struo runs each uncached action
remotely in an ephemeral sandbox, then terminates it.

Struo transfers declared inputs into a clean directory, runs the command,
downloads and validates declared outputs after an accepted exit code, and
terminates the sandbox.

**Capabilities:** remote execution. Configure shared caching separately.

## Prerequisites

- Sandbox access for your organization, requested from Together. An API key
  alone does not establish access.
- A Linux snapshot containing your tools, created with Together's supported CLI
  or SDK. It must provide `sh`, `rm`, `mkdir`, and `tar` on `PATH`, a writable
  `/tmp`, and an agent supporting raw binary file reads. Its `tar` must support
  `--hard-dereference`, as GNU tar does.

For example, create a snapshot from a pinned Node.js image with Together's CLI:

```sh
together-sandbox snapshots create --image node:22.18.0-bookworm
```

Snapshot creation and execution may incur charges. Struo does not build
snapshots or install the Together SDK.

## Configuration

Copy the resulting snapshot ID into the repository root `struo.toml`:

```toml
[infrastructures.remote_tests]
kind = "together"
snapshot = "<snapshot-id>"

[infrastructure.execution]
provider = "remote_tests"
```

Use a snapshot ID, not a mutable alias; the snapshot reference is part of the
action's cache identity. Keep repository dependencies, including
`node_modules`, as declared inputs and outputs instead of baking them into the
snapshot.

### Authentication

Set `TOGETHER_API_KEY` outside the script. `STRUO_TOGETHER_API_KEY` takes
precedence when both variables exist. Struo does not add this key to the
action's environment or cache identity.

`STRUO_TOGETHER_API_URL` overrides the management API origin, whose default is
`https://api.bartender.codesandbox.io`.

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
Struo preserves executable modes and safe symbolic links. Native dependencies
must match the guest's Linux architecture and runtime. To produce
`node_modules` in a separate cacheable action, see
[Remote Execution](/guide/infrastructure/remote-execution).

## Limits and cleanup

- Stdout and stderr are preserved as binary and appear after the command
  finishes, not live. Update the snapshot if Struo reports an agent without
  raw-download support.
- The default sandbox size is 1 vCPU and 2 GB; Struo's resource declarations do
  not resize the guest.
- Output-idle timeouts, network denial, output path redirection, and graph
  filesystem setup are rejected instead of ignored.

Struo terminates the sandbox after success, failure, transfer errors, or
cancellation and waits for Together to confirm. It requests no termination
snapshot. Each sandbox also has a TTL, ten minutes without an action timeout or
the timeout plus two minutes, in case the client disappears.

A lost create response or interrupted cleanup stays recorded in the workspace's
`.struo` runtime state, and a later run finishes cleanup for sandboxes carrying
Struo's ownership tag. Struo never retries creation.

## Next steps

- [Remote Execution](/guide/infrastructure/remote-execution): split dependency
  installation and tests into separate cacheable actions.
- [Infrastructure](/guide/infrastructure#available-providers): pick a cache
  provider so other machines reuse remote results.
- [Execution provider contract](/reference/infrastructure/execution): see what
  every execution provider guarantees.
