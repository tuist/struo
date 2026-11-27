# `turbo_test`

A cacheable test suite from a [Turborepo](https://turborepo.com) workspace.

## Description

The native Turborepo resolver emits `turbo_test` for tasks named `test`.
Other tasks remain [`turbo_task`](/docs/reference/prelude/turbo_task)
targets and do not advertise the test capability. The workspace seed
reports every package's suite as a first-party test root, so `struo test`
runs all of them.

The script runs with the same action contract as `turbo_task`, including
remote execution. Each task is one suite unit in `struo.test_results.v1`.
Its exit status determines success or failure, and its duration and log
are retained. Individual assertions are not imported. Case listing, case
filtering, and within-suite sharding are unsupported. Failed scripts
cannot produce a passing suite.

## Attributes

All attributes are shared with
[`turbo_task`](/docs/reference/prelude/turbo_task), including `package`,
`task`, `command`, `package_dir`, `outputs`, and `source_dirs`.

Set `coverage_per_test = true` with product `coverage_sources` for a package
script consisting of one native Jest or Vitest command. Native case discovery
and isolated measurement reuse the dedicated runner collectors, retaining
the package working directory and declared environment. Python 3 resolves argv
without evaluating shell expansion.

Shell pipelines, package-manager wrappers, multiple commands and declared task
outputs are not supported in this mode, and `pre_command` and `post_command`
do not run. They retain ordinary graph selection
with coverage disabled. A successful task alone is not complete case evidence.

## Providers

The target emits `turbo_task` and `struo_test_info`.

## Capabilities

| Capability | Output groups |
| --- | --- |
| `build` | none, executes when used as a prerequisite |
| `test` | `default`, `test_results`, `logs` |
| `test-discovery` | `test_inventory`, in coverage mode |

## Example

```sh
struo query example turbo_test turbo-test
struo test hello__test
```
