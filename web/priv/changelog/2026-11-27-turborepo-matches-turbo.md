---
title: Turborepo tasks hash, run and select like turbo
date: 2026-11-27
---

Turborepo workspaces now behave more like `turbo run` with no extra
configuration. A second `struo build` or `struo test` hits the cache: caches
Vite, Vitest and Babel keep in `node_modules`, pnpm's install bookkeeping, and
untracked files git ignores, such as `tsconfig.tsbuildinfo`, are no longer task
inputs, while committed files and files a task's turbo.json `inputs` name, such
as `.env*`, `$TURBO_ROOT$/.env`, a whole directory, or `{a,b}` alternatives,
still are. Workspaces nested inside a larger git repository and checkouts git
cannot read load and hash correctly.
Sources a task's outputs negate, as in `["*/**", "!src/**"]`, stay inputs.
Tasks run their `pre` and `post` package scripts when the workspace's package
manager would, and pnpm workspaces get pnpm's hoisted `NODE_PATH`.
`struo query affected-tests` follows turbo's package ownership: `README.md`
and CI files select no tests, and a change to a package selects the tests of
the packages that depend on it. Package scripts that turbo.json does not
configure no longer stop the graph from loading, and cached test runs keep
their discovery binding. In workspaces that mix Turborepo with other
ecosystems, a file outside every package still selects the other ecosystems'
tests.
