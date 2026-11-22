---
title: "Self-hosted execution with OpenSandbox"
date: 2026-11-21
---

You can now select [OpenSandbox](/docs/guide/infrastructure/opensandbox) as an
execution provider. Configure a Linux image and your lifecycle API endpoint to
run uncached actions on your own sandbox service.

Struo transfers declared inputs, including dependency directories, captures
stdout and stderr, retrieves declared outputs, and deletes the owned sandbox.
It forwards the guest access headers returned by endpoint resolution and
retains ownership records for cleanup recovery after interrupted runs.
