---
title: Tensorlake remote execution
date: 2026-11-23
---

Run declared scripts in a prepared Tensorlake image or snapshot with
`kind = "tensorlake"`. Struo stages inputs, preserves binary command output,
retrieves declared artifacts after accepted exits, and terminates the sandbox.

The adapter supports ordinary named sandboxes, not warm-pool claims. Lost
creation responses without a recorded sandbox ID require manual investigation.
Tensorlake retains terminated sandboxes for restartability for 48 hours; cleanup
does not imply immediate data erasure. Live provider validation remains pending.

See the [Tensorlake guide](/guide/infrastructure/tensorlake) for setup and limits.
