# Vercel Sandbox

[Vercel Sandbox](https://vercel.com/docs/sandbox) runs code in isolated Linux
microVMs. Struo runs each uncached action remotely in a fresh Vercel sandbox,
then deletes it.

Struo uploads declared inputs, runs the command, restores declared outputs
after an accepted exit code, and deletes the sandbox.

## Capabilities

| Capability | Support |
| --- | --- |
| Shared cache | No; use a [cache provider](/guide/infrastructure/#available-providers) |
| Remote execution | Hosted sandboxes on a Vercel runtime |
| Artifact publication | No |
| Project setup | Manual [configuration](#configuration) |
| Authentication | `VERCEL_TOKEN` |

## Prerequisites

- A Vercel account, team and project with Sandbox access.
- Dependencies prepared for the guest's Linux architecture and runtime. Do not
  upload macOS-built native modules.

## Configuration

Select a runtime and the team and project in the repository root `struo.toml`:

```toml
[infrastructures.remote_tests]
kind = "vercel"
runtime = "node24"

[infrastructure.execution]
provider = "remote_tests"
account = "team_your_team_id"
project = "prj_your_project_id"
```

`account` selects the Vercel team and `project` selects its project. Without
them in the binding, set `VERCEL_TEAM_ID` and `VERCEL_PROJECT_ID`; explicit
bindings take precedence. Struo rejects an unresolved scope before looking up
cached results.

Supported runtime values are `node22`, `node24`, `node26`, and `python3.13`.
Built-in `--compute vercel` uses `node24`. These names are provider-managed
references, not immutable image pins. Image-based environments are not
supported.

### Authentication

Set `VERCEL_TOKEN` to an access token for that team and project, outside
scripts and manifests. Struo does not forward it to commands. Automatic OIDC
discovery is not supported.

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
Struo includes declared `node_modules` even when Git ignores it, preserving
executable modes and safe symbolic links. To produce `node_modules` in a
separate cacheable action, see
[Remote Execution](/guide/infrastructure/remote-execution).

## Limits and cleanup

- Stdout and stderr are preserved separately as binary and appear after the
  command completes, not live.
- Commands use the selected environment plus the runtime's `PATH` unless you
  supply an explicit `PATH`.
- The action deadline covers preparation, provisioning, transfer, execution and
  output installation, ten minutes by default. The session lifetime adds two
  minutes and must fit Vercel's 24-hour maximum and your plan; Hobby sessions
  are limited to 45 minutes.
- Guest CPUs round up to one or an even number, with 2048 MB per CPU.
- Network denial, output-idle timeouts, explicit filesystem setup and output
  redirection are not supported and fail instead of running with weaker
  guarantees.

See [pricing and quotas](https://vercel.com/docs/sandbox/pricing) for current
account availability, limits and billing.

### Cleanup and recovery

Vercel sandboxes persist by default. Struo creates them non-persistent, never
creates snapshots or mounts persistent drives, deletes the named sandbox and
any orphan snapshots, and confirms the name is gone. Stopping a session alone
is not cleanup because the named configuration remains.

Struo checks its ownership tag before every deletion. Failed or inconclusive
cleanup stays recorded in the workspace's `.struo` runtime state for a later
run. The session timeout limits compute after a client crash, but the named
sandbox still needs that later cleanup.

## Next steps

- [Remote Execution](/guide/infrastructure/remote-execution): split dependency
  installation and tests into separate cacheable actions.
- [Infrastructure](/guide/infrastructure#available-providers): pick a cache
  provider so other machines reuse remote results.
- [Execution provider contract](/reference/infrastructure/execution): see what
  every execution provider guarantees.
