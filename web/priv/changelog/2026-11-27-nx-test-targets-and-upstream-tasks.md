---
title: Nx test suites with any name, upstream dependencies that match Nx, and cached Vitest typechecks
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
