---
title: Run tests in Runloop Devboxes
date: 2026-11-21
---

Run annotated scripts in a fresh Runloop Devbox using a configured Blueprint ID.
Struo transfers declared inputs, including local `node_modules`, captures stdout
and stderr, restores declared outputs, and confirms Devbox shutdown after execution.

The adapter retains resource admission and archive validation, and recovers owned
Devboxes through journaled metadata after interrupted runs. See
[Runloop Devboxes](/guide/infrastructure/runloop) for setup and platform requirements.
