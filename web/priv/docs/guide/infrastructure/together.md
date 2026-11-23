# Together Code Sandbox

[Together Code Sandbox](https://docs.together.ai/docs/together-code-sandbox)
runs commands in hosted sandboxes created from prepared snapshots. Your
organization must request sandbox access from Together before using this
provider. An API key alone does not establish access.

**Capabilities:** remote execution. Configure shared caching separately.

## Prerequisites

Use Together's supported CLI or SDK to create a Linux snapshot containing your
tools. The snapshot must provide `sh`, `rm`, `mkdir`, and `tar` on `PATH`, a
writable `/tmp`, and an agent supporting raw binary file reads.
The snapshot's `tar` must support `--hard-dereference`, as GNU tar does. Struo
uses it to restore hard-linked output files as independent regular files while
preserving symlinks. Unsupported tar implementations fail output collection.

For example, create a snapshot from a pinned Node.js image with Together's CLI:

```sh
together-sandbox snapshots create --image node:22.18.0-bookworm
```

Snapshot creation and execution may incur charges. Struo does not build
snapshots or install the Together SDK.

## Configuration

Copy the resulting
snapshot ID into the workspace's `struo.toml`:

```toml
[infrastructures.remote_tests]
kind = "together"
snapshot = "<snapshot-id>"

[infrastructure.execution]
provider = "remote_tests"
```

Use a snapshot ID, not a mutable alias. Struo includes the provider binding and
snapshot reference in the action's cache identity. Keep repository dependencies,
including `node_modules`, as declared inputs and outputs instead of baking them
into the snapshot. See [Remote Execution](/guide/infrastructure/remote-execution)
for a dependency-installation and test workflow.

### Authentication

Set `TOGETHER_API_KEY` outside the script. `STRUO_TOGETHER_API_KEY` takes
precedence when both variables exist. Struo does not add this key to the action's
environment or cache identity.

## Usage

```sh
struo exec --remote --compute remote_tests -- node --version
```

An annotated script can select the same provider with
`# struo remote "remote_tests"`.

Struo transfers
declared inputs, executable modes, and safe symlinks into a clean execution
directory. After an accepted exit code it downloads, validates, and installs
only declared outputs. Unsafe archives fail before installation.

Struo preserves binary stdout and stderr. Output appears after the command
finishes, rather than live. Update the snapshot if Struo reports an agent
without raw-download support.

## Limits and cleanup

Struo creates one ephemeral sandbox for an action that misses the cache. It
sets a TTL of ten minutes without an action timeout, or the timeout rounded up
to seconds plus two minutes. It requests no termination snapshot and explicitly
terminates after success, failure, transfer errors, or cancellation. Cleanup
waits for Together to confirm termination before making execution capacity
available again.

Transfers and captures respect Struo's resource limits.
The provider's default sandbox size is 1 vCPU and 2 GB; action resource
estimates govern coordinator admission, not guest sizing.

Together execution does not support command-output idle timeouts, network
denial, output-path redirection, or graph command filesystem setup. Struo
rejects these requirements instead of ignoring them.

Struo journals creation intent and an ownership tag before sending the create
request. A later run can recover a lost create response or interrupted cleanup
by looking up that tag. It does not retry creation or delete sandboxes with
other ownership tags. Failed or inconclusive recovery keeps the journal entry.
TTL provides a separate safety net if the client or journal disappears.

`STRUO_TOGETHER_API_URL` overrides the management API origin, whose default is
`https://api.bartender.codesandbox.io`.
