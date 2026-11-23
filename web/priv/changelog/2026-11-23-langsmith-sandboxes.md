---
title: Run annotated scripts in LangSmith Sandboxes
date: 2026-11-23
---

Struo supports LangSmith Sandboxes as an execution provider. Configure a prepared
v2-runtime snapshot with `kind = "langsmith"` and `snapshot`, then select the
infrastructure from an annotated script or `struo exec --remote --compute`.

Struo stages declared inputs, preserves binary stdout and stderr, and retrieves
declared outputs after accepted exits. It deletes the sandbox after completion
or cancellation and retains uncertain creation or cleanup records for recovery.
This provider does not supply shared caching.

Read the [LangSmith guide](/docs/guide/infrastructure/langsmith) for prerequisites,
workspace authentication, trusted sandbox origins and limits. Local protocol
tests cover the adapter; live provider execution and teardown remain unverified.
