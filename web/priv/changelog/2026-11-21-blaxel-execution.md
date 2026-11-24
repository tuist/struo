---
title: Run annotated scripts in Blaxel Sandboxes
date: 2026-11-21
---

Struo can now run uncached commands and annotated scripts in fresh Blaxel
Sandboxes. Configure a named `blaxel` infrastructure with a prepared image,
then select it with `--compute` or a script's `# struo remote` header.

Struo resolves the workspace before cache lookup, keeping results separate
across workspaces even when image names match. Prepared images need Python 3
to preserve literal command arguments and selected environment values.

Declared inputs, including local `node_modules`, transfer through bounded
multipart archives. Struo retrieves declared outputs after accepted exits,
captures binary stdout and stderr separately, and explicitly deletes each
sandbox. Journaled operation names and labels support ownership-checked cleanup
after interrupted runs.

See the [Blaxel guide](/guide/infrastructure/blaxel) for authentication,
environment requirements and lifecycle limits.
