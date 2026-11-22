---
title: Run annotated scripts in disposable Boat sandboxes
date: 2026-11-21
---

Struo now supports Boat as an execution provider. Select a saved snapshot with
`kind = "boat"` and `snapshot` in a named infrastructure, then use that name
from an annotated script or `struo exec --remote --compute`.

Struo uploads declared inputs, including local `node_modules`, captures binary
stdout and stderr, and retrieves declared outputs after accepted exits. It
requests permanent sandbox deletion after execution and retains failed
ownership records for recovery. Boat handles data erasure asynchronously after
accepting deletion; TTL alone only stops the sandbox.

Read the [Boat guide](/docs/guide/infrastructure/boat) for credentials,
snapshot preparation, lifecycle behavior, and current limits.
