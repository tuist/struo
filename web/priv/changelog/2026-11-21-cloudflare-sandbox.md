---
title: Run scripts through a Cloudflare sandbox bridge
date: 2026-11-21
---

Struo now includes a Cloudflare Sandbox Containers adapter and an authenticated
Worker bridge. Configure `kind = "cloudflare"` to upload declared inputs,
including dependency directories, execute a command, capture separate stdout
and stderr with its exit code, and retrieve declared outputs.

You must deploy the Worker and its Durable Object binding in a Workers Paid
account before using the provider. Struo does not deploy infrastructure or call
a direct Cloudflare sandbox REST API. The bridge uses durable expiry alarms,
while the CLI journals ownership and attempts cleanup after execution or
cancellation. Local protocol tests and a Wrangler dry-run cover the integration;
live Cloudflare validation requires an approved deployment.

Read the [Cloudflare Sandbox guide](/docs/guide/infrastructure/cloudflare) for
setup, the bridge contract and limits.
