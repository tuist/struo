---
title: Hosted simulator test batches no longer kill each other
date: 2026-11-26
---

- `struo test` no longer runs two test bundles hosted by the same app on one
  simulator at once. Before, both batches launched the same host app and one
  was killed before it connected. Batches for different host apps and other
  targets still run in parallel.
- Actions that name the same `run_action` resource slot now wait for each
  other across Struo processes, not only within one, so the parallel batches
  of `struo test` respect them too.
