---
title: Nx test suites with any name, upstream dependencies that match Nx, and cache keys shared across pnpm checkouts
date: 2026-11-27
---

The `nx_workspace` target takes a new `test_targets` attribute that lists the
Nx tasks that run test suites, such as `test:lib` or `test:unit`. They become
test targets that `struo test` and `struo query affected-tests` select. It
defaults to `test`, as before.

Upstream `dependsOn` entries now resolve the way Nx resolves them. When a
dependency project lacks one of a task's `^` targets, every `^` entry of the
task applies again from that project's dependencies, so a task that depends on
`^build` and `^compile` also waits for the builds behind a project with no
`compile` task. The object form `{"target": "build", "dependencies": true}` is
read as `^build` instead of as a task in the same project.

A Vitest suite with typechecking no longer invalidates every Nx task. Vitest
writes a `.tsbuildinfo` file into its own package under `node_modules`, and
Struo hashed it as an input of every task. TypeScript build info inside
`node_modules` is now treated as a runtime cache, like `node_modules/.vite`.

pnpm workspaces now share cache keys across checkouts. pnpm writes the
checkout path and install time into `node_modules/.modules.yaml`, which made
every Nx task key differ per clone. Install records and tool caches inside
`node_modules`, such as `.modules.yaml`, `.package-lock.json`,
`.yarn-state.yml`, `.vite`, `.vitest`, and `.cache`, are no longer task inputs.
Package stores such as `.pnpm`, `.bun`, and `.deno`, and generated code such as
`.prisma`, still are.
