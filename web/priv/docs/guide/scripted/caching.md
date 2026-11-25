# Caching Scripted Workflows

Control what invalidates a cached script: declare inputs, environment variables,
tool fingerprints, dependent scripts, and outputs, then check repeated runs.

The [first scripted workflow](/guide/scripted/) declared an input, an output,
and a working directory. Add more headers only when another fact can change the
result.

## Describe every relevant input

Track files and directories with `input`. Track selected environment variables
with `env`, and use `fingerprint` for tool or host facts that are not files:

```sh
#!/usr/bin/env -S struo exec -- bash
# struo input "../src/**/*.ts"
# struo input "../package-lock.json"
# struo output "../dist/"
# struo env "NODE_ENV"
# struo fingerprint "node --version"
# struo cwd ".."

npm run build
```

Here the source files, lockfile, `NODE_ENV` value, Node.js version, and script
file all contribute to the cache key. The script uses the
[npm package manager](https://www.npmjs.com/) to run the build. Struo forwards
only declared environment variables. A fingerprint command runs before the
action; if it fails, Struo stops before running the script.

::: tip Keep the contract precise
Declare facts that can change the result, not every file or host value the
script can see. A precise contract produces useful cache hits while still
invalidating the result when it should.
:::

## Connect dependent scripts

Use `needs` when one annotated script must run before another:

```sh
#!/usr/bin/env -S struo exec -- bash
# struo needs "./generate-assets.sh"
# struo input "../src/**/*.ts"
# struo output "../dist/"
# struo cwd ".."

npm run build
```

Struo runs independent dependencies concurrently. A dependency's declared
outputs become declared inputs of the dependent script, and their fingerprints
become part of its cache key. This lets a remote test consume an installation
script's `node_modules` output without exposing the rest of the host workspace.
If the dependency runs again but produces the same outputs, the dependent
script can still reuse its previous result.

## Choose how symbolic links are stored

Directory outputs use `materialize-external` by default. When an output contains
a symbolic link whose target is outside that output directory, Struo copies the
target into the cached result. This makes restored directories such as
`node_modules/` independent of a package manager's original storage location.

Use `output-symlinks` only when the script needs a different policy:

```sh
# struo output "../dist/"
# struo output-symlinks "preserve"
```

`preserve` stores links as links. Use it only when the link targets will exist
at restore time.

## Use another script runtime

The same headers work in languages that use line comments. Put the interpreter
after `struo exec --` in the shebang:

::: code-group
```python [Python]
#!/usr/bin/env -S struo exec -- python3
# struo input "../src/**/*.py"
# struo output "../dist/"

print("build")
```

```ruby [Ruby]
#!/usr/bin/env -S struo exec -- ruby
# struo input "../lib/**/*.rb"
# struo output "../dist/"

puts "build"
```

```elixir [Elixir]
#!/usr/bin/env -S struo exec -- elixir
# struo input "../lib/**/*.ex"
# struo output "../dist/"

IO.puts("build")
```
:::

Struo also recognizes `//`, `;`, `--`, `%`, and `'` line-comment forms. To keep
an existing shebang, invoke the interpreter explicitly:

```sh
struo exec -- python3 scripts/build.py
```

Struo detects the headers and applies script-aware execution. Pass `--script`
when you need to force that mode.

## Use a mise file task

Struo can wrap a [Mise file task](https://mise.jdx.dev/tasks/file-tasks.html)
without changing how callers invoke it:

```sh
#!/usr/bin/env -S struo exec -- bash
#MISE description="Build assets"
# struo input "../src/**/*.ts"
# struo output "../dist/"

npm run build
```

After saving the task in a Mise task directory, run it normally:

```sh
mise run build
```

## Supported headers

| Header | Purpose |
| --- | --- |
| `input` | Tracks files, directories, or globs. |
| `needs` | Runs another annotated script first and includes its output fingerprints in this script's cache key. |
| `fingerprint` | Runs a read-only command and includes its text, exit status, standard output, and standard error in the cache key. |
| `output` | Declares files or directories to restore on cache hits. |
| `output-symlinks` | Uses `materialize-external` or `preserve` when capturing symbolic links in outputs. |
| `env` | Forwards a selected environment variable and includes its value in the cache key. |
| `cwd` | Chooses the working directory for the script body. |
| `remote` | Selects a named remote execution infrastructure provider. |

The script file itself is always part of the cache key, even when it has no
`input` headers.

## Check repeated execution

A second cache hit checks cache reuse, not whether executing the command again
produces the same result. To compare fresh executions of the site packaging script
from [the first scripted workflow](/guide/scripted/), run:

```sh
struo exec --verify-reproducible --script -- bash scripts/package-site.sh
```

Struo snapshots the whole workspace, then runs two trials from that same
snapshot at the same private workspace path. Each trial has its own initially
empty local Struo cache. Script dependencies declared with `needs` and commands
declared with `fingerprint` run inside each trial, rather than reusing work
performed before the check. Declared outputs stay in the private trials;
Struo does not replace outputs in your original workspace with trial artifacts.

The snapshot excludes `.struo` runtime state and known declared outputs of the
script and its dependencies; other pre-existing generated files remain. This is
not a clean checkout or a way to discover every undeclared input. Symlinks
pointing outside the workspace and special files prevent the snapshot from being
made; materialize required files inside the workspace or remove those entries
before retrying. A script cannot declare the
whole workspace as an output for this check; declare its generated paths instead.

### Read the result

Both trials must execute successfully. Two identical failures never pass.
Struo reports artifact equality separately from captured standard output and
standard error:

- The comparison covers the selected script's declared result, not artifacts
  and logs produced only by prerequisites. Prerequisites still run in each trial.
- When outputs are declared, artifact differences fail the check. Log-only
  differences are advisory and do not fail a matching-artifact check.
- Without declared artifacts, the check uses captured logs instead. Matching
  logs do not establish that undeclared files match.

The command exits successfully only when those conditions hold. A passing
check means **no artifact divergence observed in these two trials**, or matching
captured logs when no artifacts are declared. It is not proof of reproducibility.

### Inspect retained trials

The result prints a verification UUID and a command to inspect it again:

```sh
struo query verification <ID>
struo query verification <ID> --format json
```

These queries read the retained report without executing the script again.
The report is stored at `.struo/verifications/<ID>/report.json`. Each
`trial-1` and `trial-2` directory retains `result.json`, decoded declared
artifacts under `outputs/`, captured `stdout` and `stderr` when present, and
its local Struo cache. Failed trials also retain their workspace for debugging.
If the listed artifact differences are truncated, inspect the retained
artifacts directly. The
[verification report reference](/reference/mcp/#verification-reports) describes
the JSON fields used by command-line tools and coding harnesses.

Reports, artifacts, and logs are local runtime state, not automatically
published evidence. They can contain sensitive source or output; inspect them
before sharing and remove `.struo/verifications/<ID>/` when no longer needed.

### Limitations of the check

Only run a repeated-trial check when you authorize repeated execution of the
script and all its dependencies. Host toolchains, host tool caches, absolute
paths, network services, and external side effects are not isolated. Commands
using those resources can still change host or external state. Struo does not
perturb or control randomness, clocks, or thread scheduling, and both trials
use the same private path, so path-sensitive differences can remain hidden.
Script verification currently requires local execution, including every
`needs` dependency; scripts selecting remote execution are rejected.

To deny Internet sockets, including loopback, add `--network deny` to
`struo exec`. This policy also applies to `needs` dependencies and refuses
execution when enforcement is unavailable. Local Unix-domain communication
remains available. Fingerprint commands cannot currently enforce this policy,
so a script with a fingerprint fails closed under `--network deny`, rather
than executing the probe online. Replace the fingerprint with declared inputs
before running offline.

## Continue during a cache outage

If a remote cache remains unavailable after bounded retries, Struo executes the
script instead of reusing a cached result. New results stay in the local cache,
so later runs can reuse them while the remote cache is down. The outage is
logged; it is not treated as proof that remote content is absent.

Cache reuse and restoration still fail on rejected credentials, corrupt content,
malformed cache records, and local filesystem errors. Explicit cache reads,
including SDK reads, report availability errors rather than returning missing
data.

## Keep the local cache bounded

The local cache is shared by every workspace on the machine, and Struo keeps it
within a size cap of 20 GB without any action from you. At most once an hour,
a command that runs actions, such as `struo build`, `struo test`, or
`struo exec`, starts a cleanup in the background and exits without waiting for
it. When the cache is over the cap, the cleanup removes the least recently used
results until the cache is back under 80% of the cap. The first check runs an
hour after Struo first uses the cache on a machine.

Struo counts a result as used whenever a build relies on it, including builds
that skip their actions entirely because nothing changed, so results you reuse
every day stay cached. Nothing used within the last hour is removed, so builds
running at the same time keep their work. A removed result runs again the next
time it is needed.

Set the cap for a workspace in its root `struo.toml`:

```toml
[local_cache]
max_size = "50GB"
```

The value is a byte count or a size such as `"50GB"` or `"8GiB"`, and `0`
turns automatic cleanup off. A value that is not a size fails every command
that loads the manifest. The `STRUO_CACHE_MAX_SIZE` environment variable takes
the same values and overrides the workspace setting, which suits a cap for
the whole machine; when it is not a size, commands still run and warn that
they skipped the cleanup. Because the cache is shared, a cleanup applies the
cap of the workspace whose command started it.

Run `struo cache stats` to see usage against the cap, and
`struo cache gc --max-size <size>` to reclaim space right away; it removes
recently used results too. Without `--max-size`, `struo cache gc` uses the
cap, or 20 GB when automatic cleanup is off. Cleanup affects only the local
cache; remote caches keep their own retention.

## Next steps

- [Infrastructure](/guide/infrastructure/): share these cached results across
  machines.
- [Remote Execution](/guide/infrastructure/remote-execution): run scripts in a
  fresh sandbox.
- [Cache CLI](/guide/scripted/runtime): let an existing tool probe or
  populate the cache itself.
