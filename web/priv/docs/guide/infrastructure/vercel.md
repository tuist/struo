# Vercel Sandbox

[Vercel Sandbox](https://vercel.com/docs/sandbox) runs each uncached action in
a fresh Linux sandbox. Struo uploads declared inputs, including local
`node_modules`, runs the command, restores declared outputs after an accepted
exit code, and deletes the sandbox.

## Configure A Runtime

```toml
[infrastructures.remote_tests]
kind = "vercel"
runtime = "node24"

[infrastructure.execution]
provider = "remote_tests"
account = "team_your_team_id"
project = "prj_your_project_id"
```

`account` selects the Vercel team and `project` selects its project. Without
explicit scope, set `VERCEL_TEAM_ID` and `VERCEL_PROJECT_ID`. The CLI records
these non-secret defaults in the execution binding before computing cache
identity. Explicit bindings take precedence. Runtime API callers must supply
nonempty `account` and `project` fields; unresolved scope is rejected before
cache lookup. Set `VERCEL_TOKEN`
to a Vercel access token with Sandbox access for that team/project. Keep the
token outside scripts and manifests. Struo does not forward it to commands.
This adapter uses token authentication, not automatic OIDC credential discovery.

Runtime values follow Vercel's deprecated but still published runtime-based
create API, currently `node22`,
`node24`, `node26`, and `python3.13`. Built-in `--compute vercel` uses `node24`.
These runtime names are provider-managed references, not immutable image pins.
The guest must provide `sh`, `env`, `mkdir`, and `tar` supporting
`--hard-dereference`. Local native dependencies must match the guest's Linux
architecture and runtime. Do not upload macOS-built native modules.

## Run Annotated Tests

For a script at `scripts/test.sh`:

```sh
#!/bin/sh
# struo remote "remote_tests"
# struo input "../node_modules"
# struo input "../package.json"
# struo input "../tests"
# struo output "../reports"
# struo cwd ".."
node_modules/.bin/vitest run --reporter=junit --outputFile=reports/junit.xml
```

Run it with `struo exec -- scripts/test.sh`. Explicitly declared ignored
directories such as `node_modules` are included. Dependency scripts can also
produce this directory as an output consumed by the test script. See
[Remote Execution](/guide/infrastructure/remote-execution) for dependency chaining.

Literal commands use `struo exec --remote --compute remote_tests -- node --version`.
Select the same account/project in the workspace execution binding when using
the default provider, or supply the scope environment variables when selecting
a provider by name.

## Transfers And Limits

Struo streams a gzip-compressed input archive through Vercel's binary write
endpoint and reads output files as binary streams. Safe symlinks and executable
modes are preserved. Output archives pass through the shared safe extractor;
hard links are copied as independent files without dereferencing symlinks.

Standard output and standard error are captured separately into guest files
and downloaded under one combined capture allowance. Output is replayed after
completion, not streamed live. The command uses literal positional arguments
and the selected environment plus the guest runtime's `PATH`. An explicitly
selected `PATH` overrides that default. Struo's existing host memory, remote
admission, capture, archive and transfer-stall budgets apply.

The action deadline covers preparation, ownership recovery, provisioning,
transfer, execution and output installation. It defaults to ten minutes.
The session lifetime adds two minutes of cleanup grace and must fit Vercel's
24-hour maximum and the account's plan limits. Hobby sessions are limited to
45 minutes. Guest CPUs round up to one or
an even number, with 2048 MB per CPU, satisfying Vercel's allocation contract.
Network denial, output-idle timeout, explicit filesystem setup and output
redirection are not supported by this adapter and fail closed.

## Disposal And Recovery

Vercel sandboxes persist by default. Struo explicitly sets `persistent = false`
and never creates snapshots or mounts persistent drives. It deletes the named
sandbox, requests orphan-snapshot deletion, and confirms that the name is gone.
Stopping a session alone is not cleanup because the named configuration remains.

Creation intent is durably recorded before provisioning. A unique name and
atomic ownership tag allow recovery after a lost response or process exit.
Cleanup verifies the exact name and tag through an authenticated lookup with
`resume=false` before every deletion attempt, including retries and recovery.
Vercel does not offer a conditional-delete ownership precondition, so a name
replaced between that lookup and deletion remains an API-level race.
Inconclusive ownership and failed
cleanup remain journaled. Cancellation retains the admission lease until the
bounded cleanup attempt finishes. Session timeout limits active compute after
a client crash, but named metadata still needs a later reconciliation pass.

The adapter follows the official SDK's runtime-based `/v2` create path and
session APIs. The newer image-based `/v3` and `/v4` create APIs are separate
contracts and are not used here. See the [REST API](https://vercel.com/docs/rest-api#sandboxes)
and [pricing and quotas](https://vercel.com/docs/sandbox/pricing) for current
account availability, limits and billing.
