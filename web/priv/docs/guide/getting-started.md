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

## Run a cacheable script

Suppose a project packages its static site into an archive before deploying
it. Create a directory with the site sources and a place for the script. You
can also follow these steps in an existing repository:

```sh
mkdir struo-demo
cd struo-demo
mkdir -p scripts site/css
printf '<h1>Welcome</h1>\n' > site/index.html
printf 'h1 { color: tomato; }\n' > site/css/main.css
```

Save the following as `scripts/package-site.sh`:

```sh
#!/usr/bin/env -S struo exec -- bash
# struo input "../site"
# struo output "../build/site.tar.gz"
# struo cwd ".."

set -eu
mkdir -p build
tar -czf build/site.tar.gz -C site .
tar -tzf build/site.tar.gz
```

Make the script executable and run it:

```sh
chmod +x scripts/package-site.sh
./scripts/package-site.sh
```

The script lists the files in the archive, and the first invocation ends with
a trailer containing `cache miss`. Run the same command again:

```sh
./scripts/package-site.sh
```

The second trailer contains `cache hit`. Struo reused the recorded result and
restored the declared output without running the script body again.

### Restore a deleted output

Check that Struo restores the output, not just that it remembers the command
succeeded:

```sh
rm -r build
./scripts/package-site.sh
tar -tzf build/site.tar.gz
```

You should see another cache hit and the restored archive with the site files.

### Change an input

```sh
printf '<h1>Welcome back</h1>\n' > site/index.html
./scripts/package-site.sh
```

Struo reports a miss and packages the site again. Unchanged inputs reuse the
result, and changed inputs run the work again.

## Understand the script contract

The three `# struo` lines form the action contract:

- `input` tells Struo which files affect the result. A directory covers every
  file inside it.
- `output` tells Struo what to capture and restore.
- `cwd` chooses the working directory for the script.

The script itself is also part of the cache key, so changing either the script
or any file under its declared input runs the work again. Paths in these
headers are relative to the script's directory, which is why `../site` names
the directory at the repository root.

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
