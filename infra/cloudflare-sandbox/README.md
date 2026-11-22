# Struo Cloudflare sandbox bridge

This Worker exposes Struo's authenticated `/v1` bridge over Cloudflare's
Durable Object Container API. It requires a Workers Paid account and Containers
availability. It is a deployment asset, not an automatic deployment hook.

Run `npm ci`, `npm test` and `npm run check` here. The check runs Wrangler's
deployment dry-run without provisioning infrastructure. Review the Worker name
in `wrangler.jsonc` before deploying to an account.

After obtaining deployment approval, set a random secret of at least 32 bytes
with `npx wrangler secret put BRIDGE_TOKEN` and deploy with
`npx wrangler deploy`. Set `STRUO_CLOUDFLARE_BRIDGE_URL` and
`STRUO_CLOUDFLARE_BRIDGE_TOKEN` in the CLI environment. The bridge token is not
a Cloudflare account API token. Keep one deployment per trust boundary.

The [Cloudflare provider guide](https://struo.run/docs/guide/infrastructure/cloudflare)
documents the protocol, image requirements, limits and lifecycle. The
[Cloudflare API reference](https://developers.cloudflare.com/containers/api/durable-object-container/)
defines the native API used by the Durable Object. This implementation uses the
current native container API rather than Sandbox SDK 0.x.

Local tests use fake Durable Object storage, container doubles, and a
process-backed archive/command roundtrip. The dry-run bundles real Worker code
but does not start a Cloudflare container.
No live-provider compatibility claim follows from those checks.
