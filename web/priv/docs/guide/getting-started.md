# Getting Started

Install Struo, cache your first script, restore its output from the local
cache, and try Struo on an existing Cargo, Swift, Xcode, or Bazel project.

No account or remote service is needed. Everything on this page runs against
the local cache.

## Installation

Install the current release with [mise](https://mise.jdx.dev/):

```sh
mise use -g "github:tuist/struo@$(mise latest github:tuist/struo)"
mise exec -- struo --version
```

Use `mise use github:tuist/struo@...` without `-g` when a repository should
pin its own Struo version in `mise.toml`.

The remaining examples assume that mise is active in your shell. If it is
not, prefix each `struo` command with `mise exec --`.

### Build from source

Building the CLI only needs the pinned Rust toolchain, not every ecosystem tool
used by Struo's development and test suite. Clone Struo, then run from the parent
directory, outside the checkout so mise does not load its full tool list:

```sh
git clone https://github.com/tuist/struo.git
mise exec rust@1.99.0 -- cargo build --release -p struo-cli --manifest-path "$PWD/struo/Cargo.toml"
./struo/target/release/struo --version
```

On macOS, install Xcode's command-line tools first. On Linux, install the native
development libraries `libcap-ng-dev` and `libasound2-dev`; the resulting binary
also needs `libasound2` at runtime.

## Run a cacheable script

Create an empty directory with a place for the script. You can also follow
these steps in an existing repository:

```sh
mkdir struo-demo
cd struo-demo
mkdir -p scripts
```

Save the following as `scripts/greet.sh`:

```sh
#!/usr/bin/env -S struo exec -- bash
# struo input "../message.txt"
# struo output "../build/greeting.txt"
# struo cwd ".."

set -eu
mkdir -p build
cp message.txt build/greeting.txt
cat build/greeting.txt
```

Create `message.txt`, make the script executable, and run it:

```sh
printf 'hello from Struo\n' > message.txt
chmod +x scripts/greet.sh
./scripts/greet.sh
cat build/greeting.txt
```

The first invocation ends with a trailer containing `cache miss`. Run the same
command again:

```sh
./scripts/greet.sh
```

The second trailer contains `cache hit`. Struo reused the recorded result and
restored the declared output without running the script body again.

### Restore a deleted output

Check that Struo restores the output, not just that it remembers the command
succeeded:

```sh
rm build/greeting.txt
./scripts/greet.sh
cat build/greeting.txt
```

You should see another cache hit and `hello from Struo` in the restored file.

### Change an input

```sh
printf 'hello again\n' > message.txt
./scripts/greet.sh
```

Struo reports a miss and writes `hello again`. Unchanged inputs reuse the
result, and changed inputs run the work again.

## Understand the script contract

The three `# struo` lines form the action contract:

- `input` tells Struo which files affect the result.
- `output` tells Struo what to capture and restore.
- `cwd` chooses the working directory for the script.

The script itself is also part of the cache key, so changing either the script
or its declared input runs the work again. Paths in these headers are relative
to the script's directory, which is why `../message.txt` names the file at the
repository root.

Correct reuse depends on an honest contract. Declare every file and environment
variable that affects the result, and every output you need restored. Struo
cannot infer a dependency that the script reads but does not declare. Do not
cache commands whose purpose is an external side effect, such as publishing a
release or sending a notification.

## Try an existing project

If you already have a working Cargo, Swift Package Manager, Xcode, or Bazel
project, try it before writing any Struo configuration. Keep its existing
toolchain installed: Struo reuses the native project metadata but does not
replace the compiler or SDK.

From the project root, list what Struo discovered, then build and test it:

```sh
cd path/to/project
struo query targets
struo build
struo test
```

Run `struo lint` when the discovered graph includes a lint target. A project
without one reports that no lint targets are available rather than silently
skipping analysis.

`struo build`, `struo test`, and `struo lint` default to the workspace's own
targets. No `struo.toml` is created. Pass `--all` to include targets reached
through resolved dependencies. `struo lint --all` also lints vendored
third-party code, so pair it with `--fail-on error` to keep the output
manageable. The [Typed Graph guide](/guide/graph/#start-from-a-native-project)
explains the default scope in detail.

Continue with the matching guide for [Rust](/guide/graph/rust),
[Swift Packages](/guide/graph/swift-packages),
[Xcode Projects](/guide/graph/apple/xcode), or [Bazel](/guide/graph/bazel) for
prerequisites, native dependency behavior, and limitations.

## Next steps

- [Scripted automation](/guide/scripted/): cache existing repository scripts
  with minimal changes.
- [Typed Graph guide](/guide/graph/): add typed targets, dependencies, and
  capabilities that Struo and coding agents can query.
- [Infrastructure](/guide/infrastructure/): share cache entries with your team
  or run actions on another machine.
- [Software development kits](/guide/sdk/): give application code direct
  access to Struo cache primitives.
