# Fly.io Sprites

[Sprites](https://fly.io/sprites) provides hosted Linux environments. Struo
creates a disposable Sprite for each uncached action by default, uploads
declared inputs, runs the command, retrieves declared outputs, and deletes the
Sprite. Opt in to a worker pool to retain prepared environments across runs.

## Capabilities

| Capability | Support |
| --- | --- |
| Shared cache | Optional retained cache Sprite |
| Remote execution | Disposable Fly.io Linux environments or a locked worker pool |
| Artifact publication | No |
| Project setup | Manual [configuration](#configuration) |
| Authentication | `SPRITES_TOKEN` |

## Prerequisites

- A Sprites account and token with creation, execution, file-transfer and
  deletion access.
- Tools your scripts need in Sprites' standard Linux environment. Struo does
  not accept custom images or templates, and that environment is not pinned to
  an immutable image, so verify tool versions before reusing cached results
  across environment updates. The guest must provide `tar` with
  `--hard-dereference` support, `mkdir`, and GNU `dd`.

## Configuration

Add a provider to the repository root `struo.toml`:

```toml
[infrastructures.remote_tests]
kind = "sprites"

[infrastructure.execution]
provider = "remote_tests"
```

You can also select the built-in name with `--compute sprites` without any
configuration.

### Authentication

Set `SPRITES_TOKEN` outside the script. `STRUO_SPRITES_TOKEN` takes precedence.
Struo does not forward it to the command unless you explicitly declare it in
the action environment.

`STRUO_SPRITES_API_URL` overrides `https://api.sprites.dev` for testing or a
compatible endpoint.

### Locked worker pool

Keep a bounded pool of prepared workers instead of reinstalling tools for each
action. Commit a root `mise.toml` and `mise.lock`, including Linux x64 entries,
then add:

```toml
[infrastructure.execution.properties.worker_pool]
max_workers = 10
max_age_seconds = 604800
setup = [["sudo", "apt-get", "update"], ["sudo", "apt-get", "install", "-y", "build-essential", "pkg-config"]]
```

The pool is lazy: a cache hit creates no worker. Struo snapshots the tool
configuration before cache lookup and fingerprints the file contents,
referenced supported lock sidecars, setup argument vectors, worker platform,
and its pinned mise version. Source edits do not change that environment key.
On a new worker, Struo installs checksum-verified mise, runs the setup commands
directly, runs `mise install --locked`, and saves a prepared checkpoint. Actions
use `mise exec --locked` with automatic tool installation disabled.

The initial contract supports root `[tools]` string requests, one locked
version per tool, and optional `settings.lockfile = true`. Supported uv
sidecars are copied with the lockfile. Local or environment-specific mise
configs, includes, hooks, tasks, env declarations, plugins, tool options,
unsupported native version files, and unsupported sidecars fail with a
diagnostic rather than silently escaping the fingerprint. Consolidate tools
into the root configuration and put OS preparation in `setup`. Preparation
commands run before tool installation and should not depend on a mise tool.
The combined configuration and setup limit is 1 MiB. Workers also need
`curl`, `sha256sum`, and `chmod`; setup commands must provide any additional
native prerequisites. The fingerprint identifies the declared preparation
recipe, not an attestation of every OS package. Pin package versions when
your setup requires that reproducibility.

Struo isolates mise configuration from the coordinator and staged project
configs. A native worker service retains immutable, digest-verified input
blobs and prepared tools. Each action receives copied inputs in a new workspace,
private scratch mounts, read-only tools, and isolated process and network
namespaces. Credentials and the worker's blob store are not mounted into
actions. Explicit action `HOME` and `TMPDIR` values are preserved; declare
their workspace directories when needed. Linux bubblewrap is required and
Struo refuses to fall back to unisolated execution.

Service startup uses passwordless `sudo` to prepare a private parent mount
namespace, then drops privileges before starting the worker runtime. This
allows private action process mounts without changing the Sprite's original
mounts. Tool installation data stays read-only; the tool environment reported
by mise is retained while each action receives fresh home and temporary state.

Successful actions remove their workspace before releasing the worker. They
do not restore the checkpoint or reinstall tools. Errors and cancellation
retain checkpoint recovery. Worker-local blobs are disposable acceleration
state, never undeclared inputs: missing blobs are transferred again and
verified. Reuse remains intended for trusted workspace builds, not as a
multi-tenant security boundary. A checkpoint does not pin the provider's
base image. Actions cannot access the network; fetch dependencies as declared
inputs before execution.

Workers are exclusively leased across CLI processes on the same computer.
The pool is scoped to the user's data directory, canonical workspace path,
API endpoint, and credential. It is not a fleet shared by separate computers.
A changed environment key replaces an idle worker within the same capacity
limit. Workers older than `max_age_seconds` are replaced on their next lease,
not by a background timer. The defaults are one worker and seven days.

The worker runs as a Sprites managed service. Struo closes action connections
without polling or sending keepalives between builds. Sprites controls idle
suspension and wakes the retained service for the next request. Suspension
with this service must be verified against the provider's current behavior.
Suspended workers and their checkpoints still incur storage
charges. `max_workers` caps retained execution workers across environment
generations; a retained cache is an additional Sprite. The normal CPU and
memory admission budgets may further reduce active concurrency.

### Cargo build and test graphs

With a locked worker pool, Struo distributes individual Cargo graph actions,
including crate compilation, build scripts, test compilation, and test
execution. Dependencies finish before their consumers start. When the selected
cache is Sprites or an HTTP cache, workers publish output files directly to
that shared cache and downstream workers fetch missing inputs by digest.
The coordinator still restores outputs for graph inspection and local use,
but does not relay those input bytes to each downstream worker.

Direct transfers use the existing cache binding; no extra configuration is
required. Cache-only credentials travel over the authenticated worker control
connection and stay in the trusted service, outside action environments and
mounts. Actions remain network-isolated. Read-only caches allow fetching but
disable worker publication. Missing, corrupt, evicted, or unavailable objects
fall back to coordinator transfers without replaying the action. Other cache
backends retain coordinator-mediated transfers.

Transfers stream within the worker's resource budget and retained CAS limit.
If the worker cannot admit additional transfer buffers, it uses coordinator
transfers instead of exceeding its budget. Real local worker/cache integration
is covered by automated checks; direct transfers over hosted Sprites ingress
have not yet been verified live.

Run the coordinator with the Rust version pinned in `mise.lock`. Struo resolves
the compiler inside each prepared worker and checks its executable digest
against the compiler used for graph analysis. A mismatch fails with guidance
to align the toolchains. Preparation must also supply compatible native
linkers, libraries, and tools required by your packages. Struo does not copy
the coordinator's sysroot or native dependencies into workers.

Discover your target names, then select a binary to build or schedule tests:

```sh
struo query targets
struo build YOUR_BINARY_TARGET --remote --compute remote_tests
struo test --all --jobs 2 --remote --compute remote_tests
```

You can also select one test target or use `test --plan PLAN.json`. Scheduled
test subprocesses keep the parent's resolved execution environment and
configuration. Cargo test distribution operates across whole test targets;
it does not split individual libtest cases into automatic shards.

`max_workers` bounds the shared worker pool. `--jobs` bounds concurrent test
batches, while CPU and memory budgets also constrain admission. Remote cache
hits can satisfy actions without leasing a worker.

### Retained cache

Bind the same provider to caching to let Struo create a separate cache Sprite
on the first remote cache read or write:

```toml
[infrastructures.remote_tests]
kind = "sprites"
cache_max_size_bytes = 21474836480

[infrastructure.execution]
provider = "remote_tests"

[infrastructure.cache]
provider = "remote_tests"
```

The cache needs `python3` in the standard guest. Struo installs a small HTTP
service, registers it to restart on cold wake, and reuses its persistent disk
across runs. The service wakes on HTTP requests and can sleep between runs.
Execution Sprites remain disposable unless worker reuse is enabled. Selecting
execution alone does not create a cache Sprite, and a local cache hit does not
provision or wake the cache.

`cache_max_size_bytes` defaults to 20 GiB and accepts 4 KiB through 80 GiB.
The cache evicts least-recently-accessed objects when it needs space. Object
sizes round up to 4 KiB, with a minimum of 4 KiB even for empty values. Uploads
count toward the limit before publication. Replacing an object needs room for both the old
value and its complete replacement; otherwise the write fails without
discarding the old value. Use the same capacity across workspaces sharing the
cache, since a change updates and restarts its service.

Set `read_only = true` to prevent publication of cache entries. As with other
remote caches, `STRUO_CACHE_READ_ONLY=1` also disables publication. Read-only
mode does not disable lazy provisioning or service maintenance.

The same named provider and cache binding work in your personal
`~/.config/struo/config.toml`, so repositories need not opt in individually.

### Scope and authentication

Struo keeps the retained Sprite's name, immutable ID, and a generated cache
credential under the user's credentials directory. Workspaces using that
directory, API endpoint, and Sprites API token reuse one cache. Different
users' credential directories get separate caches even if they use the same
Sprites organization. Another computer does not discover this cache
automatically. Changing API tokens or losing local credentials creates a
different cache; delete the old Sprite separately to stop its storage charges.

The cache URL uses public provider ingress, but every cache request requires
its generated bearer credential. The Sprites API token is used only by the
coordinator for provisioning and service maintenance, not for cache traffic.
The cache credential grants read and write access to this cache only. Keep
the credentials directory private and out of source control.

The service is reachable from other sandboxes that permit outbound HTTPS to
its URL. Locked execution workers use it directly for artifact transfers.
Cache credentials belong to the trusted worker service, never the command.
Disposable execution continues to transfer artifacts through the coordinator.

## Usage

Run a literal command on the provider:

```sh
struo exec --remote --compute remote_tests -- node --version
```

Omit `--compute remote_tests` to use the workspace's default execution provider.

### Run an annotated test script

Save this script as `scripts/test.sh`:

```sh
#!/bin/sh
# struo remote "remote_tests"
# struo cwd ".."
# struo input "../node_modules"
# struo input "../package.json"
# struo input "../tests"
# struo output "../reports"

node node_modules/vitest/vitest.mjs run \
  --reporter=json --outputFile=reports/vitest.json
```

Run it:

```sh
struo exec --script -- sh scripts/test.sh
```

Declare any additional source, configuration, or setup files your tests read.
Struo preserves executable modes and safe symbolic links. Install native
dependencies on the same Linux architecture as the Sprite rather than copying
them from macOS. To produce `node_modules` in a separate cacheable action, see
[Remote Execution](/guide/infrastructure/remote-execution).

## Limits and cleanup

### Execution Sprites

Struo preserves executable modes, safe symbolic links and separate binary
stdout and stderr. It validates outputs before installation. Interrupted
execution without an exit status fails the action. Transfers must make
progress within the configured stall timeout; quiet commands may remain silent.

Without worker reuse, Struo deletes the Sprite after success, command failure,
timeout, or transfer failure. Idle hibernation does not delete a Sprite or its storage.
Cancellation cleanup retains the admission lease until deletion finishes.

Sprites has no finite provider-side lifetime to back up Struo's cleanup. See
the [Sprites documentation](https://sprites.dev/) for account limits and
billing.

Failed deletions stay recorded in the workspace's `.struo` runtime state for a
later run, which deletes only a Sprite whose name and ID both match. Do not
replace a Struo-owned `struo-<nonce>` Sprite while cleanup runs. If a create
response is lost, Struo cannot prove ownership and will not delete the Sprite:
inspect such Sprites in your account and remove them manually, as you must if
the coordinator disappears permanently.

With worker reuse, lease records live under
`$XDG_DATA_HOME/struo/workers/sprites/` (normally
`~/.local/share/struo/workers/sprites/` on Linux). Keep this state private and
retain it across runs. A slot is marked dirty before work starts and becomes
available only after acknowledged action cleanup. Errors use checkpoint reset;
failed resets remain dirty for recovery
on the next lease. Cancellation keeps the worker lease and execution admission
through bounded reset; a process crash leaves durable recovery state.

Uncertain creates and failed deletions consume pool capacity rather than
allowing unbounded replacement. A reduced capacity fails if more slots already
exist, including empty slots. Resolve retained resources before lowering it.
To retire a pool, stop its builds, remove the worker-pool configuration, match
each recorded identity against your account, and destroy those Sprites. Remove
their local slot state only after confirming deletion. Losing state, switching
credentials, or moving a workspace does not delete its former workers.

### Cache Sprite

The retained cache is named `struo-cache-<identifier>`. It is deliberately
excluded from per-action cleanup and remains billable for stored data while
idle. Remove its cache binding before deleting it with
`sprite destroy <cache-sprite-name>` or through your Sprites account. Confirm
the exact name against your retained cache state, rather than deleting
execution Sprites by prefix. If the binding remains enabled, a later cache
operation recreates a deleted cache with empty storage.

Struo verifies the recorded name and immutable ID before maintaining an
existing cache. If creation succeeds but its response or local identity save
is lost, Struo refuses to adopt an unrecognized Sprite. Inspect it manually
before deleting it or resetting the corresponding local cache state. Do not
replace a retained Sprite with another machine of the same name while Struo
is using it.

The service streams bounded buffers, limits concurrent requests, and publishes
complete uploads atomically. Downloads retain Struo's digest verification and
transfer budgets. Large cache objects also depend on the provider proxy's
upload limits and idle timeouts. Live cache validation covers authenticated
access from another Sprite, 8 MiB transfers, empty-local-cache restoration,
service restart, and retrieval after idle. Full CLI remote execution is also
validated with declared binary inputs and outputs, separate stdout and stderr,
and a remote cache hit after clearing the local cache. The executor accepts
provider `debug` events as metadata without adding them to command output.
Cold VM wake and larger transfers remain unverified.

## Next steps

- [Remote Execution](/guide/infrastructure/remote-execution): split dependency
  installation and tests into separate cacheable actions.
- [Infrastructure](/guide/infrastructure#available-providers): pick a cache
  provider so other machines reuse remote results.
- [Execution provider contract](/reference/infrastructure/execution): see what
  every execution provider guarantees.
