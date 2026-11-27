# `go_workspace`

Native Go module seed.

## Description

`go_workspace` asks the Go command which packages a module contains and emits
ordinary Go targets for them. Every directory with a `go.mod` gets its own
seed, so a repository with several modules is discovered without
configuration. Like the Go command, discovery skips modules under directories
whose names begin with `_` or `.`, such as `_examples`. Go stays authoritative
for build constraints, imports, and module versions: the resolver runs
`go list -deps -test ./...` with the host's default build tags and cgo
setting, and `go mod graph` to find the module manifests the build reads.
A module or package Go cannot load fails discovery with Go's own error.

Each first-party package becomes a `go_library`, or a `go_binary` when it is a
`main` package. Each package with tests also becomes a `go_test` that compiles
its internal and external test files together, as `go test` does, and runs
from the package directory so tests find `testdata` and other relative paths.
Every external module that provides a package becomes a `go_module`:

- A vendored module is read from the `vendor` directory when
  `vendor/modules.txt` exists.
- A module replaced by a local directory, such as `replace example.com/m => ../`,
  is read from that directory, wherever it lies in the workspace.
- Any other module is snapshotted from the host's Go module cache into a
  Struo output. Builds read that snapshot and the module manifests and checksums
  the build needs, with `GOPROXY=off`, so they never fetch. The set of
  manifests comes from loading the module graph into an empty module cache, so
  it is the same on every host whatever else the host's cache holds.

On a fresh clone, the first graph load can access the network while the Go
command downloads modules into its cache.

All Go actions in a workspace share one Go build cache under `.struo`. Go's
cache is content-addressed and never changes what a build produces, so the
standard library compiles once instead of once per package. Go's cache does not
track C headers that cgo flags include from outside a package directory, such
as `-I${SRCDIR}/include`, so Struo declares those headers as inputs and adds
their digest to the cgo flags.

A test's run inputs include every file in its package directory, except
dotfiles, and under its `testdata` directory, since `go test` runs there and
tests read such files by relative path. Changing one reruns the test rather
than replaying a cached result.

## Test selection

A changed Go file, embedded file, or cgo header selects the tests of its
package and of every package that imports it, directly or through other
packages. `go.mod` and `go.sum` select every test in the module and in modules
that replace it with a local directory. A file under a package's `testdata`
selects that package's tests. Any other file selects a test only when the
test's Go files name it, such as `"expected.txt"` or
`path.Join(dir, "/../fixtures")`. A
changed README or documentation file selects nothing. Files under a nested
`go.mod` or `struo.toml` belong to that package and are selected by its own
targets.

## Attributes

| Attribute | Type | Required | Default | Description |
| --- | --- | --- | --- | --- |
| `go` | string | no | `go` on the search path | Go command used for discovery |
| `resolver_inputs` | list&lt;string&gt; | no | `srcs` | Text inputs available while deriving the graph |

## Providers

The target emits `go_workspace`.

## Capabilities

| Capability | Output groups |
| --- | --- |
| `build` | none |

## Direct Use

Inspect the automatically derived graph, then build and test it:

```sh
struo query targets
struo build go
struo test
```

No `struo.toml` is required. To configure the resolver explicitly, author a
target equivalent to:

```toml
[[target]]
name = "go"
kind = "go_workspace"
srcs = ["go.mod"]

[target.attrs]
resolver_inputs = ["go.mod", "go.sum", "vendor/modules.txt", "**/*.go"]
```

## Limitations

- Builds pass `-trimpath`, so compiled outputs do not contain the checkout
  path and action keys match across clones. Tests that expect absolute source
  paths from `runtime.Caller`, or `src/` in standard library frames, behave as
  they do under `go test -trimpath`.
- Tests run offline. A test that runs `go mod download` or reaches the
  network fails.
- Discovery uses the host's default build tags. Files behind other tags are
  not part of the graph.
- `go.work` files are ignored. Each module builds with `GOWORK=off`, reading
  its own `go.mod`; use local `replace` directives to build modules together.

## Sources

- [`go list`](https://pkg.go.dev/cmd/go#hdr-List_packages_or_modules) defines
  packages, files, imports, and providing modules.
- [`go mod graph`](https://pkg.go.dev/cmd/go#hdr-Print_module_requirement_graph)
  defines the module requirement graph.
