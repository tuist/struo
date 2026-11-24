# Runloop Devboxes

**Capabilities:** remote execution. Bind a separate
[cache provider](/guide/infrastructure) for shared caching.

Struo creates a fresh [Runloop Devbox](https://docs.runloop.ai/docs/devboxes/overview)
for each uncached remote action. It uploads declared inputs, runs the command,
retrieves declared outputs after an accepted exit code, and shuts down the Devbox.

## Prerequisites

Use a Runloop account with Devbox creation, listing, execution, file access and
shutdown permissions. Prepare a Blueprint containing your test runtime, Python 3
with isolated-mode support and its standard library, `sh`, `env`, `rm`, `mkdir`,
and `tar` supporting `--hard-dereference`. Make `python3` available on the guest's
default `PATH`. Keep credentials
out of the Blueprint. The default user needs a writable `/home/user`.

## Configuration

Use a prepared Blueprint ID rather than a mutable Blueprint name:

```toml
[infrastructures.remote_tests]
kind = "runloop"
blueprint = "bpt_your_node_blueprint"

[infrastructure.execution]
provider = "remote_tests"
```

The built-in `--compute runloop` selection uses Runloop's mutable default image.
Prefer a named infrastructure with a Blueprint ID for cacheable project workflows.

Set `RUNLOOP_API_KEY` outside your scripts. `STRUO_RUNLOOP_API_KEY` takes
precedence. Struo uses the key for API requests and does not inject it into
the command environment. `STRUO_RUNLOOP_URL` overrides the API base, which
defaults to `https://api.runloop.ai/v1`; HTTPS is required except for loopback tests.

## Usage

Select the configured provider from an annotated script:

```sh
#!/bin/sh
# struo remote "remote_tests"
# struo input "package.json"
# struo input "package-lock.json"
# struo input "node_modules/**"
# struo input "src/**"
# struo input "tests/**"
# struo output "reports/"

mkdir -p reports
exec node --test --test-reporter=junit --test-reporter-destination=reports/tests.xml
```

Save it as `test.sh` at the workspace root and run `struo exec ./test.sh`, or use
`struo exec --remote --compute remote_tests -- node --version` for a literal command.

Declared inputs include ignored directories such as local `node_modules`.
Struo preserves executable modes and safe symlinks. Native dependencies must match the Devbox's Linux architecture
and runtime. Undeclared files do not transfer. See
[Remote Execution](/guide/infrastructure/remote-execution) for dependency-producing
actions and cache behavior.

Struo applies the declared working directory and selected environment values.
It preserves binary stdout and stderr within a combined capture limit. Terminal output appears
after completion rather than streaming live.

## Limits and cleanup

Struo waits for `running` before transferring inputs. One action deadline covers
preparation, provisioning, transfers, execution and output installation. It defaults
to ten minutes without an explicit timeout. The Devbox keep-alive is the action
timeout plus two minutes, within Runloop's 48-hour limit.

Resource declarations control Struo's remote admission budget, not the Devbox
machine size. Runloop uses the Blueprint's launch defaults. Choose a Blueprint
with enough memory and disk for the action and its dependencies. Network denial,
explicit account/project scoping, output-path redirection and explicit filesystem
setup are unsupported and fail rather than run with weaker guarantees. Command
output-idle timeouts are unsupported because Struo downloads captures after execution.

Struo records ownership before provisioning. Failed shutdowns and uncertain
create responses retain recovery records. Keep the workspace's `.struo` runtime
state and the original account access available for later cleanup. Incomplete
listings do not establish that an uncertain creation produced no Devbox.

After success, failure, timeout or cancellation, Struo requests forced shutdown
and waits for `shutdown`. The admission lease remains held through the bounded
cleanup attempt. Runloop documents shutdown as ending active compute; this does
not promise erasure of retained records or snapshots. Struo does not create
snapshots or suspend Devboxes. The finite keep-alive supplements explicit cleanup
if the client disappears.

Local protocol tests cover this adapter. Live account access, execution and
teardown remain unverified.
