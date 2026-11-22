---
title: Run annotated scripts on Fly.io Sprites
date: 2026-11-21
---

You can select `kind = "sprites"` as an execution infrastructure for annotated
scripts. Struo creates a disposable Sprite, transfers declared inputs including
`node_modules`, runs the command, retrieves declared outputs, and deletes the
Sprite. WebSocket execution preserves separate binary stdout and stderr and
the process exit status.

Struo journals owned Sprite names and immutable IDs for recovery after interrupted runs.
Ambiguous creates require manual cleanup rather than risking deletion of a replacement Sprite. Configure
`SPRITES_TOKEN` outside the script and follow the [Sprites guide](/docs/guide/infrastructure/sprites)
for setup, environment requirements, and cleanup limits.
