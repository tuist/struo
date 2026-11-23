# Northflank

[Northflank Sandboxes](https://northflank.com/docs/v1/application/sandboxes/quickstart)
runs each uncached action in a fresh deployment service. Struo uploads the
declared inputs, captures stdout and stderr, retrieves declared outputs after
an accepted exit code, and deletes the service.

**Capabilities:** remote execution. Configure shared caching separately.

## Prerequisites

Create a dedicated Northflank project and an API token with permission to
create, read, execute commands in, and delete services in that project. Set
`NORTHFLANK_TOKEN` outside the script, or use `STRUO_NORTHFLANK_TOKEN` to take
precedence. Struo does not forward this transport credential to the test process.

## Configuration

Configure the image, deployment plan, and project in the workspace `struo.toml`:

```toml
[infrastructures.remote_tests]
kind = "northflank"
image = "node:22.18.0-bookworm"
deployment_plan = "nf-compute-200"

[infrastructure.execution]
provider = "remote_tests"
project = "your-northflank-project-id"
# account = "your-team-id" # Only for a team-scoped API token.
```

`account` selects Northflank's team scope. Omit it for personal-account tokens.
Struo scopes service operations and ownership recovery to the selected API
origin, team, and project. It does not create or delete the project.

Choose an immutable image digest for reproducible runs. The image must provide
`sleep infinity`, `env`, `sh`, and `tar`, and its default user must be able to
create and write `/workspace`. Struo checks the plan's advertised CPU and
memory against the action's request before creating a service. Plans unavailable
through the plans API fail closed rather than silently ignoring resource requests.

Northflank Cloud supplies sandbox isolation. For BYOC, first configure a
microVM or gVisor runtime as the cluster default. Installing sandbox runtimes
without selecting one does not isolate workloads. Struo does not set a
per-service runtime class, attach persistent volumes, or expose public ports.
See [BYOC sandbox configuration](https://northflank.com/docs/v1/application/sandboxes/sandboxes-in-your-own-cloud).
The adapter does not enforce denied network access; requests for that policy fail.

## Usage

Select the configured default:

```sh
struo exec --remote -- sh scripts/test.sh
```

Or select the provider with `--compute remote_tests`. A test script can also
select it through its headers:

```sh
#!/bin/sh
# struo remote "remote_tests"
# struo input "../package.json"
# struo input "../node_modules"
# struo input "../tests"
# struo output "../reports"
# struo cwd ".."

./node_modules/.bin/vitest run --reporter=json --outputFile=reports/results.json
```

Declare dependency installation as a separate action when necessary. See
[Remote Execution](/guide/infrastructure/remote-execution) for dependency
ordering. Struo includes declared `node_modules` trees, executable modes, and
safe symlinks in its input archive; it does not apply Git ignore rules.

## Limits and cleanup

You do not need Node.js or the Northflank CLI on the coordinator. Struo
preserves separate binary stdout and stderr, with terminal output shown after
capture. Transfers and captures respect Struo's resource limits.

A single action deadline includes provisioning, transfers, command execution, and output
installation. Without a declared timeout, the adapter uses ten minutes. The
configured timeout plus five minutes must fit Northflank's seven-day expiry limit.

After success, failure, timeout, or cancellation, Struo deletes the service
and confirms its absence before making execution capacity available again.
Failed deletions remain in the workspace lifecycle journal.
On later runs, Struo recovers services by their operation-specific names and
verifies the matching ownership description before deleting them. It retains
uncertain create outcomes rather than retrying a potentially successful create.

Struo also sets service expiry to the action deadline plus five
minutes. Northflank pauses expired services and schedules them for later
deletion. Expiry is a fallback, not confirmation of immediate deletion. Keep
the lifecycle journal and the original project access available for recovery.
See [the deployment service API](https://northflank.com/docs/v1/api/project/services/create-deployment-service)
and [file transfer documentation](https://northflank.com/docs/v1/api/copy-files).

`STRUO_NORTHFLANK_API_URL` overrides the API origin for compatible endpoints.
The same origin serves REST and exec WebSocket traffic. Credentials belong in
the environment, not in the URL or manifest.
