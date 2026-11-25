# Daytona Sandboxes

[Daytona](https://www.daytona.io/docs/en/sandboxes/) provides hosted sandboxes.
Struo runs each uncached action in a Daytona sandbox built from a container
image, then deletes it.

Struo transfers declared inputs, runs the command, retrieves declared outputs,
and deletes the sandbox.

## Capabilities

| Capability | Support |
| --- | --- |
| Shared cache | No; use a [cache provider](/guide/infrastructure/#available-providers) |
| Remote execution | Hosted sandboxes from a container image |
| Artifact publication | No |
| Project setup | Manual [configuration](#configuration) |
| Authentication | `DAYTONA_API_KEY` |

## Prerequisites

- A Daytona account and API key with sandbox access.
- A Linux container image with the stable tools your actions need, including
  `tar`. Keep repository dependencies such as `node_modules` in the action
  graph so a lockfile change invalidates the right action.

## Configuration

Bind the image in the repository root `struo.toml`. A Vitest workflow can use a
pinned Node.js image:

```toml
[infrastructures.remote_tests]
kind = "daytona"
image = "node:22.18.0-bookworm"

[infrastructure.execution]
provider = "remote_tests"
```

Struo asks Daytona to build the sandbox environment from that image.

### Authentication

Set the provider access key outside the script:

```sh
export DAYTONA_API_KEY=...
```

`STRUO_DAYTONA_API_KEY` takes precedence when both variables are present. Struo
does not forward the key to the action process.

Self-hosted or test endpoints can override the control and toolbox addresses
with `STRUO_DAYTONA_CONTROL_URL` and `STRUO_DAYTONA_TOOLBOX_URL`.

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
Native dependencies must match the guest's Linux architecture and runtime. To
produce `node_modules` in a separate cacheable action, see
[Remote Execution](/guide/infrastructure/remote-execution).

## Limits and cleanup

- Daytona returns standard output and standard error as one combined result.
  Struo exposes it as standard output.
- The sandbox stops automatically after an interval derived from the action
  timeout, with extra time for provisioning and transfers. Actions without an
  explicit timeout use Daytona's short-lived execution policy.

Struo sends an explicit delete request after success, command failure, timeout,
or transfer failure. It also marks each sandbox as ephemeral with immediate
deletion after stop, so Daytona removes it if the client disappears first. See
Daytona's [sandbox lifecycle guide](https://www.daytona.io/docs/en/sandboxes/#ephemeral-sandboxes).

## Next steps

- [Remote Execution](/guide/infrastructure/remote-execution): split dependency
  installation and tests into separate cacheable actions.
- [Infrastructure](/guide/infrastructure#available-providers): pick a cache
  provider so other machines reuse remote results.
- [Execution provider contract](/reference/infrastructure/execution): see what
  every execution provider guarantees.
