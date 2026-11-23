# Cloudflare Sandbox Containers

Struo runs commands in [Cloudflare Sandbox Containers](https://developers.cloudflare.com/sandbox/)
through an authenticated Worker bridge that you operate in your account.

**Capabilities:** remote execution through a self-deployed bridge. Configure
shared caching separately, including when using Cloudflare R2 object storage.

## Prerequisites

You need a Workers Paid account with Containers access and a deployed Struo
bridge. Installing Struo or selecting this provider does not deploy that
infrastructure. An administrator must approve the deployment and its costs.

### Deploy the bridge

Use Struo's Cloudflare sandbox bridge project. Install its
locked dependencies with `npm ci`, run `npm test` and `npm run check`, then
review the Wrangler configuration. The check only bundles the Worker with a
dry-run. Choose a Worker name for your account before deployment.

Set the Worker secret `BRIDGE_TOKEN` with `npx wrangler secret put BRIDGE_TOKEN`.
Use a random secret of at least 32 bytes. After approving the paid
infrastructure, deploy with `npx wrangler deploy`. This creates the Worker and
its SQLite-backed Durable Object binding with the `durable_object` container
scheduling policy. Keep the token outside manifests and action environments.

The default managed image includes Node.js 24. Custom images must provide
`sleep`, `mkdir`, `tar`, `tee`, and `cat`. Register custom images in Wrangler's
container `images` map and select their map key in Struo. Only the managed
`cloudflare/debian-trixie` image and those registered image keys are accepted.
For reproducible caching, use a versioned image key bound to an immutable
digest and change the key when changing the image. The managed image reference
does not promise immutable contents.

## Configuration

```toml
[infrastructures.remote_tests]
kind = "cloudflare"
image = "cloudflare/debian-trixie"

[infrastructure.execution]
provider = "remote_tests"
```

Set `STRUO_CLOUDFLARE_BRIDGE_URL` to the deployed Worker's HTTPS origin and
`STRUO_CLOUDFLARE_BRIDGE_TOKEN` to the same value as `BRIDGE_TOKEN`. Do not use
a Cloudflare account API token as the bridge credential. The CLI authenticates
each bridge call and refuses redirects. HTTP is accepted only on loopback for
local development.

## Usage

Run `struo exec --remote -- node --version`, or select the named provider with
`struo exec --remote --compute remote_tests -- node --version`. Annotated
scripts can select `# struo remote "remote_tests"`.

Declare dependency directories such as `node_modules` as inputs to the action
that consumes them. Struo transfers those directories with their executable
modes and portable symlinks. Dependencies containing native binaries must match
the guest's Linux architecture and ABI. See
[Remote Execution](/guide/infrastructure/remote-execution) for dependency and
test actions.

## Limits and cleanup

Struo transfers only declared inputs and restores declared outputs after an
accepted exit. It preserves separate binary stdout and stderr and validates
output paths before installing files in your workspace.

Struo spends one action deadline across provisioning, input staging, execution
and output retrieval. The bridge expires the container after one hour, even if
the CLI disappears. Explicit timeouts must be shorter than one hour; actions
without one use a one-hour client deadline. Cleanup uses separate bounded
requests and retains the resource lease until the cleanup attempt finishes.
Failed or uncertain cleanup remains in the workspace ownership journal for
later reconciliation. Durable Object alarms retry destruction, including after
a Worker restart. No creation or command request is retried.

The bridge streams transfers and writes captures to guest disk, with one shared
stdout/stderr byte limit. It destroys the whole container on capture overflow
or an internal transfer failure. Upload chunks avoid the single-request body
limit; platform disk, CPU, memory and plan limits still apply. This bridge
requests 10,000 MB of guest disk and rounds requested memory up to at least
256 MiB. Cloudflare may reject unavailable instance sizes.

This provider permits outbound Internet access and does not implement network
denial or custom account/project execution scoping. Struo rejects those
requirements. Each deployment trusts one bridge token: use separate deployments
for separate tenants. The bridge token stays in the Worker and CLI, never in
the command environment.

### Deployment verification

The bundled regression tests exercise the bridge with container doubles and a
local process-backed dependency/output roundtrip, and the Rust adapter with
HTTP fixtures. Wrangler's dry-run validates bundling
and configuration. These checks do not establish live account availability,
container startup, scheduling, billing or alarm delivery. Validate a deployed
bridge with a declared dependency directory, distinct stdout/stderr, a nonzero
exit, output retrieval and cancellation before using it for production work.
