# Cloudflare Sandbox Containers

Run Struo actions remotely in [Cloudflare Sandbox Containers](https://developers.cloudflare.com/sandbox/)
through an authenticated Worker bridge that you deploy in your own account.

**Capabilities:** remote execution through a self-deployed bridge. Configure
shared caching separately, including when using Cloudflare R2 object storage.
Regression tests cover the bridge and adapter; live account behavior is
unverified.

## Prerequisites

You need a Workers Paid account with Containers access and a deployed Struo
bridge. Installing Struo or selecting this provider does not deploy that
infrastructure. An administrator must approve the deployment and its costs.

### Deploy the bridge

Use Struo's Cloudflare sandbox bridge project. Install its locked dependencies
with `npm ci`, run `npm test` and `npm run check`, then review the Wrangler
configuration. The check only bundles the Worker with a dry-run. Choose a Worker
name for your account before deployment.

Set the Worker secret `BRIDGE_TOKEN` with `npx wrangler secret put BRIDGE_TOKEN`.
Use a random secret of at least 32 bytes. After approving the paid
infrastructure, deploy with `npx wrangler deploy`. This creates the Worker and
its SQLite-backed Durable Object binding with the `durable_object` container
scheduling policy. Keep the token outside manifests and action environments.

### Choose an image

The default managed image includes Node.js 24. Custom images must provide
`sleep`, `mkdir`, `tar`, `tee`, and `cat`. Register custom images in Wrangler's
container `images` map and select their map key in Struo. Only the managed
`cloudflare/debian-trixie` image and those registered image keys are accepted.
For reproducible caching, use a versioned image key bound to an immutable
digest and change the key when changing the image. The managed image reference
does not promise immutable contents.

## Configuration

Bind the image in the repository root `struo.toml`:

```toml
[infrastructures.remote_tests]
kind = "cloudflare"
image = "cloudflare/debian-trixie"

[infrastructure.execution]
provider = "remote_tests"
```

### Authentication

Set `STRUO_CLOUDFLARE_BRIDGE_URL` to the deployed Worker's HTTPS origin and
`STRUO_CLOUDFLARE_BRIDGE_TOKEN` to the same value as `BRIDGE_TOKEN`. Do not use
a Cloudflare account API token as the bridge credential. Struo authenticates
each bridge call and refuses redirects. HTTP is accepted only on loopback for
local development. The bridge token never reaches the command environment.

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
Struo transfers declared directories such as `node_modules` with their
executable modes and portable symbolic links. Native dependencies must match
the guest's Linux architecture and ABI. To produce `node_modules` in a separate
cacheable action, see [Remote Execution](/guide/infrastructure/remote-execution).

## Verify the deployment

The bundled tests and Wrangler's dry-run do not establish live account
availability, container startup, scheduling, billing or alarm delivery. Before
production use, run an action against the deployed bridge that covers a
declared dependency directory, distinct stdout and stderr, a nonzero exit,
output retrieval and cancellation.

## Limits and cleanup

- Stdout and stderr are preserved separately as binary, with one shared byte
  limit. Exceeding it destroys the container.
- One action deadline covers provisioning, input staging, execution and output
  retrieval. Explicit timeouts must be shorter than one hour; actions without
  one use a one-hour deadline.
- The bridge requests 10,000 MB of guest disk and at least 256 MiB of memory.
  Cloudflare platform and plan limits still apply and may reject unavailable
  instance sizes.
- The container has outbound Internet access. Network denial and
  account/project execution scoping are rejected.
- Each deployment trusts one bridge token. Use separate deployments for
  separate tenants.

The bridge expires each container after one hour, even if the CLI disappears,
and Durable Object alarms retry destruction after a Worker restart. Failed or
uncertain cleanup stays recorded in the workspace's `.struo` runtime state for
a later run. Struo never retries a creation or command request.

## Next steps

- [Remote Execution](/guide/infrastructure/remote-execution): split dependency
  installation and tests into separate cacheable actions.
- [Object Storage Cache](/guide/infrastructure/object-storage): share results
  through a Cloudflare R2 bucket.
- [Execution provider contract](/reference/infrastructure/execution): see what
  every execution provider guarantees.
