---
title: Rust library compilation can overlap dependency code generation
date: 2026-11-27
---

Local Rust library builds on Linux and macOS can start compiling dependents
as soon as dependency metadata is ready, without waiting for code generation
to finish. Metadata and link artifacts come from the same compiler invocation.
Final linking, procedural macros, and build scripts still wait for full artifacts.

The producer keeps its resource reservation until it finishes. Metadata is
hashed before consumers start and checked again at completion, and a producer
failure still fails the build. Sandboxed builds, remote execution, Windows,
and materialized registry sources retain the completion barrier.

Incremental compilation remains disabled. This change does not introduce
mutable compiler state into cacheable actions.

Project target kinds can use the generic `ready_outputs` action declaration
and opt into early dependency analysis with `accepts_ready_dependencies`.
See the [Starlark modules reference](/reference/modules) for the contract.
