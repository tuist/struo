# Boat

[Boat](https://docs.boat.dev/api/v1) provides hosted Linux sandboxes. Struo
creates a disposable sandbox for an uncached action, uploads declared inputs,
runs the command, retrieves outputs after an accepted exit, and requests
permanent deletion.

**Capabilities:** remote execution. Configure shared caching separately.

## Prerequisites

Use a Boat account with sandbox creation, listing, command execution, file
read/write, and permanent-deletion access. Prepare a saved Linux snapshot with
your runtime, `sh`, `env`, `rm`, `mkdir`, and `tar`.

Set `BOAT_API_KEY` outside scripts and manifests. `STRUO_BOAT_API_KEY` takes
precedence. Struo does not forward this key to your command.

## Configuration

Prepare a saved Boat snapshot containing your tools, then bind its name:

```toml
[infrastructures.remote_tests]
kind = "boat"
snapshot = "node-tests-v1"

[infrastructure.execution]
provider = "remote_tests"
```

Select a saved snapshot, not an OCI image. Keep the
snapshot name stable only while its contents remain unchanged; replacing it
under the same name does not change Struo's action identity. The guest needs
`sh`, `env`, `rm`, `mkdir`, and `tar`, plus the runtime your script uses.

You can select Boat's current default Linux environment without a saved
snapshot with `--compute boat`. That default is not an immutable toolchain.
Struo requests private visibility, disables inherited account environment,
snapshots and monitoring, and does not attach account repositories, secrets,
or credentials to the sandbox. It does not inherit an organization's shared
sandbox visibility default. A saved snapshot can still contain files you placed
in it; prepare it without secrets.

`STRUO_BOAT_API_URL` overrides the default
`https://boat.dev/api/v1` endpoint for compatible test services.

## Usage

```sh
struo exec --remote --compute remote_tests -- node --version
```

An annotated test script can upload an existing local dependency tree:

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

Save this as `scripts/test.sh` and run `struo exec --script -- sh scripts/test.sh`.
Declare any additional source, configuration, or setup files your tests read.
Struo includes declared `node_modules` even when Git ignores it, preserving
executable modes and safe symbolic links. Native dependencies must match the
guest's Linux architecture and runtime. A dependency-install action can also
produce `node_modules`; see [Remote Execution](/guide/infrastructure/remote-execution).

## Limits and cleanup

Struo preserves binary stdout and stderr. Terminal output appears after command
completion rather than live. Transfers and captures respect Struo's resource
limits, and Struo validates outputs before installing them.

Boat inherits the saved snapshot's machine size when one is
selected, otherwise it uses its default size. Struo's resource declarations
budget coordinator work and do not resize Boat machines.
Network denial, explicit account/project scoping, path redirection, graph
filesystem setup, and output-idle timeouts are not supported by this adapter
and fail closed.

### Cleanup and recovery

Struo records ownership before creation and does not retry an uncertain create.
A later run can discover owned sandboxes and retry cleanup.
Boat's active wallet can change without changing the API key, so even an empty
listing or a DELETE returning 404 does not release ownership. Struo retains
those records until cleanup receives a matching permanent-deletion receipt.
This can retain records for sandboxes deleted outside Struo; enough unresolved
records will block new provisioning rather than silently discard ownership.

Struo requests permanent deletion after
success, failure, timeout, or cancellation. Cleanup retains admission until
the bounded deletion attempt settles and keeps failed ownership records for
recovery. Boat's accepted deletion ends billing and removes the sandbox from
use at once. Boat then erases stored data in the background. Struo logs the
deletion operation ID and releases ownership after acceptance; it does not
wait for or claim completed data erasure. Check the deletion operation in Boat
if you need purge completion.

TTL is a fallback: one hour without an action timeout, otherwise the timeout
rounded up plus five minutes, within Boat's 30-day maximum. TTL archives a
sandbox rather than requesting permanent deletion. Disabling snapshots avoids
keeping its disk after a stop, but explicit DELETE remains the cleanup path.
See Boat's [deletion contract](https://docs.boat.dev/data-retention/delete).
