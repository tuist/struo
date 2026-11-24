# E2B Sandboxes

[E2B](https://e2b.dev/docs) provides hosted Linux sandboxes created from
templates. Struo runs each uncached action in a fresh E2B sandbox and deletes it
afterwards.

Struo transfers declared inputs, runs the command, retrieves declared outputs,
and deletes the sandbox.

**Capabilities:** remote execution. Configure shared caching separately.

## Prerequisites

- An E2B account and API key.
- An [E2B template](https://e2b.dev/docs/template) containing the stable tools
  shared by your actions. A Vitest template should provide Node.js, npm, and
  `tar`. Keep repository dependencies such as `node_modules` in the action
  graph instead of baking them into the template.

## Configuration

Add the template name or identifier to the repository root `struo.toml`:

```toml
[infrastructures.remote_tests]
kind = "e2b"
template = "vitest-node-22"

[infrastructure.execution]
provider = "remote_tests"
```

Pin the template contents so two executions with the same action identity see
the same tools.

### Authentication

Set the provider access key outside the script:

```sh
export E2B_API_KEY=e2b_...
```

`STRUO_E2B_API_KEY` takes precedence when both variables are present. Struo
does not forward the key to the action process.

Self-hosted or test endpoints can override the control and sandbox addresses
with `STRUO_E2B_API_URL` and `STRUO_E2B_SANDBOX_URL`.

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
Native dependencies must match the guest's Linux architecture and runtime. To
produce `node_modules` in a separate cacheable action, see
[Remote Execution](/guide/infrastructure/remote-execution).

## Limits and cleanup

- Sandboxes have outbound network access, so an installation action can
  download packages. Declare downloaded dependencies as outputs so later
  actions receive them explicitly.
- Each sandbox has a finite lifetime and is killed when it times out.

Struo sends an explicit delete request after success, command failure, timeout,
or transfer failure. The finite lifetime removes the sandbox if the client
disappears first. E2B explains the difference between killed and paused
sandboxes in its [persistence guide](https://e2b.dev/docs/sandbox/persistence).

## Next steps

- [Remote Execution](/guide/infrastructure/remote-execution): split dependency
  installation and tests into separate cacheable actions.
- [Infrastructure](/guide/infrastructure#available-providers): pick a cache
  provider so other machines reuse remote results.
- [Execution provider contract](/reference/infrastructure/execution): see what
  every execution provider guarantees.
