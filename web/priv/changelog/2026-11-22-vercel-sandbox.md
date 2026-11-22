---
title: Run annotated scripts in Vercel Sandbox
date: 2026-11-22
---

Vercel Sandbox is now an execution provider. Select a named `kind = "vercel"`
infrastructure with a runtime such as `node24`, then run annotated scripts
remotely with declared inputs, including local `node_modules`.

Struo captures binary stdout and stderr separately, restores declared outputs
after accepted exits, and deletes each disposable sandbox. Durable ownership
tags support cleanup after interrupted runs. Vercel access credentials and
team/project scope are required. Live provider validation remains pending.

See [Vercel Sandbox](/docs/guide/infrastructure/vercel) for setup and limits.
