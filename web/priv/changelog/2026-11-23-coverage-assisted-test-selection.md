---
title: Opt-in coverage-assisted test selection with Tuist
date: 2026-11-23
---

You can publish isolated native test coverage to Tuist and request approximate
whole-target exclusion with `struo query test-plan --coverage-selection`.
Graph-based selection remains the strict default.

The planner discovers current test inventory on fresh workers and retains
graph-selected work when history, instrumentation or provider access is
insufficient. Native producers cover Go, pytest, RSpec, Minitest, Jest, Vitest,
Rust, JVM and Android host tests, ExUnit, CTest, Zig, Bazel, ShellSpec and Apple
host test bundles. Toolchains own collection and attribution; shared selection
uses one normalized contract. Complete atomic invocation profiles remain
distinct from individual native case observations.

Nx and Turborepo tasks reuse exact Jest or Vitest collection when they resolve
to one native test command without task outputs. Native workspace seeds can
forward coverage opt-in without redeclaring their discovered test targets.

Enable `coverage_per_test` on supported test targets and publish a full
observation with `struo test --all --coverage-report` before using coverage
selection. Selected execution does not replace full-run checkpoints.
