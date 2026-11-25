# Microsandbox

[Microsandbox](https://github.com/microsandbox/microsandbox) runs commands in
local micro-VMs. Struo uses it to run actions in isolation on your own machine
with the same contract as hosted providers.

It is useful for validating declared inputs and outputs locally before moving
to a hosted executor such as E2B or Daytona.

## Capabilities

| Capability | Support |
| --- | --- |
| Shared cache | No; use a [cache provider](/guide/infrastructure/#available-providers) |
| Remote execution | Local micro-VMs |
| Artifact publication | No |
| Project setup | Manual [configuration](#configuration) |
| Authentication | None |

## Prerequisites

- Linux or Apple Silicon macOS with the virtualization support required by
  [Microsandbox](https://github.com/microsandbox/microsandbox). On Linux this
  needs KVM.
- An image containing the runtime and tools your actions need.

## Configuration

Bind the image in the repository root `struo.toml`. Name the infrastructure
after its role, not the test framework:

```toml
[infrastructures.remote_tests]
kind = "microsandbox"
image = "node:22.18.0-alpine"

[infrastructure.execution]
provider = "remote_tests"
```

The image supplies tools such as [Node.js](https://nodejs.org/). Pin it to an
immutable digest for reproducible shared workflows. Microsandbox runs locally
and needs no credentials.

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

The command uses the Node.js executable from the configured image; Struo does
not copy host interpreter paths or the host tool environment. Declare any
additional files your tests read. Struo rejects symbolic links that escape the
declared input trees.

Copying the host's `node_modules` can bring in platform-specific native files
and links into a package manager store. Prefer producing it in a separate
cacheable install action, as shown in
[Remote Execution](/guide/infrastructure/remote-execution).

## Limits and cleanup

- Each action starts in a clean workspace that contains only declared inputs.
- Struo retrieves only declared outputs after a successful command and installs
  them through a staging directory.
- Microsandbox is available through Struo on Linux and Apple Silicon macOS only.

Struo stops the micro-VM and removes its persisted state after success,
failure, or timeout.

## Next steps

- [Remote Execution](/guide/infrastructure/remote-execution): split dependency
  installation and tests into separate cacheable actions.
- [E2B Sandboxes](/guide/infrastructure/e2b): run the same actions on a hosted
  provider.
- [Execution provider contract](/reference/infrastructure/execution): see what
  every execution provider guarantees.
