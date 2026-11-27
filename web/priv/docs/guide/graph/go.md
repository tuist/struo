# Go Builds and Tests

Build, run, test, cross-compile, and cache Go packages with Struo, including
vendored modules, cgo, and exported C libraries declared in `struo.toml`.

Struo models Go packages, executables, and tests as typed targets and invokes
the Go command directly. Vendored modules become queryable graph targets, and
ordinary builds stay offline.

## Prerequisites

Install the pinned Go toolchain through mise:

```sh
mise install
mise exec -- go version
```

## Build an existing module without configuration

Struo recognizes every directory with a `go.mod` and derives its graph from
the Go command, so no `struo.toml` is needed:

```sh
struo query targets
struo build go
struo test
```

Each package becomes a library or command target, each package with tests a
test target, and each external module a module target read from `vendor`, a
local replacement, or a snapshot of the Go module cache. See the
[`go_workspace` reference](/reference/prelude/go_workspace) for how targets are
named, how changed files select tests, and the current limitations.

Declare targets by hand, as below, when a module needs settings that discovery
does not infer, such as cross-compilation or exported C libraries.

## Declare targets

### Declare a module

Create `struo.toml` beside `go.mod`:

```toml
[[target]]
name = "GoDependencies"
kind = "go_dependencies"
srcs = ["go.mod", "go.sum", "vendor/modules.txt"]

[target.attrs]
resolver_inputs = ["go.mod", "go.sum", "vendor/modules.txt"]

[[target]]
name = "Greeting"
kind = "go_library"
srcs = ["internal/greeting/*.go"]
deps = ["GoDependencies"]

[target.attrs]
package = "./internal/greeting"
importpath = "example.com/hello/internal/greeting"

[[target]]
name = "Hello"
kind = "go_binary"
srcs = ["cmd/hello/*.go"]
deps = ["Greeting", "GoDependencies"]

[target.attrs]
package = "./cmd/hello"

[[target]]
name = "GreetingTests"
kind = "go_test"
srcs = ["internal/greeting/*_test.go"]
deps = ["GoDependencies"]

[target.dependencies]
target_under_test = ["Greeting"]

[target.attrs]
package = "./internal/greeting"
coverage = true
labels = ["unit"]
```

`go_source` groups files that another target embeds into the same package;
connect it through the `embed` dependency role. Native dependencies used by
cgo belong under the `cdeps` role.

### Query before building

Inspect the graph before executing it:

```sh
struo query target-kinds
struo query schema go_binary
struo query targets --kind go_module
struo query capabilities Hello
```

### Build, run, and test

Use the shared capability commands:

```sh
struo build Hello
struo run Hello
struo test GreetingTests
```

The test target compiles one Go test binary and writes
[JavaScript Object Notation](https://www.json.org/json-en.html) events, a
readable log, and normalized test results. Exact test names from
`struo query test-manifest` can be scheduled independently. With `coverage`
set, a Go coverage profile is written alongside the results.

### Start from the runnable starter

The `go-comprehensive` starter includes a first-party package, an embedded
source group, a runnable executable, a Linux cross-compiled executable, exact
tests with coverage, and the vendored `github.com/pkg/errors` module:

```sh
struo query example go_binary go-comprehensive --format json
```

## Manage dependencies

### Import external modules

Go stays authoritative for version selection and source verification. Update
and vendor dependencies before an ordinary Struo build:

```sh
go mod tidy
go mod vendor
```

`go_dependencies` reads the requirement graph from a Go 1.17 or newer `go.mod`,
checks the selected versions against `go.sum` and `vendor/modules.txt`, and
emits one `go_module` target for every vendored module that contributes a
package. The committed vendored source is part of every consuming action's
cache key.

Build and test actions run with `-mod=vendor`, `GOPROXY=off`, and
`GOSUMDB=off`, so they cannot fetch missing source. A missing checksum, a
version disagreement, or a vendored module absent from the requirement graph
fails while the graph loads.

For a `go.work` workspace, set `workspace_file`, include that file and every
relevant checksum file in `sum_files` and `resolver_inputs`, and create the
vendor tree with `go work vendor`. Consumers set `go_work` to the same
workspace-relative file.

## Cross-compile and export native libraries

Set `goos` and `goarch` on a binary or library to select its destination:

```toml
[[target]]
name = "HelloLinux"
kind = "go_binary"
srcs = ["cmd/hello/*.go"]
deps = ["Greeting", "GoDependencies"]

[target.attrs]
package = "./cmd/hello"
goos = "linux"
goarch = "amd64"
strip = true
```

Build tags, compiler, assembler, and linker flags, the race detector,
sanitizers, profile-guided optimization, and link-time string definitions are
also typed attributes. See the [`go_binary` reference](/reference/prelude/go_binary).

Set `build_mode` to `c-archive` or `c-shared` to expose a Go `main` package to
native consumers. Struo returns the generated header and library as a C
provider, including the Android
[Application Binary Interface](https://developer.android.com/ndk/guides/abis)
record when `android_abi` is set. Other supported build modes are `exe`, `pie`,
`plugin`, `shared`, and `archive`.

## Limitations

- `struo run` and `struo test` require a binary for the host operating system
  and architecture. Cross-compiled executables can be built but not run.
- Cross-platform cgo requires destination-compatible `cc` and `c++` commands.
- Builds never fetch modules. Vendor dependencies with `go mod vendor` or
  `go work vendor` first.

## Target kinds

- [`go_workspace`](/reference/prelude/go_workspace)
- [`go_dependencies`](/reference/prelude/go_dependencies)
- [`go_module`](/reference/prelude/go_module)
- [`go_source`](/reference/prelude/go_source)
- [`go_library`](/reference/prelude/go_library)
- [`go_binary`](/reference/prelude/go_binary)
- [`go_test`](/reference/prelude/go_test)
- [`golangci_lint`](/reference/prelude/golangci_lint)

## Next steps

- [Linting](/guide/graph/linting): run golangci-lint as a cached graph target.
- [Testing and Scheduling](/guide/graph/testing): select and schedule exact Go
  tests across the graph.
- [Caching](/guide/scripted/caching): share build results across machines
  with a remote cache.
