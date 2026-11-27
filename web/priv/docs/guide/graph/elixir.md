# Elixir and Mix

Build, lint, test, release, and cache Elixir projects with Struo, straight from
an existing `mix.exs` or from Elixir targets declared in `struo.toml`.

Struo reads a Mix project without any configuration and caches the compiled
project and each locked dependency separately. Hand-authored targets add
standalone libraries, custom lint and release tasks, and test setup.

## Prerequisites

Struo invokes `elixir`, `erl`, and `mix` from the selected toolchain. Native
project evaluation requires Erlang 27 or newer. Confirm that the versions the
project expects are available:

```sh
elixir --version
mix --version
```

If the repository pins them with [mise](https://mise.jdx.dev/), install that
configuration first:

```sh
mise install
mise exec -- elixir --version
mise exec -- mix --version
```

Fetching packages from Hex requires a local Hex installation. A dependency
built with Rebar also requires [Rebar 3](https://rebar3.org/):

```sh
mix local.hex --force
mix local.rebar --force
```

Struo does not download dependencies or change `mix.lock`. Materialize the
exact locked sources before loading the graph:

```sh
mix deps.get --check-locked
```

Run `mix deps.get` without `--check-locked` once when the lockfile needs to be
created or deliberately updated, and review that change. A project with
external dependencies needs a concrete `mix.lock`; a dependency-free project,
or one whose dependencies are all local paths, works without one.

## Start from an existing Mix project

From the directory that contains `mix.exs`, inspect, build, and test the
project without creating `struo.toml`:

```sh
struo query workspace
struo query targets
struo build
struo test
```

`struo build` follows `MIX_ENV`, defaulting to `dev`, while `struo test`
compiles and runs the test environment. Neither command writes a manifest.

In a Git repository, these commands discover only the current Mix project and
the path dependencies it reaches inside the repository. Outside a Git
repository, the project directory is the boundary, so use a containing
workspace with an explicit target when dependencies live outside it. An
explicit `struo.toml` between the project and the repository root takes
priority.

### Inspect the derived graph

The identifiers printed by `struo query targets` are the source of truth. For
a Mix project at the workspace root, discovery normally derives:

- `mix_application_dev`, `mix_application_test`, and `mix_application_prod`,
  which compile the project in each environment;
- one dependency target per environment when `mix.lock` is present;
- `mix_lint`, which checks unused dependencies and formatting;
- `mix_tests`, which runs the project tests when the project has files under
  `test/`;
- `mix_release`, which assembles a production release;
- `mix`, which builds the default development application.

Nested projects use package-qualified identifiers such as
`apps/accounts/mix_tests`.

### Build, lint, test, run, and release

Use the derived targets through the same commands as every other Struo graph:

```sh
struo build mix
struo run mix_lint
struo test mix_tests
struo run mix_application_dev -- phx.server
struo build mix_release
```

The argument after `--` is a Mix task chosen at invocation time; `phx.server`
is only an example, and discovery does not assume a framework. Native tests
keep the project's test aliases, test paths, and test file patterns, and run
against the compiled application without recompiling it.

`mix_lint` runs `mix deps.unlock --check-unused` and
`mix format --check-formatted`. It does not infer project tools such as Credo.
`mix_release` suits applications that support `mix release`. It does not run
framework preparation such as asset compilation; declare an explicit release
target with `pre_tasks` for that.

### Confirm caching

Run the same build twice without changing an input:

```sh
struo build mix_application_dev
struo build mix_application_dev
```

The second invocation restores unchanged actions from the configured cache.
Outputs are materialized under `.struo/out/`. Changing a dependency
invalidates that package and its consumers, and changing application source
invalidates the application. Compilation also tracks non-hidden project files
outside the build and dependency directories, such as a version file. Test
scratch files that ExUnit writes under the project's `tmp/` directory are not
tracked.

Builds, `mix_lint`, dependency compilation, and release assembly are
cacheable. `struo run` with a task is not cached, so servers and interactive
tasks run every time. Discovered tests also run on every invocation and
inherit the host environment, so suites that need a database or other service
exercise it each time. Continue with [Caching](/guide/scripted/caching) to
configure a shared remote cache.

### Umbrellas, nested projects, and path dependencies

An umbrella root builds its detected child projects, and nested Mix projects
get their own package-qualified targets. Use `struo query targets` to find
them rather than assuming every target lives at the root.

As in Mix, the umbrella root owns the locked dependencies. Struo resolves them
once per environment from the root's `mix.lock`, and each child compiles
against the packages it declares for the current target plus its
`in_umbrella: true` siblings, built in the child's environment. A child's
`path:` dependency outside the umbrella's apps resolves its own packages from
the root's `mix.lock` too, so it needs no lockfile of its own; as in Mix,
building such a project on its own fails until it has one. Run
`mix deps.get --check-locked` once in the root; an umbrella whose children
depend only on each other needs no lockfile. Commands run inside a child
directory build and test that child, loading the root and the siblings it
needs. Because only the root sees every child's dependencies, child lint
targets check formatting but not unused lockfile entries.

Targetless `struo test` includes discovered nested test suites, even though
`mix test` at the root runs only the root project. Each separate Mix project
outside an umbrella needs its own `mix deps.get --check-locked`. Provision any external services
that test aliases such as database creation and migration need before
running those suites.

## Declare targets

Hand-authored targets suit a standalone library without Mix, custom lint or
release tasks, database setup, or a boundary that should be cached
independently.

### Declare a standalone library and test

Create `apps/greeting/struo.toml`:

```toml
[[target]]
name = "greeting"
kind = "elixir_library"
srcs = ["lib/**/*.ex"]

[target.attrs]
app_name = "greeting"
mix_env = "test"

[[target]]
name = "greeting_test"
kind = "elixir_test"
srcs = ["test/**/*.exs"]
deps = ["./greeting"]

[target.attrs]
labels = ["unit"]
```

Use this source layout:

```text
apps/greeting/
├── struo.toml
├── lib/
│   └── greeting.ex
└── test/
    ├── test_helper.exs
    └── greeting_test.exs
```

An `elixir_test` depends on exactly one application compiled for the test
environment, which is why the library sets `mix_env = "test"`. The test reuses
that bytecode instead of compiling the application again.

### Query before building

Inspect the declared targets and their contracts:

```sh
struo query targets --kind elixir_library
struo query capabilities apps/greeting/greeting
struo query capabilities apps/greeting/greeting_test
struo query schema elixir_library
struo query schema elixir_test
```

The library exposes `build`. The test target exposes `test`. Neither exposes
`run`.

### Build and test targets

Build the application bytecode:

```sh
struo build apps/greeting/greeting
```

The bytecode and application metadata appear under
`.struo/out/apps/greeting/greeting/ebin`, with private files under `priv` and
compile warnings in `compile-warnings.log` beside them.

Run the tests against the compiled library:

```sh
struo test apps/greeting/greeting_test
```

Results and logs appear under `.struo/out/apps/greeting/greeting_test/test/`,
including `test_results.json` and `elixir-test.log`. Changing only a test file
reruns the test without recompiling the library.

Compile-time dependencies, such as macros or structs used from another target,
belong in separate `elixir_library` targets connected through `deps`. The
library does not require `mix.exs`; set `mix_config` only when the project
file should affect the build or tests must run through `mix test`.

## Manage dependencies

### Import locked Hex packages

Declare one `mix_dependencies` target for third-party packages. Mix evaluates
the active dependency graph from `mix.exs`, `mix.lock` supplies exact versions
and checksums, and Struo emits a `mix_package` target per locked package:

```toml
[[target]]
name = "mix_dependencies"
kind = "mix_dependencies"
srcs = ["mix.exs", "mix.lock", "mix-dependencies.json"]

[target.attrs]
graph_file = "mix-dependencies.json"
resolver_inputs = [
  "mix.exs",
  "mix.lock",
  "mix-dependencies.json",
  "local_helper/mix.exs",
]
vendor_dir = "deps"

[target.attrs.path_dependencies]
local_helper = "./local_helper"

[[target]]
name = "local_helper"
kind = "elixir_library"
srcs = ["local_helper/lib/**/*.ex"]

[target.attrs]
app_name = "local_helper"

[[target]]
name = "greeting"
kind = "elixir_library"
srcs = ["lib/**/*.ex"]
deps = ["./mix_dependencies"]
```

Fetch dependency sources into the vendor directory first; graph loading never
fetches packages or updates the lockfile. With `graph_file`, graph loading
reads a checked-in snapshot instead of running Mix. The snapshot is bound to
the exact `mix.exs`, `mix.lock`, resolver inputs, and `mix_env`, so it must be
regenerated when any of them change. Without it, Struo evaluates the project
offline with isolated Mix and Hex homes.

Map every active Mix path dependency to a Struo target through
`path_dependencies`; graph loading fails when one is missing. Use a separate
dependency target with a distinct `target_prefix` for each of the development,
test, and production graphs. When dependencies read compile-time application
configuration, declare the full root configuration set, including every file
that `config_entry` imports:

```toml
[target.attrs]
config = ["config/**/*.exs"]
config_entry = "config/config.exs"
```

Inspect the imported packages, then build and evaluate the bundled
dependency starter:

```sh
struo query targets --kind mix_package
struo build greeting
elixir \
  -pa .struo/out/mix-locked-greeting/mix/prod/lib/locked_greeting/ebin \
  -pa .struo/out/local_helper/ebin \
  -pa .struo/out/greeting/ebin \
  -e 'IO.write(Greeting.message("Struo"))'
```

The starter prints `Hello, Struo!`, which confirms that the locked package and
the mapped local helper both reached the application. The
[`mix_dependencies` reference](/reference/prelude/mix_dependencies) documents
the snapshot format and root selection.

### Compile with the project's Mix compilers

Declare `mix_project` instead of `elixir_library` for a first-party project
that needs its registered Mix compiler pipeline:

```toml
[[target]]
name = "dependencies_test"
kind = "mix_dependencies"
srcs = ["mix.exs", "mix.lock"]

[target.attrs]
resolver_inputs = [
  "mix.exs",
  "mix.lock",
  "deps/*/mix.exs",
  "deps/*/rebar.config",
]
mix_env = "test"
target_prefix = "mix-test"

[[target]]
name = "application_test"
kind = "mix_project"
srcs = ["lib/**/*", "test/support/**/*"]
deps = ["./dependencies_test"]

[target.attrs]
app_name = "greeting"
mix_env = "test"
manifest = "mix.exs"
compile_args = ["--warnings-as-errors"]

[[target]]
name = "tests"
kind = "elixir_test"
srcs = ["test/**/*.exs"]
deps = ["./application_test"]

[target.attrs]
cacheable = false
setup_tasks = [
  ["ecto.create", "--quiet", "--no-compile", "--no-deps-check"],
  ["ecto.migrate", "--quiet", "--no-compile", "--no-deps-check"],
]
data = ["priv/repo/migrations/**/*"]
```

Fetch the locked sources, then build and test:

```sh
mix deps.get --check-locked
struo build application_test
struo test tests
```

`mix_project` compiles the first-party project with dependency checks and
dependency compilation disabled, because each dependency is already its own
target. Set `cacheable = false` on a test, as above, when it must contact an
external service on every run. A `mix_project` can set `run_task`, or the
caller can pass a task after `--` to `struo run`.

## Lint and release

A `mix_project` can expose deterministic checks through `struo run`:

```toml
[target.attrs]
run_tasks = [
  ["deps.unlock", "--check-unused"],
  ["format", "--check-formatted"],
  ["credo"],
]
run_cacheable = true
```

Assemble a release from a compiled production project with `mix_release`:

```toml
[[target]]
name = "application_prod"
kind = "mix_project"
srcs = ["lib/**/*"]
deps = ["./dependencies_prod"]

[target.attrs]
app_name = "greeting"
mix_env = "prod"

[[target]]
name = "release"
kind = "mix_release"
srcs = ["mix.exs"]
deps = ["./application_prod"]

[target.attrs]
manifest = "mix.exs"
config = ["config/**/*.exs"]
```

Projects that need preparation can add ordered `pre_tasks`, documented by
[`mix_release`](/reference/prelude/mix_release). Declare the files and
executables those tasks use through `data` and `tools` so they take part in
caching. Build the release twice to confirm that the second build comes from
the cache:

```sh
struo build release
struo build release
```

## Build remotely

For `struo build --remote`, the worker image must already contain Erlang,
Elixir, Hex, and Rebar; actions do not install tools or fetch packages. Mix
discovery still runs locally, so keep the local toolchain and materialized
dependencies available too.

Describe the worker paths in the workspace configuration:

```toml
[infrastructure.execution.properties.elixir]
schema_version = 1
os = "linux"
arch = "x86_64"
elixir_bin = "/opt/elixir/bin"
erlang_bin = "/opt/erlang/bin"
mix_home = "/opt/mix"
rebar3 = "/opt/mix/rebar3"
```

These paths are illustrative and must exist inside the image. Add
`environment` with an image to bind the description to that exact image;
builds then fail if the execution image differs. Linux `x86_64` and `arm64`
are supported when the provider supports them. Changing the image or its
toolchain paths invalidates compiled results.

Select the provider per build with `struo build --remote --compute workers`
after configuring it. See [Remote Execution](/guide/infrastructure/remote-execution).

## Troubleshooting

- If graph loading reports a missing lockfile or dependency source, run
  `mix deps.get`, then retry with `mix deps.get --check-locked`.
- If `mix_tests` is absent, check whether the project has files under `test/`.
  `mix_application_test` is still available for a test-environment compile.
- If release assembly needs `assets.deploy` or another preparation task,
  declare a `mix_release` target with `pre_tasks`.

## Limitations

- External dependencies require a committed `mix.lock` for repeatable builds
  on other machines.
- Hex and Git dependencies built with Mix or Rebar 3 are supported. Packages
  that require Make or a custom compile command fail with an explicit
  diagnostic.
- `struo test` does not run remotely; keep test runs local.
- Remote toolchain descriptions do not configure cross-compilation or make
  incompatible native dependencies portable.
- Direct ExUnit options cannot be combined with Mix test mode.
- Compatibility attributes listed as reserved in a target kind reference fail
  validation when set to a non-empty value.

## Target kinds

- [`mix_workspace`](/reference/prelude/mix_workspace)
- [`mix_dependencies`](/reference/prelude/mix_dependencies)
- [`mix_package`](/reference/prelude/mix_package)
- [`mix_project`](/reference/prelude/mix_project)
- [`mix_release`](/reference/prelude/mix_release)
- [`elixir_library`](/reference/prelude/elixir_library)
- [`elixir_test`](/reference/prelude/elixir_test)

## Next steps

- [Caching](/guide/scripted/caching): share build results across machines
  with a remote cache.
- [Testing and Scheduling](/guide/graph/testing): select and schedule tests in
  larger graphs.
- [Memory](/guide/memory/): inspect the durable context Struo records for
  builds and tests.
