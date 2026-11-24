# `minitest_test`

Ruby Minitest files run through Struo.

## Start Here

The selected `ruby` interpreter must be able to load `minitest/autorun`. Many
Ruby installations include Minitest. When the project needs the standalone
gem, install it in the Ruby environment used by Struo. The interpreter may be
a name on the executable search path, an absolute path, or a
workspace-relative path:

```sh
gem install minitest
```

Retrieve the runnable starter when you want a complete declaration and sample
tests:

```sh
struo query example minitest_test minitest-test-minimal --format json
```

Copy the declaration below into `struo.toml`, adjust `srcs` and `ruby` for the
project, then establish discovery and run the file batches:

```sh
struo test tests --format json
struo query test-manifest tests --format json
struo test tests --jobs 4 --format json
```

The first command runs the complete target. Later runs schedule each
discovered test file independently. Minitest methods are not separate Struo
units. See [Testing and Scheduling](/guide/graph/testing) for affected plans
and file-level exact execution.

## Description

`minitest_test` treats each declared test file as one stable unit. It invokes
each selected file with Ruby, captures its output, and writes normalized Struo
results. This makes file selection exact without depending on Minitest class or
method naming conventions.

After a complete run, Struo schedules the files as independent batches and
balances them using historical durations. Individual Minitest methods are not
separate Struo units.

## Attributes

| Attribute | Type | Required | Default | Description |
| --- | --- | --- | --- | --- |
| `ruby` | string | no | `ruby` | Ruby interpreter name, absolute path, or workspace-relative path that can load Minitest |
| `config` | list&lt;string&gt; | no | `Gemfile`, `Gemfile.lock` | Dependency and runner configuration inputs |
| `data` | list&lt;string&gt; | no | `[]` | Runtime data and support inputs |
| `args` | list&lt;string&gt; | no | `[]` | Arguments passed to each test file |
| `coverage_per_test` | bool | no | `false` | Measure each complete file invocation independently with Ruby Coverage |
| `coverage_sources` | list&lt;string&gt; | no | `[]` | Product Ruby source globs to instrument |
| `env` | map&lt;string, string&gt; | no | `{}` | Explicit test environment variables |
| `env_inherit` | list&lt;string&gt; | no | `[]` | Host environment variables inherited by name |
| `batching` | string | no | `file` | Accepted for parity; file units remain the scheduling boundary |
| `labels` | list&lt;string&gt; | no | `[]` | Labels exposed through test discovery |
| `timeout_ms` | int | no | empty | Optional timeout in milliseconds |

## Dependency Edges

`deps` names targets whose changes should select this test target.

## Providers and Capabilities

The target emits `struo_test_info` and exposes `test` with the `default`,
`test_results`, and `logs` output groups.

`test-discovery` emits the current declared file invocation units without
running test bodies. Opt-in `coverage_per_test` measures each complete file
invocation separately. Its evidence belongs to that file unit, never to the
individual Minitest methods it runs. Declare product Ruby paths in
`coverage_sources`. Ruby Coverage must support `line_stub` for unloaded
sources. Measurement bypasses the action cache; child processes and failed
invocations cannot establish complete replacement checkpoints.

## Example

```toml
[[target]]
name = "tests"
kind = "minitest_test"
srcs = ["test/**/*_test.rb"]
```

Shared helpers and runtime files must be included in `srcs` or `data`.
