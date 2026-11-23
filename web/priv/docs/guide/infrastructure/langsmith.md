# LangSmith Sandboxes

**Capabilities:** remote execution. Bind a separate
[cache provider](/guide/infrastructure) for shared caching.

[LangSmith Sandboxes](https://docs.langchain.com/langsmith/sandboxes) provides
hosted Linux execution. Struo stages declared inputs, runs your command in a
prepared snapshot, retrieves declared outputs after an accepted exit, and deletes
the sandbox.

## Prerequisites

- A LangSmith workspace with Sandboxes enabled and a key with creation, listing,
  execution, file-transfer and deletion permissions.
- A prepared snapshot on the v2 sandbox runtime. Include Node and Vitest, or stage
  a Linux-compatible dependency tree with your inputs. The snapshot must provide
  `sh`, `rm`, `mkdir` and `tar`, and allow writes under `/tmp`.
- Your deployment's trusted sandbox data-plane HTTPS origin if it differs from
  the LangSmith API origin. Obtain this from your administrator before running.

Prepare snapshots without embedded credentials or sensitive files. Struo does
not grant the command access to its LangSmith key, attach volumes, or publish
service URLs. Snapshot environment defaults still apply to your command.

## Configuration

Declare a prepared snapshot in your workspace `struo.toml`:

```toml
[infrastructures.remote_tests]
kind = "langsmith"
snapshot = "node-tests:v1"

[infrastructure.execution]
provider = "remote_tests"
```

Use a new snapshot reference when you change its tools. Replacing a snapshot
under the same reference does not change Struo's action cache identity.

Set `LANGSMITH_API_KEY` and `LANGSMITH_WORKSPACE_ID`. The corresponding
`STRUO_LANGSMITH_API_KEY` and `STRUO_LANGSMITH_WORKSPACE_ID` variables take
precedence. Struo requires an explicit workspace UUID. Set
`STRUO_LANGSMITH_API_URL` for a regional, BYOC or self-hosted deployment; the
default is `https://api.smith.langchain.com`.

Set `STRUO_LANGSMITH_DATAPLANE_ORIGINS` to a comma-separated list of exact trusted
HTTPS origins, including a port if needed. Do not include paths or wildcards.
Struo refuses to send credentials to an untrusted sandbox origin and does not
follow redirects. A deployment that serves sandbox operations from the API
origin needs no additional entry.

## Usage

Select the configured environment:

```sh
struo exec --remote --compute remote_tests -- node --version
```

An annotated script can stage an existing dependency tree and retrieve a Vitest
report. Save this as `scripts/test.sh`:

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

Run `struo exec --script -- sh scripts/test.sh`. Declare any additional source,
configuration or fixture files your tests read. Struo includes declared ignored
directories such as `node_modules`, safe symbolic links and executable modes.
Native dependencies must match the guest's Linux platform and Node version.

## Limits and cleanup

Non-Enterprise LangSmith accounts use a managed egress allowlist. Check access
to private registries and integration-test services before relying on them.
Self-hosted installations need sandbox infrastructure and v2 runtime support.

Struo preserves binary stdout and stderr and displays them after command
completion. Your command receives the selected environment entries. The snapshot
supplies the base environment and machine size; Struo's resource declarations
budget coordinator work rather than resizing the sandbox.

One action deadline covers staging, creation, readiness, execution and output
retrieval, with a ten-minute default when no timeout is declared. Struo bounds
transfers and command capture. Network denial, explicit account/project
bindings, output path redirection, graph filesystem setup and output-idle
timeouts are unsupported and fail closed.

Struo requests a disposable filesystem, cold boot and idle-stop retention as
fallbacks. It deletes the whole sandbox on completion, failure or cancellation,
then confirms its UUID no longer appears in the creator-scoped listing. Closing
a command connection alone does not cancel the process. Cleanup has a separate
bounded attempt and retains unresolved ownership records for a later run.

A lost creation response has no documented visibility deadline or idempotency
guarantee. Struo does not repeat the create request or discard an uncertain
intent after an empty listing. Keep the workspace's `.struo` runtime state and
the same workspace/key access available for recovery. Relabeling a sandbox or
losing that state can require manual cleanup. Confirmed deletion does not
establish a physical-storage erasure guarantee.

Local protocol tests cover this adapter. Live LangSmith execution, account
availability and teardown remain unverified.
