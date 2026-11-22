# Boat

[Boat](https://docs.boat.dev/api/v1) provides hosted Linux sandboxes. Struo
creates a disposable sandbox for an uncached action, uploads declared inputs,
runs the command, retrieves outputs after an accepted exit, and requests
permanent deletion.

## Configure A Snapshot

Prepare a saved Boat snapshot containing your tools, then bind its name:

```toml
[infrastructures.remote_tests]
kind = "boat"
snapshot = "node-tests-v1"

[infrastructure.execution]
provider = "remote_tests"
```

Boat's `from` field selects this saved snapshot, not an OCI image. Keep the
snapshot name stable only while its contents remain unchanged; replacing it
under the same name does not change Struo's action identity. The guest needs
`sh`, `env`, `rm`, `mkdir`, and `tar`, plus the runtime your script uses. Struo uses
`/home/user/struo-workspace` as its execution root.

You can select Boat's current default Linux environment without a saved
snapshot with `--compute boat`. That default is not an immutable toolchain.
Struo explicitly requests private visibility, sets `noEnv = true`, disables
snapshots and monitoring, and does not attach account repositories, secrets,
or credentials to the sandbox. It does not inherit an organization's shared
sandbox visibility default. A saved snapshot can still contain files you placed
in it; prepare it without secrets.

## Authenticate And Run

Set `BOAT_API_KEY` in your environment. `STRUO_BOAT_API_KEY` takes precedence.
Use an account key with sandbox creation, listing, command execution, file
read/write, and permanent-deletion access. Struo does not forward the key to
your command. `STRUO_BOAT_API_URL` overrides the default
`https://boat.dev/api/v1` endpoint for compatible test services.

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

## Execution And Limits

Boat documents UTF-8 newline-delimited command streams and a synchronous
command limit of 600 seconds. Struo instead starts detached commands, polls
their status, and downloads separate file-backed stdout and stderr as binary
data. This preserves non-UTF-8 output and supports longer declared timeouts.
Terminal output appears after command completion rather than live.
Selected environment entries are applied to the final executable after shell
setup and capture redirection, preserving values such as `IFS` and `PWD`.

Struo streams archives through Boat's short-lived binary PUT/GET URLs without
sending the account bearer token to those URLs. It applies shared remote
admission, bounded metadata and capture limits, transfer-stall bounds, and safe
output validation. Boat inherits the saved snapshot's machine size when one is
selected, otherwise it uses its default size. Struo's resource declarations
budget coordinator work and do not resize Boat machines.
Network denial, explicit account/project scoping, path redirection, graph
filesystem setup, and output-idle timeouts are not supported by this adapter
and fail closed.

## Disposable Ownership

Struo journals intent before creation, uses the operation nonce as both a
label and an `Idempotency-Key`, and never retries an uncertain create. Boat
keeps idempotency keys for 24 hours and requires the same account, key, and body
for a retry. A later run discovers owned sandboxes by their exact returned
label and retries cleanup. Foreign or truncated listings cannot prove absence.
Boat's active wallet can change without changing the API key, so even an empty
listing or a DELETE returning 404 does not release ownership. Struo retains
those records until cleanup receives a matching permanent-deletion receipt.
This can retain records for sandboxes deleted outside Struo; enough unresolved
records will block new provisioning rather than silently discard ownership.

Struo sends `DELETE /sandboxes/{id}` with `X-Ascii-Confirm-Delete: {id}` after
success, failure, timeout, or cancellation. Cleanup retains admission until
the bounded deletion attempt settles and keeps failed ownership records for
recovery. Boat's accepted deletion ends billing and removes the sandbox from
use at once. Boat then erases stored data in the background. Struo logs the
deletion operation ID and releases ownership after acceptance; it does not
wait for or claim completed data erasure. Check the operation with Boat's
`GET /deletion-operations/{id}` if you need purge completion.

TTL is a fallback: one hour without an action timeout, otherwise the timeout
rounded up plus five minutes, within Boat's 30-day maximum. TTL archives a
sandbox rather than requesting permanent deletion. Disabling snapshots avoids
keeping its disk after a stop, but explicit DELETE remains the cleanup path.
See Boat's [deletion contract](https://docs.boat.dev/data-retention/delete).
