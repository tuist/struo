# Bazel Workspaces

Query, build, and test an existing Bazel workspace with Struo: no manifest needed, Bazel keeps analysis, and Struo executes and caches the actions.

Struo reads the workspace through `bazel query` and exposes every rule as a
Struo target. Bazel still loads targets, resolves toolchains, and downloads
external repositories. Struo runs the resulting actions itself, and falls back
to the native `bazel` command for actions it does not support.

## Prerequisites

Install Bazel, or Bazelisk to use the version pinned by the workspace, and
check that it resolves in the shell that runs Struo:

```sh
bazel --version
```

A workspace that cannot load through the raw `bazel` command cannot load
through Struo either. When graph loading fails, run the same `bazel query`
from the workspace root to diagnose it.

Add the Struo cache directory to `.bazelignore` next to `MODULE.bazel`:

```
.struo
```

Without that entry, `BUILD.bazel` files that other integrations write under
`.struo/out/` can be picked up by `bazel query` and fail graph loading.

## Start from an existing project

### Inspect, build, and test

Struo recognizes `MODULE.bazel`, `WORKSPACE.bazel`, and `WORKSPACE`
automatically. From the workspace root, without a `struo.toml`:

```sh
struo query targets
struo build
struo test
```

`struo query targets` lists every rule returned by
`bazel query 'kind("rule", //...)'`. Like `bazel build //...` and
`bazel test //...`, `struo build` builds every rule, tests included, and
`struo test` runs every test rule, both skipping rules tagged `manual`. Those
still build and test when you name them. Discovery skips the
generated `bazel-bin`, `bazel-out`, and `bazel-testlogs` directories and does
not write a Struo manifest.

### Keep Bazel commands

Struo can sit behind the `bazel` command for one workspace. Add this mise
wrapper, then run `mise reshim` and activate mise in the shell that starts the
build:

```toml
[wrappers.bazel]
command = "struo"
args = ["bazel", "--"]
```

Labeled build and test commands then run through the Struo graph:

```sh
bazel build //src:kura
bazel test //src:kura_lib_test
```

The wrapper accepts one fully qualified label per invocation. Every other
request, including wildcards (`//...`), extra flags (`--config=debug`),
queries, and `bazel run`, runs the system `bazel` executable with its original
arguments and exit status.

To troubleshoot without mise, call the compatibility command directly and put
the separator before the Bazel arguments:

```sh
struo bazel -- --version
struo bazel -- query "kind(rule, //:*)"
```

Set `STRUO_BAZEL_PATH` to select a specific `bazel` executable while the
wrapper is active. By default Struo searches `PATH` and honors
[`bazelisk`](https://github.com/bazelbuild/bazelisk) shims.

## Choose which rules to include

Declare a [`bazel_workspace`](/reference/prelude/bazel_workspace) target in
`struo.toml` when the defaults do not fit:

- `query` replaces the default `kind("rule", //...)` expression.
- `exclude_packages` drops package prefixes from the default query.
- `test_query` selects which tests a bare `struo test` runs, without removing
  any targets from the graph.
- `env` and `test_env` set the environment for Bazel and for native tests.

Struo does not read test filters from `.bazelrc`. If your native suite skips
tagged tests, declare the matching `test_query`.

## Limitations

- Compile and link actions are cached only when the workspace declares
  `toolchain_inputs` covering every host tool, system header, and library the
  actions use. Without it, those actions run on every build.
- Actions Struo does not support run through the native `bazel` command. Struo
  does not cache them; Bazel's own cache applies.
- Test execution itself is not cached, and Struo does not implement Bazel's
  flaky-test retries.
- Bazel targets cannot be authored by hand in `struo.toml`; they always come
  from the workspace.
- Native actions can contain absolute host paths, so cached results are not
  portable across machines. Declaring `toolchain_inputs` does not enable remote
  execution.

## Target kinds

- [`bazel_workspace`](/reference/prelude/bazel_workspace): the workspace seed,
  its queries, environment, and caching inputs.
- [`bazel_target`](/reference/prelude/bazel_target): library-style rules.
- [`bazel_binary`](/reference/prelude/bazel_binary): rules whose class ends in
  `_binary`.
- [`bazel_test`](/reference/prelude/bazel_test): test rules, including
  per-test coverage.

## Next steps

- [`struo bazel`](/reference/cli/bazel): the full compatibility command
  reference.
- [Testing and scheduling](/guide/graph/testing): plan and run Bazel tests with
  Struo's test commands.
- [Ecosystems](/guide/graph/ecosystems): export a graph reproduction when the
  derived graph disagrees with Bazel.
