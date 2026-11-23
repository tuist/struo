# Tensorlake

**Capabilities:** remote execution. Tensorlake does not provide a Struo shared
cache. Bind a [cache provider](/guide/infrastructure) separately.

[Tensorlake](https://docs.tensorlake.ai/sandboxes/lifecycle) provides hosted Linux
sandboxes. Struo creates a named sandbox for an uncached action, stages declared
inputs, runs the command, retrieves declared outputs after an accepted exit,
and terminates the sandbox.

## Prerequisites

- A Tensorlake project API key with sandbox creation, process execution, file
  access, and termination permissions. Set `TENSORLAKE_API_KEY` in your
  environment; `STRUO_TENSORLAKE_API_KEY` takes precedence.
- Your project's namespace identifier. Struo checks that returned sandboxes
  belong to this namespace; it does not switch projects on your behalf.
- A prepared, registered Tensorlake Linux image or snapshot containing your runtime and `sh`, `env`,
  `rm`, `mkdir`, and GNU-compatible `tar`. The guest user must be able to write
  `/tmp` and create `/home/tl-user/struo-workspace`. Tensorlake's default
  `tl-user` may not write `/workspace`, so Struo does not use that directory.

Prepare the environment without embedded credentials or background workloads.
Import or register an OCI image with Tensorlake before using it here; an
arbitrary OCI reference is not a registered Tensorlake image name.
Struo does not inject project secrets, attach persistent filesystems, or expose
application ports. A snapshot can still contain secrets or running processes
that you placed in it.

## Configuration

Add a named infrastructure to `struo.toml`:

```toml
[infrastructures.remote_tests]
kind = "tensorlake"
image = "node-tests-v1"
namespace = "your-project-namespace"

[infrastructure.execution]
provider = "remote_tests"
```

To restore a snapshot, replace `image` with `snapshot = "your-snapshot-id"`.
Supply exactly one. Keep the reference unchanged only while its contents remain
unchanged; replacing an image under the same name does not change Struo's
action identity.

Struo uses `https://api.tensorlake.ai` for the control API. Compatible deployments
can set `STRUO_TENSORLAKE_API_URL`. Guest access requires HTTPS on Tensorlake's
`*.sandbox.tensorlake.ai` hosts by default. If your deployment returns another
trusted guest origin, set `STRUO_TENSORLAKE_SANDBOX_ORIGIN` to its exact HTTPS
origin. Struo refuses other guest origins and credential-bearing redirects.
Use the original project key and endpoint when recovering unfinished actions.

## Usage

Select the named environment:

```sh
struo exec --remote --compute remote_tests -- node --version
```

For a Vitest script saved as `scripts/test.sh`:

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

Run `struo exec --script -- sh scripts/test.sh`. Declare additional source,
configuration, and setup files your tests read. Struo includes declared
`node_modules` even when Git ignores it, preserving executable modes and safe
symbolic links. Native dependencies must match the guest's Linux architecture
and runtime. A dependency-install action can also produce `node_modules`; see
[Remote Execution](/guide/infrastructure/remote-execution).

## Limits and cleanup

Struo supports ordinary named creation, not warm-pool claims. Capacity waits can
take minutes. One action deadline covers preparation, creation, readiness,
transfers, execution, and output installation; it defaults to ten minutes.
Struo allows up to thirty minutes of pending time at the provider, but the action
deadline still applies. Resource declarations budget Struo's coordinator work;
they do not resize the Tensorlake sandbox. Prepare an environment with suitable
resources.

Struo preserves binary stdout and stderr, subject to its combined capture
limit. Terminal output appears after the command completes. Selected environment
values reach the final executable, including `IFS` and `PWD`. The guest retains
the prepared environment's other defaults. Struo bounds file transfers and
validates retrieved output archives before installation.

Network denial, explicit account/project binding fields, output-path redirection,
command filesystem setup, and command-output idle timeouts are unsupported and
fail closed. The `namespace` setting verifies ownership within the API key's
project. It does not grant access to another project.

Struo requests authenticated guest access, no application ports, and disabled
automatic resume. It terminates the whole sandbox on completion, failure,
timeout, or cancellation rather than relying on process-tree signals. Cleanup
has its own bounded retry budget and retains execution admission until that
attempt settles. Failed cleanup stays journaled for a later run.

After a lost creation response, names alone cannot establish ownership. If
Struo has not recorded the returned sandbox ID, it keeps the intent unresolved
for manual investigation instead of adopting or deleting a name match. Such
records occupy journal capacity and can block further provisioning. Retain
the workspace's `.struo` state and inspect your Tensorlake account before
manually removing an unresolved record.

Tensorlake's one-hour idle timeout suspends named sandboxes; it is not a
wall-clock execution limit or cleanup guarantee. Explicit termination stops
the sandbox, but Tensorlake retains restartability for 48 hours. Struo does
not claim immediate data erasure. See Tensorlake's
[lifecycle documentation](https://docs.tensorlake.ai/sandboxes/lifecycle).

Transport and failure tests cover this adapter. Live Tensorlake provisioning,
Vitest execution, and account-specific retention behavior remain unverified.
