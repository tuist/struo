# Cloudflare Sandbox Containers

Struo runs commands in [Cloudflare Sandbox Containers](https://developers.cloudflare.com/sandbox/)
through an authenticated Worker that you operate. Cloudflare exposes container
control to a Durable Object inside that Worker. The Rust CLI calls the Struo
bridge protocol, not a Cloudflare sandbox REST API.

You need a Workers Paid account with Containers access and a deployed Struo
bridge. Installing Struo or selecting this provider does not deploy that
infrastructure. An administrator must approve the deployment and its costs.

## Deploy The Bridge

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

## Configure Struo

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

Run `struo exec --remote -- node --version`, or select the named provider with
`struo exec --remote --compute remote_tests -- node --version`. Annotated
scripts can select `# struo remote "remote_tests"`.

Declare dependency directories such as `node_modules` as inputs to the action
that consumes them. Struo transfers those directories with their executable
modes and portable symlinks. Dependencies containing native binaries must match
the guest's Linux architecture and ABI. See
[Remote Execution](/guide/infrastructure/remote-execution) for dependency and
test actions.

## Transfer And Execution Contract

The bridge exposes an authenticated `/v1` protocol:

| Request | Contract |
| --- | --- |
| `POST /sandboxes` | Provision one container for a unique ownership marker, selected image, CPU and memory request, and transfer/capture limits. Return that marker as `id`. |
| `GET /sandboxes?marker=…` | Return owned records with their exact markers, including uncertain creates and failed deletions. |
| `PUT /sandboxes/{id}/inputs/{offset}` | Append an ordered input tar chunk of at most 8 MiB. Reject replayed offsets. |
| `POST /sandboxes/{id}/stage` | Extract the uploaded declared inputs into the fresh execution root. |
| `POST /sandboxes/{id}/exec` | Execute literal `argv`, explicit environment and working directory once. Return a required integer `exitCode`. |
| `GET /sandboxes/{id}/stdout` or `/stderr` | Stream separate binary captures from guest disk. |
| `POST /sandboxes/{id}/outputs` | Pack only the declared relative `paths`. |
| `GET /sandboxes/{id}/outputs` | Stream the output tar. Struo validates paths, archive limits and declared-output containment before installing it. |
| `DELETE /sandboxes/{id}` | Destroy the whole container, confirm destruction and retain a tombstone against delayed creates. |

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

## Verification Boundary

The bundled regression tests exercise the bridge with container doubles and a
local process-backed dependency/output roundtrip, and the Rust adapter with
HTTP fixtures. Wrangler's dry-run validates bundling
and configuration. These checks do not establish live account availability,
container startup, scheduling, billing or alarm delivery. Validate a deployed
bridge with a declared dependency directory, distinct stdout/stderr, a nonzero
exit, output retrieval and cancellation before using it for production work.
