---
title: Lazy shared caching on Sprites
date: 2026-11-24
---

A Sprites infrastructure provider can now supply both remote execution and a
retained cache. Bind it to `[infrastructure.cache]` to create a separate cache
Sprite on the first remote cache operation and reuse it across runs. Existing
execution-only configurations stay unchanged.

The retained service restarts on cold wake, uses a cache-only bearer
credential, streams uploads atomically, and evicts old objects within a
configurable disk budget. Cache Sprites remain separate from disposable action
cleanup. The Rust SDK's workspace constructor uses the same configuration.

Reuse is scoped to a user's local credentials directory and Sprites API
credential. Direct execution-sandbox-to-cache transfers are not implemented.
Live cache validation covers inter-Sprite access, 8 MiB transfers, cache reuse,
service restart, and retrieval after idle. Cold VM wake and larger uploads
remain unverified. The execution adapter now accepts the provider's `debug`
events as metadata. Full CLI remote execution is validated with declared binary
inputs and outputs, followed by a cache hit from an empty local cache without
creating another worker.

See the [Sprites guide](/guide/infrastructure/sprites) for configuration,
authentication, retention, and manual cleanup.

Struo also provides `struo cache serve`, a native bearer-authenticated HTTP
cache server with bounded streaming, atomic writes, restart persistence, and
disk eviction. Run it behind a TLS reverse proxy with the HTTP cache provider.
Automatic Sprites provisioning still uses the bundled Python service.
