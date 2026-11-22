# OpenSandbox

[OpenSandbox](https://open-sandbox.ai/) is a self-hostable sandbox service.
Struo creates a fresh image-based sandbox for each uncached action, uploads
declared inputs, runs the command, retrieves declared outputs, and deletes
the sandbox.

## Deploy The Service

Follow OpenSandbox's [deployment guide](https://open-sandbox.ai/deployment/)
to run its lifecycle service with a supported Docker or Kubernetes backend.
Configure API-key authentication and make both the lifecycle API and the
sandbox endpoints it returns reachable from your Struo coordinator.

Use a Linux image with a POSIX shell, `tail`, `mkdir`, `tar`, and the tools
your actions require. The service injects execd into the sandbox. Select an
image compatible with the service's host architecture. Pin the image by
registry digest for repeatable builds; changing the configured image changes
the action's cache identity. Updating a mutable image tag does not invalidate
an existing cached result.

## Configure Execution

```toml
[infrastructures.self_hosted]
kind = "opensandbox"
image = "node:22.18.0-bookworm"

[infrastructure.execution]
provider = "self_hosted"
properties = { secure_access = false }
```

This example configures Struo for a Docker-backed service, which rejects secured-access
creation requests. Set `secure_access = true` for a Kubernetes deployment
configured for secured access. Omitting the property defaults to `true`;
Struo never retries a rejected secured request without that protection.
The value must be a boolean. When secured access is disabled, isolate guest
endpoints on a trusted network or protect them with an authenticated gateway.

Set the lifecycle API base URL, including its `/v1` prefix, and provide the
API key through your environment or secret manager:

```sh
export STRUO_OPENSANDBOX_URL=https://sandbox.example.com/v1
export OPEN_SANDBOX_API_KEY=...
```

`STRUO_OPENSANDBOX_API_KEY` takes precedence over `OPEN_SANDBOX_API_KEY`.
Struo sends that credential only to the lifecycle service. It resolves the
execd endpoint on port 44772 and forwards the returned access headers,
including `X-EXECD-ACCESS-TOKEN`, to guest requests. Do not substitute a
lifecycle API key for a guest access token. Use HTTPS outside trusted local
networks because these headers authorize sandbox access.

Run with the workspace default or select the named infrastructure:

```sh
struo exec --remote --compute self_hosted -- node --version
```

Annotated scripts can select it with `# struo remote "self_hosted"`.
Runtime execution bindings use the same `opensandbox` executor and explicit
image environment, with `secure_access` in the binding's `properties` map.
OpenSandbox provides execution only; configure shared caching separately.

## Inputs And Results

Declare dependency outputs such as `node_modules` as inputs to downstream
actions. Struo includes them even when Git ignores them, preserves executable
modes and safe symlinks, and excludes undeclared files. Uploads use binary
multipart transfers; output and capture downloads use complete binary
responses rather than text or line-range APIs.

Struo starts commands in the background and polls their exit status. A
startup acknowledgement does not count as command success. It captures
stdout and stderr separately and retrieves declared outputs only after an
accepted exit code. Transfers and captures use Struo's bounded staging and
I/O limits. Output archives must pass path and symlink validation before
Struo installs them in the workspace.

## Lifecycle And Limits

Struo applies the action's CPU and memory request to sandbox resource limits,
using 250 MiB when the action declares no memory estimate. Startup waits
through `Pending` and `Allocated` until the sandbox reaches `Running`.
The coordinator uses remote admission capacity and keeps its permit through
cleanup. An explicit action timeout covers reconciliation, provisioning,
transfers, and command execution. Without one, Struo uses a 15-minute action
budget. Cleanup has separate bounded requests and retries transient deletion
failures.

Each sandbox receives an automatic expiration as a server-side fallback.
Struo records ownership before creation and attaches an operation marker.
After cancellation, transfer failure, or command completion, it deletes the
owned sandbox. If the process exits or a response is lost, a later invocation
reconciles retained records against authenticated ownership metadata. It
does not delete sandboxes with a different marker or accept a truncated
listing as proof of absence. Keep the workspace's runtime state until
unresolved ownership records have been reconciled.

The server's maximum sandbox timeout must allow the requested action timeout
plus five minutes for setup and transfer overhead. Actions without an
explicit timeout request a 15-minute sandbox lifetime. The adapter rejects
unsupported account/project scoping, network denial, output redirection, and
explicit filesystem setup rather than ignoring those requirements.

See the official [API contracts](https://open-sandbox.ai/api/) for deployment
compatibility. Local protocol tests do not establish compatibility with a
particular OpenSandbox deployment; verify your chosen backend, image, endpoint
routing, and secured access before relying on it for production builds.
