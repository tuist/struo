---
title: Nx task inputs skip gitignored files
date: 2026-11-27
---

Nx tasks no longer hash files that `.gitignore` or `.nxignore` exclude, which
matches the files Nx itself considers. A test that writes ignored fixtures into
the source tree, such as generated translation files, now stays cached on the
next run instead of invalidating itself. Rules apply the way Git applies them,
nearest file first, and only ignore files inside the workspace count, so two
checkouts produce the same cache keys. Editing an ignore file reruns the tasks
it covers. Ignored files are also not sent to remote workers, so a task that
reads one, such as `.env.local`, should list it in its Nx `inputs`.

Module authors can use the same filtering through the new `ignore_files`
option of `glob`.
