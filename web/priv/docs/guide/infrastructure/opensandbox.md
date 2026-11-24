# OpenSandbox

[OpenSandbox](https://open-sandbox.ai/) is a self-hostable sandbox service.
Struo runs each uncached action remotely in a fresh image-based OpenSandbox
sandbox, then deletes it.

Struo uploads declared inputs, runs the command, retrieves declared outputs,
and deletes the sandbox.

**Capabilities:** remote execution through a self-hosted service. Configure
shared caching separately. Local protocol tests do not establish compatibility
with a particular deployment.

## Prerequisites

- An OpenSandbox lifecycle service with a supported Docker or Kubernetes
  backend, deployed per its [deployment guide](https://open-sandbox.ai/deployment/)
  with API-key authentication. Both the lifecycle API and the sandbox endpoints
  it returns must be reachable from your Struo coordinator.
- A server maximum sandbox timeout that allows your action timeout plus five
  minutes.
- A Linux image with a POSIX shell, `tail`, `mkdir`, `tar`, and the tools your
  actions require, compatible with the service's host architecture. Pin it by
  registry digest: updating a mutable tag does not invalidate cached results.

## Configuration

Bind the image in the repository root `struo.toml`:

```toml
[infrastructures.remote_tests]
kind = "opensandbox"
image = "node:22.18.0-bookworm"

[infrastructure.execution]
provider = "remote_tests"
properties = { secure_access = false }
```

This example targets a Docker-backed service, which rejects secured-access
creation requests. Set `secure_access = true` for a Kubernetes deployment
configured for secured access. Omitting the property defaults to `true`; Struo
never retries a rejected secured request without that protection. When secured
access is disabled, isolate guest endpoints on a trusted network or protect
them with an authenticated gateway.

### Authentication

Set the lifecycle API base URL, including its `/v1` prefix, and provide the API
key through your environment or secret manager:

```sh
export STRUO_OPENSANDBOX_URL=https://sandbox.example.com/v1
export OPEN_SANDBOX_API_KEY=...
```

`STRUO_OPENSANDBOX_API_KEY` takes precedence over `OPEN_SANDBOX_API_KEY`.
Struo sends that credential only to the lifecycle service and uses separate
service-issued credentials for guest access. Use HTTPS outside trusted local
networks.

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

- Stdout and stderr are captured separately. Declared outputs are retrieved only
  after an accepted exit code and validated before installation.
- The action's CPU and memory request becomes the sandbox's resource limits,
  with 250 MiB when the action declares no memory estimate.
- An explicit action timeout covers provisioning, transfers, and execution.
  Without one, Struo uses a 15-minute budget and sandbox lifetime.
- Account/project scoping, network denial, output redirection, and explicit
  filesystem setup are rejected rather than ignored.

Struo deletes the sandbox after completion, cancellation, or transfer failure,
and retries transient deletion failures. Each sandbox also has an automatic
expiration as a server-side fallback. If the process exits or a response is
lost, a later run cleans up sandboxes Struo can prove it owns. Keep the
workspace's `.struo` runtime state until then.

Verify your backend, image, endpoint routing, and secured access against the
official [API contracts](https://open-sandbox.ai/api/) before production use.

## Next steps

- [Remote Execution](/guide/infrastructure/remote-execution): split dependency
  installation and tests into separate cacheable actions.
- [Infrastructure](/guide/infrastructure#available-providers): pick a cache
  provider so other machines reuse remote results.
- [Execution provider contract](/reference/infrastructure/execution): see what
  every execution provider guarantees.
