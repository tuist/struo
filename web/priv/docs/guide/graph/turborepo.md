# Turborepo Workspaces

Build and test an existing Turborepo workspace with Struo: it reads packages and `turbo.json` tasks, then caches each task and runs it locally or remotely without `turbo run`.

You do not translate anything into `struo.toml`. Turborepo stays the source of
truth for packages, tasks, and `dependsOn`; Struo schedules and caches the
work.

## Prerequisites

Struo invokes Node.js and the workspace's turbo binary while loading the
graph. Confirm both are available:

```sh
node --version
npx turbo --version
```

Struo requires Turborepo 2. It prefers the workspace copy of turbo under
`node_modules/turbo`, so install dependencies with your package manager
(`npm install`, `pnpm install`, `yarn install`, or `bun install`).

## Start from an existing project

### Inspect the derived graph

From the directory that contains `turbo.json` or `turbo.jsonc`, check the
match and list the targets:

```sh
struo query workspace
struo query targets --kind turbo_task
```

These commands do not write a `struo.toml`. Struo emits one target per package
and task pair, named `<package>__<task>`: `turbo_test` for tasks named `test`
and `turbo_task` for the rest. Characters other than letters, digits, and
underscores become underscores, so `@acme/ui#build` becomes `_acme_ui__build`,
and root tasks such as `//#format` become `root__format`. Dependencies follow
turbo's own resolution of `dependsOn`.

### Build a task

```sh
struo build web__build
```

The first run is a cache miss and executes the script. A second run with
unchanged inputs is a cache hit and restores the task's outputs without
rerunning it. Building the `turbo` seed builds every `build` task.

### Run tests

```sh
struo test
```

Each package's `test` task becomes one cacheable suite. Struo records the
script's exit status, duration, and log.

## Choose which tasks to include

By default Struo emits the `build`, `test`, and `lint` tasks, plus every task
they depend on. Task names that no package defines are skipped. Tasks without
a script, such as transit nodes, do not become targets; their dependents depend
directly on the runnable tasks behind them. Persistent and interactive tasks,
such as `dev`, are left out.

To change the selection, declare a
[`turbo_workspace`](/reference/prelude/turbo_workspace) target and set
`targets`. An empty list includes every task:

```toml
[[target]]
name = "turbo"
kind = "turbo_workspace"
srcs = ["turbo.json", "package.json"]

[target.attrs]
resolver_inputs = ["turbo.json", "package.json", "**/package.json"]
targets = ["build", "test", "lint", "typecheck"]
```

Set `graph_file` to a checked-in snapshot to load the graph without Node.js or
`node_modules/turbo`. The snapshot is a JSON object with `packages`, the
package items from `turbo query`, and `dry`, the output of
`turbo run <tasks> --dry=json`.

## How tasks run

Struo runs each package script itself, from the package directory, in a POSIX
shell on Linux and macOS or `cmd.exe` on Windows. It never calls `turbo run`.

Turborepo `outputs` are treated as cache patterns, not required products.
Struo caches the paths that exist after a run, honoring `!` negations and
`$TURBO_ROOT$`, and restores them to the same place. A task with
`"cache": false` runs on every build and its outputs are not restored.

## Inputs and environment

Turbo hashes files to decide when to rerun a task, but the task reads whatever
is on disk. Struo instead declares exactly what each task receives, so the
task can also run on a fresh remote machine. A package task receives:

- every file in its package and in the workspace packages it depends on,
  except declared outputs, `node_modules`, and `.turbo`
- the root `package.json`, lockfiles, `turbo.json`, `.npmrc`, root
  `tsconfig*.json` files, and the files from `globalDependencies`
- the installed `node_modules` of the workspace root and of those packages
- the outputs of the tasks it depends on, transitively

A root task receives the whole workspace. Turbo's task `inputs` setting does
not narrow these sets.

Environment variables follow turbo's strict mode: a task receives the
variables in its `env` and `passThroughEnv`, the global forms of both, and the
variables turbo matched through wildcards and framework inference. Their
values are part of the cache key.

Tasks do not inherit your shell's `PATH`, so changing it does not invalidate
the cache. Their `PATH` holds the `node_modules/.bin` directories from the
package up to the workspace root, the directories of Node.js and of the
workspace's package manager, and the system directories, so scripts such as
`pnpm with-env next build` still work. To use a tool installed elsewhere, list
`PATH` in `passThroughEnv`. `HOME` is not passed either; list it in
`passThroughEnv` when a task needs it.

## Run tasks remotely

Each task is one action with declared inputs and outputs, so
`struo build --remote` can send uncached tasks to a configured execution
provider:

```sh
struo build --remote --compute workers web__build
```

Struo keeps scheduling and caching on your machine and sends each task's
inputs, including installed dependencies, to a fresh worker. The worker image
must provide Node.js, and the package manager when scripts call it, at the
same paths as your machine.

## Keep Turborepo authoritative

When you edit `turbo.json`, add a package, or change a script, Struo picks up
the change on the next graph load. Your package manager
still installs dependencies and updates lock files.

## Limitations

- Test suites do not support case listing, case filtering, or sharding.
  Individual assertions are not imported; native output stays in the log.
- Package manager lifecycle hooks such as `prebuild` are not run, because
  Struo runs the script directly.
- Turbo's task `inputs` do not narrow what Struo hashes, so a change can rerun
  more tasks than turbo would.

## Target kinds

- [`turbo_workspace`](/reference/prelude/turbo_workspace): the workspace seed
  and its `targets` and `graph_file` attributes.
- [`turbo_task`](/reference/prelude/turbo_task): one package task.
- [`turbo_test`](/reference/prelude/turbo_test): one package's test suite.

## Next steps

- [Nx](/guide/graph/nx): the same workflow for Nx workspaces.
- [Remote execution](/guide/infrastructure/remote-execution): configure the
  provider behind `--remote`.
- [Testing and scheduling](/guide/graph/testing): plan affected tests and
  replay them on CI workers.
