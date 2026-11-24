# Blaxel

**Capabilities:** remote execution. Bind a separate
[cache provider](/guide/infrastructure) for shared caching.

[Blaxel Sandboxes](https://docs.blaxel.ai/Sandboxes/Overview) run commands in
hosted Linux virtual machines. For each uncached action, Struo creates a fresh
sandbox, uploads declared inputs, executes the command, retrieves declared
outputs after an accepted exit code, and permanently deletes the sandbox.

## Prerequisites

Choose a supported Blaxel image or a custom template prepared with your test
runtime. Use an immutable image reference when reproducibility matters.

The image must provide `python3` with isolated mode (`-I`) and its standard
library on the default guest `PATH`, `sh`, `rm`, `mkdir`, `tar` supporting
`--hard-dereference`, a writable `/tmp` and `/workspace`, and your command's
runtime. Struo does not build or deploy custom templates. Your workspace needs
sandbox creation, execution, file access and deletion permissions, with process
keep-alive enabled.

## Configuration

```toml
[infrastructures.remote_tests]
kind = "blaxel"
image = "blaxel/base-image:latest"

[infrastructure.execution]
provider = "remote_tests"
```

The example uses a mutable tag for convenience. Replacing an image under the
same reference does not change Struo's action cache identity.

Set `BL_API_KEY` and `BL_WORKSPACE` outside the action environment.
`STRUO_BLAXEL_API_KEY` and `STRUO_BLAXEL_WORKSPACE` take precedence. Optional
`BL_REGION` selects the sandbox region. Struo resolves the workspace before
cache lookup and includes it in action identity, so identical image names in
different workspaces do not share action results. Account/project execution
scoping is not supported.

Alternatively, set `properties.workspace = "your-workspace"` in
`[infrastructure.execution]`. An explicit workspace takes precedence over the
environment. Runtime API callers must supply this resolved property themselves.

## Usage

```sh
struo exec --remote --compute remote_tests -- node --version
```

An annotated script can select the same binding:

```sh
#!/bin/sh
# struo remote "remote_tests"
# struo input "../node_modules"
# struo input "../tests"
# struo output "../reports"
# struo cwd ".."

./node_modules/.bin/vitest run --reporter=json --outputFile=reports/vitest.json
```

Local `node_modules` is uploaded when declared, including ignored files,
executable modes, and safe symlinks. Native dependencies must match the Linux
guest's architecture and runtime. See [Remote Execution](/guide/infrastructure/remote-execution)
for dependency installation as a separate cacheable action.

## Limits and cleanup

The sandbox must support multipart filesystem uploads and binary downloads.
Struo bounds transfers and validates output archives before installation.

Struo preserves binary stdout and stderr within a combined capture limit.
Output appears after completion rather than streaming live. Commands do not
automatically restart. Selected environment values are applied immediately
before the executable starts, after provider and shell setup.

Without an explicit
action timeout, Blaxel execution has a ten-minute action budget. A declared
timeout covers preparation, recovery, provisioning, transfers and execution.
Guest memory is at least the declared memory and 2048 MB per requested CPU
slot, following Blaxel's memory-based CPU allocation. Blaxel holds writable
filesystem data in guest RAM, so allow space for archives, dependencies, outputs
and the test process when declaring memory.

Network denial, output redirection, explicit command filesystem setup and live
output observers are rejected rather than silently ignored.

Struo records ownership before creation and does not repeat uncertain creates.
Keep the workspace's `.struo` runtime state, original workspace credentials and
endpoint available for recovery. Unresolved records remain until cleanup can
be established safely.

Cleanup sends an explicit permanent DELETE and waits for absence or
`TERMINATED`. Standby and archiving are not cleanup. Resource admission stays
held through bounded cleanup attempts, including cancellation. Failed deletes
remain journaled. A finite max-age TTL, at least five minutes and otherwise the
action budget plus two minutes, supplements explicit deletion if the client
disappears. No persistent volumes are attached.

For controlled testing, `STRUO_BLAXEL_CONTROL_URL` and `STRUO_BLAXEL_RUN_URL`
override the management and execution gateways. HTTPS is required except for
loopback development endpoints. Redirects are disabled and provider-returned
URLs are not used as credential destinations.

Local protocol tests cover this adapter. Live account access, execution and
teardown remain unverified.
