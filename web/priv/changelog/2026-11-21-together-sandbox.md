---
title: Run scripts in Together Code Sandbox
date: 2026-11-21
---

Struo now supports Together Code Sandbox as a named execution provider. Configure
`kind = "together"` with an existing snapshot ID, then select it with
`struo exec --remote --compute <provider>` or a script's remote header.

Struo transfers declared inputs, including dependency directories, and restores
validated declared outputs after a successful command. It preserves binary
stdout and stderr through bounded file downloads after execution. Each sandbox
gets a TTL and explicit termination, with journaled ownership for recovery after
an interrupted run. Cache hits do not create a sandbox.

Your organization needs Together sandbox access and an API key. Live provider
verification remains pending access and credentials. Read the
[Together guide](/docs/guide/infrastructure/together) for snapshot requirements,
configuration, and lifecycle limits.
