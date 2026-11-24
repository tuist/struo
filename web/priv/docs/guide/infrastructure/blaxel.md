# Blaxel Sandboxes

[Blaxel Sandboxes](https://docs.blaxel.ai/Sandboxes/Overview) are hosted Linux
virtual machines. Struo runs each uncached action in a fresh Blaxel sandbox and
deletes it afterwards.

Struo uploads declared inputs, executes the command, retrieves declared outputs
after an accepted exit code, and permanently deletes the sandbox.

**Capabilities:** remote execution. Configure shared caching separately. Local
protocol tests cover this provider; live account access, execution and teardown
are unverified.

## Prerequisites

- A Blaxel workspace with sandbox creation, execution, file access and deletion
  permissions, and process keep-alive enabled.
- A supported Blaxel image or a custom template prepared with your runtime.
  Struo does not build or deploy custom templates. The image must provide
  `python3` with isolated mode (`-I`) and its standard library on the default
  guest `PATH`, `sh`, `rm`, `mkdir`, `tar` supporting `--hard-dereference`, and
  a writable `/tmp` and `/workspace`.

## Configuration

Bind the image in the repository root `struo.toml`:

```toml
[infrastructures.remote_tests]
kind = "blaxel"
image = "blaxel/base-image:latest"

[infrastructure.execution]
provider = "remote_tests"
```

The example uses a mutable tag for convenience. Use an immutable image reference
when reproducibility matters; replacing an image under the same reference does
not change Struo's action cache identity.

### Authentication

Set `BL_API_KEY` and `BL_WORKSPACE` outside the action environment.
`STRUO_BLAXEL_API_KEY` and `STRUO_BLAXEL_WORKSPACE` take precedence. Optional
`BL_REGION` selects the sandbox region.

To pin the workspace in the manifest instead, set
`properties.workspace = "your-workspace"` in `[infrastructure.execution]`. It
takes precedence over the environment. The workspace is part of action
identity, so identical image names in different workspaces do not share
results.

For controlled testing, `STRUO_BLAXEL_CONTROL_URL` and `STRUO_BLAXEL_RUN_URL`
override the management and execution gateways. HTTPS is required except for
loopback development endpoints.

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
  appear after the command completes, not live.
- Without an explicit action timeout, an action has a ten-minute budget. A
  declared timeout covers preparation, provisioning, transfers and execution.
- Guest memory is at least the declared memory and 2048 MB per requested CPU.
  Blaxel holds writable filesystem data in guest RAM, so include archives,
  dependencies, outputs and the test process when declaring memory.
- Network denial, output redirection, explicit command filesystem setup and live
  output observers are rejected rather than silently ignored.

Struo permanently deletes the sandbox after the action, including after
cancellation. Standby and archiving are not cleanup. A max-age TTL, at least
five minutes and otherwise the action budget plus two minutes, removes the
sandbox if the client disappears. No persistent volumes are attached.

Failed deletes and uncertain creates stay recorded in the workspace's `.struo`
runtime state and are retried by a later run. Keep that state, the original
workspace credentials, and the endpoint available until cleanup completes.

## Next steps

- [Remote Execution](/guide/infrastructure/remote-execution): split dependency
  installation and tests into separate cacheable actions.
- [Infrastructure](/guide/infrastructure#available-providers): pick a cache
  provider so other machines reuse remote results.
- [Execution provider contract](/reference/infrastructure/execution): see what
  every execution provider guarantees.
