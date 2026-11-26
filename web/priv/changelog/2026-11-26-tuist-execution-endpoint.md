---
title: Override the Tuist execution endpoint
date: 2026-11-26
---

`TUIST_EXECUTION_ENDPOINT` replaces the remote execution endpoint the Tuist
server advertises, the way `TUIST_CACHE_ENDPOINT` replaces the cache endpoint.
Builds that run inside the execution service's own network, such as on Tuist
runners, can connect to it directly instead of through its public address. See
[Tuist](/guide/infrastructure/tuist).
