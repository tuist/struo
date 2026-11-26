# Tuist

[Tuist](https://tuist.dev) gives Struo a shared cache, remote execution on Linux
workers, live run reporting, and artifact publication with application previews.

Configure each capability independently. For CI orchestration, see
[Triggers](/guide/infrastructure/triggers).

## Capabilities

| Capability | Support |
| --- | --- |
| Shared cache | Yes |
| Remote execution | Linux AMD64 workers, when enabled |
| Artifact publication | Yes, with application previews |
| Live run reporting | Yes |
| Test selection and sharding | [Selection](/guide/graph/testing#coverage-assisted-selection) and [sharding](/guide/graph/testing#distribute-tests-through-tuist) |
| Project setup | [`struo connect`](#create-and-connect-a-project) |
| Authentication | [`struo auth login` or `TUIST_TOKEN`](#authentication) |

## Prerequisites

Use a Tuist account and project. The credential needs access to the operations
you select; remote execution requires account execution capacity and read/write
access to the same project you use for caching. Workers run Linux on AMD64.

## Configuration

### Create and connect a project

Create a Tuist project and bind this repository to it in one command:

```sh
struo connect --provider tuist --create
```

The command signs you in when needed, creates a project named after your
workspace directory under your personal account, and writes the provider and
cache binding described in [Configure the cache](#configure-the-cache) into the
repository root `struo.toml`. Commit the file so teammates and CI use the same
project. It holds no credentials: each machine still
[authenticates](#authentication) before it can read or write the shared cache.

Pass the account and project explicitly when the workspace directory name is not
the project handle you want, or when the project should belong to an
organization:

```sh
struo connect --provider tuist --create --account acme --project app
```

Leave out `--create` to bind a project that already exists. With no account or
project, Struo lists the projects your session can see; it binds automatically
when there is exactly one and asks you to choose when there are several:

```sh
struo connect --provider tuist --account acme --project app
```

Add `--dry-run` to print the binding without creating a project, signing in, or
writing anything. A dry run with `--create` needs an explicit `--account`,
since the account is otherwise only known once the project exists. Pass
`--format json` for a machine-readable result. Re-running the same command is
safe: when the workspace already has a matching binding, Struo reports that it
is connected without rewriting the file or creating a second project. See the
[`struo connect` reference](/reference/cli/connect) for every option.

### Configure the cache

To bind the project by hand instead, add a Tuist provider and cache binding to
the repository root `struo.toml`:

```toml
[infrastructures.tuist]
kind = "tuist"
account = "acme"
project = "app"

[infrastructure.cache]
provider = "tuist"
```

Replace `acme` and `app` with the Tuist account and project that should own the
shared entries.

### Authentication

Sign in on a developer machine:

```sh
struo auth login --provider tuist
```

Struo reuses the stored session for cache reads, cache writes, live reporting
and artifact publication. Remove it when the machine should no longer access
the provider:

```sh
struo auth logout --provider tuist
```

### Authenticate automation

Choose the authentication method supported by the environment.

#### Account token

Set `TUIST_TOKEN` to a Tuist account token authorized for the project's operations
when the runner can store a long-lived secret:

```sh
TUIST_TOKEN=tuist_... ./scripts/package-site.sh
```

#### OpenID Connect

Use [OpenID Connect](https://openid.net/developers/how-connect-works/) on a
supported automation runner. Log in without opening a browser before running
cacheable commands:

```sh
struo auth login --provider tuist --no-browser
./scripts/package-site.sh
```

On GitHub Actions, grant identity-token permission before the login step:

```yaml
permissions:
  id-token: write
  contents: read

steps:
  - uses: actions/checkout@v6
  - run: struo auth login --provider tuist --no-browser
  - run: ./scripts/package-site.sh
```

The resulting session remains available for the rest of the job.

In an [Amp orb](https://ampcode.com/docs/orbs), Struo signs in with the orb's
identity token, so cacheable commands work without logging in first. Connect
the Amp workspace in your Tuist account's Integrations settings. When the
workspace is connected to several Tuist accounts, Struo authenticates as the
manifest's `account`.

## Usage

### Verify a shared result

Use two machines with the same `struo.toml`, script, and input. On the first
machine, run the [example scripted workflow](/guide/scripted/):

```sh
./scripts/package-site.sh
```

The first run reports a cache miss. On a second machine whose local cache has
not seen this result, check out the same `site/` sources and run the script
again:

```sh
./scripts/package-site.sh
```

The second machine should report a cache hit and restore
`build/site.tar.gz`. Because its local cache has no matching entry, the hit
confirms that Tuist supplied the recorded result.

### Run actions on Tuist workers

When remote execution is enabled for the account, Tuist runs graph command
actions on single-use Linux workers. Struo uploads only the input files the
project cache does not already hold and reads outputs back from the same cache,
so results stay available to later builds and other machines.

Select the same Tuist infrastructure for caching and execution:

```toml
[infrastructure.cache]
provider = "tuist"

[infrastructure.execution]
provider = "tuist"

[infrastructures.tuist]
kind = "tuist"
account = "acme"
project = "app"
```

```sh
struo build --remote your_target
```

Struo follows the worker image the service currently runs, so image updates do
not require workspace changes. Each image partitions cached results, so the
first build after an image change runs its actions again. To fail instead of
following an image change, pin it with `execution_image = "sha-..."` in the
infrastructure.

Struo fails before uploading anything when the account has no execution
capacity, when the cache and execution name different projects, or when a
pinned `execution_image` differs from the image the service advertises.

### Follow runs live

Struo streams build and test runs to Tuist when the provider is configured and
the machine is authenticated. Struo prints a dashboard link as the run starts.
Open it to follow the graph, cache decisions, and captured output while the run
progresses.

Live reporting is best-effort: a missing endpoint or an expired session is
logged and never fails the run. Re-authenticate with `struo auth login
--provider tuist` when the session expires.

### Publish application previews

Configure Tuist as the artifact publication provider in the repository root
`struo.toml`. If the project already defines this provider for caching, reuse
that definition and add only the artifact binding:

```toml
[infrastructures.tuist]
kind = "tuist"
account = "acme"
project = "application"

[infrastructure.artifacts]
name = "tuist"
```

```sh
struo auth login --provider tuist
struo artifact publish MyApplication --track review
```

Tuist receives the artifact and its dependencies and returns the project's
previews URL. Preview processing is asynchronous, so a publication receipt
does not mean an application is already available to install.

To publish after successful builds, add an explicit policy and authorize this
machine and workspace:

```toml
[infrastructure]
publish_on_build = ["MyApplication"]
```

```sh
struo artifact publish MyApplication --allow-on-build
struo build MyApplication
```

All requested builds must succeed, and the producer must approve the entire
artifact and its dependencies for automatic publication. Struo does not publish
without both the policy and local authorization. Changing the endpoint,
workspace or target requires new authorization; Struo rejects a changed project
destination before upload. See
[Artifacts](/guide/infrastructure/artifacts#publish-after-successful-builds)
for the provider-neutral policy and replay behavior.

#### Supported formats and retention

Tuist retains arbitrary artifact payloads without executing them. Metadata
documents are limited to one mebibyte and the full artifact with dependencies
to 20 GiB. Generic artifact contents remain private even in public projects.
Supported native previews use the project's default preview visibility:

- Ad-hoc signed macOS and simulator applications support automatic publication
  and previews. Physical-device applications do not.
- Debug-signed, non-instrumentation Android APKs support automatic publication.
  Unsigned APKs do not produce installable previews.

Other artifact formats remain available through authenticated downloads.
Publications expire after 30 days on hosted Tuist, or according to the
self-hosted app-preview retention setting. Repeating the same publication does
not reactivate an expired record. The returned URL does not guarantee permanent
file availability.

## Limits and cleanup

Struo respects the account's execution concurrency and waits when the worker queue is
full. An action can wait up to its timeout plus twenty minutes, including up to
fifteen minutes in Tuist's queue. Unsupported requirements, such as network
denial or a timeout longer than ten minutes, fail instead of running locally.

Workers run `linux/amd64`:

- Commands must resolve their tools inside the worker image, not on the
  computer that started the build. Struo does not wrap remote commands in the
  coordinator's mise installation.
- Workers look up a bare program name such as `sh` only through the action's
  `PATH`. When an action declares no `PATH`, Struo sends
  `/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin`.
- Declared outputs must stay inside the execution workspace, but may sit
  outside the command's working directory. Stdout and stderr can be redirected
  to declared output files. Failed commands return the output files they
  produced, including redirected logs.
- The service may run an action more than once, for example after a restart,
  so place only actions that are safe to repeat on it.

Struo validates returned paths and symbolic links before restoring outputs.
Links must not escape the workspace or expose undeclared coordinator files.

Keep `TUIST_CACHE_ENDPOINT` unset unless you need an override. If you set it,
choose an endpoint reachable by the coordinator that the workers also read from.

`TUIST_EXECUTION_ENDPOINT` likewise replaces the execution endpoint Tuist
advertises, for coordinators that reach the execution service from inside its
own network, such as Tuist runners. It changes only where Struo connects; the
credential and project stay the same.

### Cache failures

Struo retries transient cache failures up to three times. Interrupted uploads
restart rather than resume. Authentication, permission and quota refusals,
invalid requests, request timeouts and local file errors are not retried.

## Next steps

- [Remote Execution](/guide/infrastructure/remote-execution): declare the
  inputs and outputs remote actions need.
- [Artifacts](/guide/infrastructure/artifacts): publish build outputs with a
  provider-neutral policy.
- [Memory](/guide/memory/): inspect the status, cache state, and action identity
  Struo records for each run.
