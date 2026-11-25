# LangSmith Sandboxes

[LangSmith Sandboxes](https://docs.langchain.com/langsmith/sandboxes) provide
hosted Linux execution. Struo runs each uncached action in a sandbox from a
prepared snapshot, then deletes it.

Struo stages declared inputs, runs your command, retrieves declared outputs
after an accepted exit, and deletes the sandbox.

## Capabilities

| Capability | Support |
| --- | --- |
| Shared cache | No; use a [cache provider](/guide/infrastructure/#available-providers) |
| Remote execution | Hosted sandboxes from a snapshot |
| Artifact publication | No |
| Project setup | Manual [configuration](#configuration) |
| Authentication | [API key and workspace ID](#authentication) |

Local protocol tests cover this provider; live execution, account availability
and teardown are unverified.

## Prerequisites

- A LangSmith workspace with Sandboxes enabled and a key with creation, listing,
  execution, file-transfer and deletion permissions.
- A prepared snapshot on the v2 sandbox runtime. Include Node and Vitest, or
  stage a Linux-compatible dependency tree with your inputs. The snapshot must
  provide `sh`, `rm`, `mkdir` and `tar`, and allow writes under `/tmp`.
- Your deployment's trusted sandbox data-plane HTTPS origin if it differs from
  the LangSmith API origin. Obtain this from your administrator.

Prepare snapshots without embedded credentials or sensitive files. Struo does
not grant the command access to its LangSmith key, attach volumes, or publish
service URLs. Snapshot environment defaults still apply to your command.

## Configuration

Bind the prepared snapshot in the repository root `struo.toml`:

```toml
[infrastructures.remote_tests]
kind = "langsmith"
snapshot = "node-tests:v1"

[infrastructure.execution]
provider = "remote_tests"
```

Use a new snapshot reference when you change its tools. Replacing a snapshot
under the same reference does not change Struo's action cache identity.

### Authentication

Set `LANGSMITH_API_KEY` and `LANGSMITH_WORKSPACE_ID`. The corresponding
`STRUO_LANGSMITH_API_KEY` and `STRUO_LANGSMITH_WORKSPACE_ID` variables take
precedence. Struo requires an explicit workspace UUID.

Set `STRUO_LANGSMITH_API_URL` for a regional, BYOC or self-hosted deployment;
the default is `https://api.smith.langchain.com`.

Set `STRUO_LANGSMITH_DATAPLANE_ORIGINS` to a comma-separated list of exact
trusted HTTPS origins, including a port if needed, without paths or wildcards.
Struo refuses to send credentials to an untrusted sandbox origin and does not
follow redirects. A deployment that serves sandbox operations from the API
origin needs no additional entry.

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
guest's Linux platform and Node version. To produce `node_modules` in a
separate cacheable action, see
[Remote Execution](/guide/infrastructure/remote-execution).

## Limits and cleanup

- Non-Enterprise LangSmith accounts use a managed egress allowlist. Check access
  to private registries and integration-test services before relying on them.
  Self-hosted installations need sandbox infrastructure and v2 runtime support.
- Stdout and stderr are preserved as binary and appear after the command
  completes, not live.
- The snapshot supplies the base environment and machine size; Struo's resource
  declarations do not resize the sandbox.
- One action deadline covers staging, creation, readiness, execution and output
  retrieval, with a ten-minute default when no timeout is declared.
- Network denial, explicit account/project bindings, output path redirection,
  graph filesystem setup and output-idle timeouts are unsupported and fail
  instead of running with weaker guarantees.

Struo deletes the whole sandbox on completion, failure or cancellation and
confirms it is gone. Disposable filesystem and idle-stop settings act as
fallbacks. Confirmed deletion does not guarantee physical-storage erasure.

Failed cleanup and lost creation responses stay recorded in the workspace's
`.struo` runtime state for a later run; Struo never repeats a create request.
Keep that state and the same workspace and key available. Relabeling a sandbox
or losing that state can require manual cleanup.

## Next steps

- [Remote Execution](/guide/infrastructure/remote-execution): split dependency
  installation and tests into separate cacheable actions.
- [Infrastructure](/guide/infrastructure#available-providers): pick a cache
  provider so other machines reuse remote results.
- [Execution provider contract](/reference/infrastructure/execution): see what
  every execution provider guarantees.
