# Boat

[Boat](https://docs.boat.dev/api/v1) provides hosted Linux sandboxes. Struo
runs each uncached action in a disposable Boat sandbox and deletes it
afterwards.

Struo creates the sandbox from a saved snapshot, uploads declared inputs, runs
the command, retrieves declared outputs after an accepted exit, and requests
permanent deletion.

## Capabilities

| Capability | Support |
| --- | --- |
| Shared cache | No; use a [cache provider](/guide/infrastructure/#available-providers) |
| Remote execution | Hosted sandboxes from a saved snapshot |
| Artifact publication | No |
| Project setup | Manual [configuration](#configuration) |
| Authentication | `BOAT_API_KEY` |

## Prerequisites

- A Boat account with sandbox creation, listing, command execution, file
  read/write, and permanent-deletion access.
- A saved Linux snapshot with your runtime, `sh`, `env`, `rm`, `mkdir`, and
  `tar`. Prepare it without secrets: Struo attaches no account repositories,
  secrets, or credentials, but files you placed in the snapshot remain.

## Configuration

Bind the saved snapshot by name in the repository root `struo.toml`:

```toml
[infrastructures.remote_tests]
kind = "boat"
snapshot = "node-tests-v1"

[infrastructure.execution]
provider = "remote_tests"
```

Select a saved snapshot, not an OCI image. Keep the snapshot name stable only
while its contents remain unchanged; replacing it under the same name does not
change Struo's action identity.

You can use Boat's current default Linux environment without a saved snapshot
by selecting `--compute boat`. That default is not an immutable toolchain.

Sandboxes are created with private visibility and without inherited account
environment, snapshots or monitoring, regardless of your organization's shared
sandbox defaults.

### Authentication

Set `BOAT_API_KEY` outside scripts and manifests. `STRUO_BOAT_API_KEY` takes
precedence. Struo does not forward this key to your command.

`STRUO_BOAT_API_URL` overrides the default `https://boat.dev/api/v1` endpoint
for compatible test services.

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
guest's Linux architecture and runtime. To produce `node_modules` in a separate
cacheable action, see [Remote Execution](/guide/infrastructure/remote-execution).

## Limits and cleanup

- Stdout and stderr are preserved as binary and appear after the command
  completes, not live.
- Boat uses the saved snapshot's machine size, or its default size without a
  snapshot. Struo's resource declarations do not resize Boat machines.
- Network denial, explicit account/project scoping, output path redirection,
  graph filesystem setup, and output-idle timeouts are not supported and fail
  instead of running with weaker guarantees.

### Cleanup and recovery

Struo requests permanent deletion after success, failure, timeout, or
cancellation. Boat's accepted deletion ends billing at once and erases stored
data in the background. Struo logs the deletion operation ID but does not wait
for erasure; check the operation in Boat if you need purge completion.

If deletion fails or a create response is lost, Struo keeps an ownership record
in the workspace's `.struo` runtime state and retries cleanup on a later run.
Records stay until Boat returns a matching permanent-deletion receipt, even for
sandboxes you deleted outside Struo. Enough unresolved records block new
sandboxes rather than silently dropping ownership.

A time-to-live is a fallback: one hour without an action timeout, otherwise the
timeout rounded up plus five minutes, within Boat's 30-day maximum. It archives
a sandbox rather than deleting it permanently. See Boat's
[deletion contract](https://docs.boat.dev/data-retention/delete).

## Next steps

- [Remote Execution](/guide/infrastructure/remote-execution): split dependency
  installation and tests into separate cacheable actions.
- [Infrastructure](/guide/infrastructure#available-providers): pick a cache
  provider so other machines reuse remote results.
- [Execution provider contract](/reference/infrastructure/execution): see what
  every execution provider guarantees.
