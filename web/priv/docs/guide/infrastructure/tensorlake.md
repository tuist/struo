# Tensorlake Sandboxes

[Tensorlake](https://docs.tensorlake.ai/sandboxes/lifecycle) provides hosted
Linux sandboxes. Struo runs each uncached action remotely in a named Tensorlake
sandbox, then terminates it.

Struo stages declared inputs, runs the command, retrieves declared outputs after
an accepted exit, and terminates the sandbox.

**Capabilities:** remote execution. Configure shared caching separately.
Transport and failure tests cover this provider; live provisioning and
account-specific retention behavior are unverified.

## Prerequisites

- A Tensorlake project API key with sandbox creation, process execution, file
  access, and termination permissions, and your project's namespace identifier.
- A registered Tensorlake Linux image or snapshot containing your runtime,
  Python 3 with isolated-mode support and its standard library (`python3` on
  the default `PATH`), `rm`, `mkdir`, and GNU-compatible `tar`. The guest user
  must be able to write `/tmp` and create `/home/tl-user/struo-workspace`.

Import or register an OCI image with Tensorlake before using it here; an
arbitrary OCI reference is not a registered image name. Prepare the environment
without embedded credentials or background workloads. Struo does not inject
project secrets, attach persistent filesystems, or expose application ports.

## Configuration

Add a named infrastructure to the repository root `struo.toml`:

```toml
[infrastructures.remote_tests]
kind = "tensorlake"
image = "node-tests-v1"
namespace = "your-project-namespace"

[infrastructure.execution]
provider = "remote_tests"
```

To restore a snapshot, replace `image` with `snapshot = "your-snapshot-id"`.
Supply exactly one. Replacing an image under the same name does not change
Struo's action identity.

`namespace` verifies that returned sandboxes belong to the API key's project.
It does not switch projects or grant access to another project.

### Authentication

Set `TENSORLAKE_API_KEY` in your environment. `STRUO_TENSORLAKE_API_KEY` takes
precedence.

Compatible deployments can override the `https://api.tensorlake.ai` control API
with `STRUO_TENSORLAKE_API_URL`. Guest access requires HTTPS on
`*.sandbox.tensorlake.ai` hosts by default; if your deployment returns another
trusted guest origin, set `STRUO_TENSORLAKE_SANDBOX_ORIGIN` to its exact HTTPS
origin. Struo refuses other guest origins and credential-bearing redirects.

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
executable modes and safe symbolic links. Native dependencies must match the
guest's Linux architecture and runtime. To produce `node_modules` in a separate
cacheable action, see [Remote Execution](/guide/infrastructure/remote-execution).

## Limits and cleanup

- Stdout and stderr are preserved as binary within a combined capture limit and
  appear after the command completes.
- One action deadline covers preparation, creation, readiness, transfers,
  execution, and output installation; it defaults to ten minutes. Capacity
  waits can take minutes. Warm-pool claims are not supported.
- Struo's resource declarations do not resize the sandbox. Prepare an
  environment with suitable resources.
- Network denial, explicit account/project binding fields, output path
  redirection, command filesystem setup, and output-idle timeouts are
  unsupported and fail instead of running with weaker guarantees.

Struo terminates the whole sandbox on completion, failure, timeout, or
cancellation. Tensorlake keeps a terminated sandbox restartable for 48 hours,
and its one-hour idle timeout only suspends sandboxes, so neither is a cleanup
guarantee. See Tensorlake's
[lifecycle documentation](https://docs.tensorlake.ai/sandboxes/lifecycle).

Failed cleanup stays recorded in the workspace's `.struo` runtime state for a
later run. After a lost creation response, Struo keeps the record unresolved
instead of deleting a sandbox by name; such records can block new sandboxes.
Inspect your Tensorlake account before removing them, and use the original
project key and endpoint for recovery.

## Next steps

- [Remote Execution](/guide/infrastructure/remote-execution): split dependency
  installation and tests into separate cacheable actions.
- [Infrastructure](/guide/infrastructure#available-providers): pick a cache
  provider so other machines reuse remote results.
- [Execution provider contract](/reference/infrastructure/execution): see what
  every execution provider guarantees.
