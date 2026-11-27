---
title: Lower latency for HTTP cache restores
date: 2026-11-27
---

Struo's HTTP cache server now reuses HTTP/1.1 connections instead of closing
them after each response. Connection capacity follows the server's memory
budget, including idle connections.

Remote cache reads no longer send a separate HEAD request before each GET.
Struo still checks referenced content for eviction and verifies downloaded
lengths and blob digests. Memory and staging budgets remain enforced.
