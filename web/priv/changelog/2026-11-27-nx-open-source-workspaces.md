---
title: Nx workspaces select dependent tests and run Angular and Jest plugin tasks
date: 2026-11-27
---

Running Struo against open-source Nx workspaces found six gaps, now fixed:

- `struo query affected-tests` selects the tests of every project that depends
  on a changed project, as `nx affected` does. Before, it selected only the
  changed project's own tests.
- Plugin tasks whose tools fork Node.js workers, such as Jest through
  `@nx/jest:jest`, no longer crash with "Jest worker encountered 4 child
  process exceptions".
- Plugin executors that return their result instead of yielding it, such as
  `@nx/angular:package`, no longer fail after a successful build.
- Workspaces installed with pnpm on Nx versions that ship the CLI under
  `dist/bin` load their graph again, instead of failing with
  `Cannot find module 'dotenv-expand'`.
- Outputs declared with a trailing slash, such as `outputPath: "dist/app/"`,
  no longer count as inputs of every other task, so a second run is cached.
- Loading a graph with many plugin tasks and a large external package graph
  uses a fraction of the memory it did, so it no longer runs out of memory on
  small machines.
