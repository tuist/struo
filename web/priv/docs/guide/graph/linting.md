# Linting and Static Analysis

Run Ruff, ESLint, golangci-lint, SwiftLint, detekt, Credo, and RuboCop through
Struo with cached reports, normalized findings, and an explicit failure policy.

Struo records the analyzer version, sources, configuration, command, and
report as one cached action, so unchanged code is not analyzed twice and a
cache hit reports the same findings as a fresh run. Native analyzer
configuration stays authoritative.

## Run a complete example

Start in a repository where Struo is installed. Pin Ruff so the analyzer
version is part of the repository toolchain:

```sh
mise use ruff@latest
```

Materialize the bundled Python starter under `quality/python`:

```sh
struo edit materialize-example ruff_lint ruff-lint-minimal \
  --destination quality/python
```

The starter creates `quality/python/struo.toml` and a small Python source. Its
manifest location and target name produce the target identifier
`quality/python/lint`.

Inspect the target before executing it:

```sh
struo query capabilities quality/python/lint
struo query schema ruff_lint
```

Run the analyzer:

```sh
struo lint quality/python/lint
```

A clean starter prints:

```text
struo: lint quality/python/lint complete, 0 errors, 0 warnings, 0 notes
```

Run the same command again. Struo restores the analyzer report from the action
cache when its declared inputs and analyzer identity have not changed. Inspect
the latest records to see the first cache miss and following cache hit:

```sh
struo query evidence quality/python/lint --limit 2
```

## Add linting to existing sources

A lint target belongs in the package-level `struo.toml` beside the sources it
analyzes. For example, `python/struo.toml` can declare:

```toml
[[target]]
name = "lint"
kind = "ruff_lint"
srcs = ["src/**/*.py", "tests/**/*.py"]

[target.attrs]
config = ["pyproject.toml"]
args = ["--select", "E,F,I"]
```

Run it with the identifier formed from the manifest directory and target
name:

```sh
struo lint python/lint
```

Configuration and data paths are declared action inputs. A source,
configuration, analyzer version, or target-kind implementation change causes
fresh analysis. Unchanged inputs can reuse the report locally or through a
shared cache.

## Choose a built-in analyzer

| Ecosystem | Target kind | Analyzer |
| --- | --- | --- |
| Python | [`ruff_lint`](/reference/prelude/ruff_lint) | [Ruff](https://docs.astral.sh/ruff/linter/) |
| JavaScript and TypeScript | [`eslint_lint`](/reference/prelude/eslint_lint) | [ESLint](https://eslint.org/docs/latest/use/command-line-interface) |
| Go | [`golangci_lint`](/reference/prelude/golangci_lint) | [golangci-lint](https://golangci-lint.run/docs/) |
| Swift | [`swiftlint_lint`](/reference/prelude/swiftlint_lint) | [SwiftLint](https://github.com/realm/SwiftLint) |
| Kotlin | [`detekt_lint`](/reference/prelude/detekt_lint) | [detekt](https://detekt.dev/) |
| Elixir | [`credo_lint`](/reference/prelude/credo_lint) | [Credo](https://hexdocs.pm/credo/) |
| Ruby | [`rubocop_lint`](/reference/prelude/rubocop_lint) | [RuboCop](https://docs.rubocop.org/rubocop/) |

Each target-kind reference lists its attributes, default configuration inputs,
tool resolution behavior, and starter slug. Use the native analyzer
configuration rather than reproducing analyzer settings in Struo.

## Control the failure policy

The default policy fails when warnings or errors exist:

```sh
struo lint quality/python/lint
```

Change the threshold without changing the analyzer configuration:

```sh
struo lint quality/python/lint --fail-on error
struo lint quality/python/lint --fail-on note
```

`--fail-on error` reports warnings without failing. `--fail-on note` also
gates informational findings.

Many analyzers exit with a nonzero code when findings exist. Struo treats that
as a valid report. Unexpected codes, such as configuration errors or crashes,
fail the command instead of producing a lint result, so a broken analysis is
never mistaken for a clean one.

## Consume normalized findings

Analyzer output is stored as a
[Static Analysis Results Interchange Format report](https://docs.oasis-open.org/sarif/sarif/v2.1.0/sarif-v2.1.0.html).
Struo projects it into `struo.lint_results.v1`, which contains:

- a completion marker that distinguishes valid analysis from a broken tool;
- counts for errors, warnings, and notes;
- analyzer and rule identity;
- workspace-relative file, line, and column locations;
- optional help links and fingerprints;
- the portable report path.

Request the structured result:

```sh
struo --format json lint quality/python/lint
```

Coding agents can request the same result through the `struo_lint_target`
[Model Context Protocol](https://modelcontextprotocol.io/) tool when the
server starts with execution enabled:

```sh
struo mcp --allow-run
```


## Lint the whole workspace

Without a target, `struo lint` lints every workspace-owned target that exposes
`lint` and runs each one to completion. Add `--all` to include lint-capable
targets reached through vendored dependencies:

```sh
struo lint
struo lint --all --fail-on error
```

## Add another analyzer

When no built-in target kind covers a tool, define a project-local Starlark
module. [Custom lint target kinds](/reference/modules/linting) includes a
complete implementation, adapter patterns for portable and native reports, and
a runnable starter returned by `struo query module-contract`.

## Next steps

- [`struo lint` reference](/reference/cli/lint): see every command option.
- [Custom lint target kinds](/reference/modules/linting): wrap an analyzer
  Struo does not ship.
- [Testing and scheduling](/guide/graph/testing): run tests beside your
  analyzers.
- [Target kind index](/reference/prelude/): look up each analyzer kind's
  attributes and starter.
