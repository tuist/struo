# Introduction to Struo, a Build Cache and Execution Tool

Struo is a build cache and execution tool for scripts, native projects, and
typed build graphs. Reuse results across laptops, CI, and coding agents.

A fast build is often a build you do not have to run. Struo gives work a stable
identity, so a result produced by you, a teammate, or a coding agent can be
reused instead of repeated. You can adopt it one action at a time: read native
project metadata, cache the automation that already works, and add a typed graph
where more structure helps.

## Try Struo with one script

Start with [Getting Started](/guide/getting-started). In a small,
self-contained example, you run a script, reuse its result, restore a deleted
output, and see an input change trigger fresh work. No account or remote service
is needed.

## Start with an existing project

Struo can recognize supported native workspaces and expose their build, test,
and run capabilities without changes to their project files. Native project
descriptions and lockfiles remain authoritative. Read the
[Typed Graph guide](/guide/graph/#start-from-a-native-project) to discover and
preview supported projects without adding Struo configuration.

## Cache existing automation

Use [Scripted automation](/guide/scripted/) to add explicit inputs, outputs,
environment, and cache policy to commands or scripts that already work. The
script remains the source of truth, while Struo caches operations such as
packaging, generation, and checks.

## Add structure when it helps

Use the [Typed Graph guide](/guide/graph/) for named targets, typed
dependencies, build and run capabilities, and ecosystem integrations. Native
discovery and cacheable scripts keep working beside typed targets while the
graph grows. Add a package manifest when you want to keep a choice in the
repository, and write a project [Starlark](https://starlark-lang.org/) module
only when the built-in target kinds cannot express what your project needs.

- [Testing and scheduling](/guide/graph/testing) covers normalized test
  results, selection, batching, retries, and custom test runners.
- [Linting](/guide/graph/linting) covers cacheable static analysis, shared
  findings, and failure policy.
- [Ecosystems](/guide/graph/ecosystems) helps you choose between typed targets
  and scripted automation.

## Run work where it makes sense

Struo runs actions locally first. As a project grows, control how much work runs
at once, then share cached results or move suitable work to remote execution.

- [Memory limits](/guide/local-execution/memory-limits) keep concurrent local
  actions within a budget on constrained or shared machines.
- [Unchanged builds](/guide/local-execution/unchanged-builds) explains how
  Struo answers "nothing to do" quickly, and what to set in one-shot containers.
- [Infrastructure](/guide/infrastructure/) adds shared cache storage, isolated
  sandboxes, or hosted execution once the local workflow works.

## Bring Struo into your workflow

The command line works on its own. Use these integrations when an application or
a coding agent needs the same cache, graph, and execution model.

- [Software development kits](/guide/sdk/) give application code direct access
  to Struo cache primitives.
- [Coding harnesses](/guide/harness) give a coding agent query, editing,
  execution, validation, and evidence tools through the
  [Model Context Protocol](https://modelcontextprotocol.io/).

## Next steps

- [Getting Started](/guide/getting-started): install Struo and cache your first
  script.
- [Why Struo](/guide/why): read the reasoning behind the action model.
- [Typed Graph guide](/guide/graph/): build an existing Cargo, Swift, Xcode, or
  Bazel project.
