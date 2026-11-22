---
title: Browser caching, cross-tab execution and OAuth2
date: 2026-11-22
---

The JavaScript SDK now supports browsers through `@tuist/struo/browser`.
Initialize its Wasm module, then store blobs and action results in IndexedDB.
The browser SDK shares Rust content-digest and action-key logic with native
clients, so the same inputs produce the same keys.

Same-origin tabs can share a named cache, and page reloads preserve its data
unless the browser evicts it. You can inspect cache statistics, forget action
results without deleting blobs, or clear a demonstration's database.

Node.js integrations keep their existing native API. The browser API returns
`Uint8Array` and omits filesystem methods and native execution providers.
Read the [JavaScript SDK guide](/docs/guide/sdk/javascript#browser-cache)
for initialization, storage limits, and examples.

The separate `@tuist/struo/browser/execution` entry point lets applications
dispatch named operations to same-origin tabs with explicit memory budgets.
Workers exchange cache digests and share payloads through IndexedDB.

Applications can also authenticate with a public OAuth2 client using S256 PKCE
and dispatch GitLab pipelines. The adapter supports polling, cancellation and
bounded artifact downloads into the browser cache. It requires a registered
OAuth application and a project with a configured CI pipeline; it does not
translate native Struo actions or provision runners.
