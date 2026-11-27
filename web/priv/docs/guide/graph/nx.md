# Nx Workspaces

Build and test an existing Nx workspace with Struo: it reads projects and tasks from Nx, then caches each task and runs it without `nx run`.

You do not translate anything into `struo.toml`. Nx stays the source of truth
for projects, tasks, and `dependsOn`; Struo schedules and caches the work.

## Prerequisites

Struo invokes Node.js and the workspace's Nx binary. Confirm both are
available:

```sh
node --version
nx --version
```

If the repository pins Node.js with [mise](https://mise.jdx.dev/) or a similar
tool, install and activate it first. Struo prefers the workspace copy of Nx
under `node_modules/nx`, so install dependencies with your package manager
(`npm install`, `pnpm install`, or `yarn install`).

## Start from an existing project

### Inspect the derived graph

From the directory that contains `nx.json`, check the match and list the
targets:

```sh
struo query workspace
struo query targets --kind nx_task
```

These commands do not write a `struo.toml`. Struo emits one target per project
and task pair, such as `my_app__build`: `nx_test` for tasks named `test` and
`nx_task` for the rest. Dependencies follow Nx's `dependsOn`, including the
upstream `^` form and the object form. Root dotenv files load the same way the
installed Nx version loads them.

### Build a task

```sh
struo build my_app__build
```

The first run is a cache miss and executes the task. A second run with
unchanged inputs is a cache hit and restores the task's outputs without
rerunning it.

### Run tests

```sh
struo test
```

Each project's `test` task becomes one cacheable suite. Both application and
library suites run by default. Struo records the exit status, duration, and
log; a failed command produces a failed suite.

To list only the suites a change can affect, pass the changed files:

```sh
struo query affected-tests --changed-path libs/core/src/index.ts
```

Like `nx affected`, a change to a project selects its own tests and the tests
of every project that depends on it, directly or through other projects. A
project rooted at the workspace root does not pass its changes on to the
projects that depend on it, because it would otherwise claim every file.

## Choose which tasks to include

By default Struo emits the `build`, `test`, and `lint` tasks of every project,
plus every task they depend on. A build that depends on `compile` keeps that
prerequisite even though `compile` is not in the list. Upstream `^`
dependencies skip projects that do not define the task.

To change the selection, declare an [`nx_workspace`](/reference/prelude/nx_workspace)
target and set `targets`. An empty list includes every task:

```toml
[[target]]
name = "nx"
kind = "nx_workspace"
srcs = ["nx.json", "package.json"]

[target.attrs]
resolver_inputs = ["nx.json", "package.json", "**/project.json", "**/package.json"]
targets = []
```

Set `graph_file` to a checked-in Nx project-graph JSON snapshot to load the
graph without Node.js or `node_modules/nx`. Include external package nodes in
the snapshot when tasks use plugin executors.

## How tasks run

Struo runs each task itself and never calls `nx build`, `nx test`, or Nx's
task runner. How it runs depends on the task's executor:

- `nx:run-commands` and its aliases (`@nx/run-commands:run-commands`,
  `@nrwl/run-commands:run-commands`, `@nx/workspace:run-commands`,
  `@nrwl/workspace:run-commands`) run the command from `options.command` or
  `options.commands` in a POSIX shell on Linux and macOS, or `cmd.exe` on
  Windows. `options.args` are forwarded unless `forwardAllArgs` is false.
- `nx:run-script` runs the package-manager command Nx resolved in
  `metadata.runCommand`, from the project directory.
- Other executors, including plugins, run through the installed Nx
  `runExecutor` API with the task's default configuration.

Nx `outputs` are treated as possible outputs. Struo caches the paths that exist
after a run and restores them to the same place; a missing match, such as a
coverage directory when coverage is off, is not an error.

## Inputs and environment

Struo hashes the workspace sources and installed dependencies, excluding
declared outputs, runtime caches such as `node_modules/.vite`, and files that
`.nxignore` or `.gitignore` exclude, as Nx's file map does. A test that writes
gitignored fixtures into the source tree therefore stays cached. Ignored files
are neither hashed nor sent to remote workers, so a task that reads one, such
as `.env.local`, does not rerun when it changes; add it to the task's Nx
`inputs` if it should. Unlike Nx, Struo reads only the ignore files inside the
workspace, not `.git/info/exclude`, your global excludes file, or a
`.gitignore` above the workspace root, so every checkout gets the same cache
keys. This is more
conservative than Nx's named inputs, so a change can rerun more tasks than Nx
would.

Tasks do not inherit your shell's `PATH`, so changing it does not invalidate
the cache. Their `PATH` holds the `node_modules/.bin` directories, the
directories of Node.js and of the workspace's package manager, and the system
directories. To use a tool installed elsewhere, add `{"env": "PATH"}` to the
task's Nx `inputs`; your `PATH` then becomes part of the cache key. `HOME` is
not passed either; add `{"env": "HOME"}` when a task needs it.

## Keep Nx authoritative

When you edit `nx.json`, add or remove a project, or change `dependsOn`, Struo
picks up the change on the next graph load. Your package manager
still installs dependencies and updates lock files.

## Limitations

- Test suites do not support case listing, case filtering, or sharding within
  a suite. Individual assertions are not imported; native output stays in the
  log.
- Struo does not install SDKs, restore NuGet packages, or download browser
  binaries that tasks expect.

## Target kinds

- [`nx_workspace`](/reference/prelude/nx_workspace): the workspace seed and
  its `targets` and `graph_file` attributes.
- [`nx_task`](/reference/prelude/nx_task): one project task.
- [`nx_test`](/reference/prelude/nx_test): one project's test suite.

## Next steps

- [Turborepo](/guide/graph/turborepo): the same workflow for Turborepo
  workspaces.
- [Testing and scheduling](/guide/graph/testing): plan affected tests and
  replay them on CI workers.
- [Remote execution](/guide/infrastructure/remote-execution): send uncached
  work to a configured execution provider.
