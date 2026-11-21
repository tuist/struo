---
title: Run annotated test scripts on Northflank
date: 2026-11-21
---

You can configure Northflank as an execution provider for annotated scripts.
Struo creates a service per uncached action in your selected project, transfers
declared inputs including dependency trees, captures stdout and stderr, and
retrieves declared outputs after accepted exit codes.

The adapter uses bounded binary transfers and the shared remote execution quota.
Struo deletes services after execution or cancellation and journals ownership
for recovery after interrupted creates or failed deletions. Northflank's
service expiry provides a fallback pause and scheduled deletion.

See [Northflank execution](/docs/guide/infrastructure/northflank) for setup,
project permissions, image requirements, and BYOC isolation configuration.
