# Northflank Sandboxes

[Northflank Sandboxes](https://northflank.com/docs/v1/application/sandboxes/quickstart)
run containers in isolated services. Struo runs each uncached action in a fresh
Northflank service, then deletes it.

Struo uploads declared inputs, captures stdout and stderr, retrieves declared
outputs after an accepted exit code, and deletes the service.

**Capabilities:** remote execution. Configure shared caching separately.

## Prerequisites

- A dedicated Northflank project and an API token with permission to create,
  read, execute commands in, and delete services in that project.
- An image that provides `sleep infinity`, `env`, `sh`, and `tar`, whose
  default user can create and write `/workspace`. Use an immutable digest for
  reproducible runs.
- For BYOC, a microVM or gVisor runtime configured as the cluster default.
  Installing sandbox runtimes without selecting one does not isolate workloads.
  See [BYOC sandbox configuration](https://northflank.com/docs/v1/application/sandboxes/sandboxes-in-your-own-cloud).
  Northflank Cloud supplies sandbox isolation.

You do not need Node.js or the Northflank CLI on the coordinator.

## Configuration

Configure the image, deployment plan, and project in the repository root
`struo.toml`:

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
Struo does not create or delete the project. It checks the plan's CPU and
memory against the action's request before creating a service; plans
unavailable through the plans API fail rather than ignoring resource requests.

### Authentication

Set `NORTHFLANK_TOKEN` outside the script. `STRUO_NORTHFLANK_TOKEN` takes
precedence. Struo does not forward this credential to the command.

`STRUO_NORTHFLANK_API_URL` overrides the API origin for compatible endpoints.
Keep credentials in the environment, not in the URL or manifest.

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

- Stdout and stderr are preserved separately as binary and appear after the
  command completes.
- One action deadline covers provisioning, transfers, execution, and output
  installation. Without a declared timeout it is ten minutes. The timeout plus
  five minutes must fit Northflank's seven-day expiry limit.
- Struo does not set a per-service runtime class, attach persistent volumes, or
  expose public ports.
- Network denial is not enforced; actions that request it fail.

After success, failure, timeout, or cancellation, Struo deletes the service and
confirms it is gone. It also sets service expiry to the action deadline plus
five minutes; Northflank pauses expired services and deletes them later, so
expiry is only a fallback.

Failed deletions and uncertain creates stay recorded in the workspace's
`.struo` runtime state, and a later run deletes services it can prove it owns.
Keep that state and the original project access available for recovery. See
the [deployment service API](https://northflank.com/docs/v1/api/project/services/create-deployment-service).

## Next steps

- [Remote Execution](/guide/infrastructure/remote-execution): split dependency
  installation and tests into separate cacheable actions.
- [Infrastructure](/guide/infrastructure#available-providers): pick a cache
  provider so other machines reuse remote results.
- [Execution provider contract](/reference/infrastructure/execution): see what
  every execution provider guarantees.
