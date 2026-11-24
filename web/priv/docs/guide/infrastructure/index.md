# Infrastructure for Shared Cache and Remote Execution

Struo caches and runs actions locally with no setup. Add infrastructure to share
cached results across machines, run actions remotely, or publish build artifacts.

Each capability is configured independently in the repository root
`struo.toml`. A repository can store results in one provider, run actions in
another, or use only one of them.

## Connect a project

The shortest path to a shared cache creates a project with a provider and binds
this repository to it:

```sh
struo connect --provider tuist --create
```

Struo signs you in when needed, creates the project, and records the binding in
the root `struo.toml`. Read [Connect a Project](/guide/infrastructure/connect)
for account selection, binding an existing project, and previewing the write.

## Share results across machines

A cache provider lets another developer, CI runner, or coding agent reuse an
action result instead of running it again:

```toml
[infrastructures.tuist]
kind = "tuist"
account = "acme"
project = "app"

[infrastructure.cache]
provider = "tuist"
```

To use storage you already run, configure a
[bucket](/guide/infrastructure/object-storage) or an
[HTTP build cache](/guide/infrastructure/http-cache) instead.

## Run actions on another machine

Name an execution provider and select it in the root `struo.toml`:

```toml
[infrastructures.remote_tests]
kind = "e2b"
template = "vitest-node-22"

[infrastructure.execution]
provider = "remote_tests"
```

Then run a command through it:

```sh
struo exec --remote -- node --version
```

Struo transfers only the declared inputs to a fresh environment, runs the
command, retrieves the declared outputs, and deletes the environment. Read
[Remote Execution](/guide/infrastructure/remote-execution) for scripts, build
graphs, and limits.

## Available providers

Each guide covers prerequisites, configuration, usage, and limits.

| Provider | Shared cache | Remote execution | Environment |
| --- | --- | --- | --- |
| [Tuist](/guide/infrastructure/tuist) | Yes | When enabled for your account | Single-use Linux workers; also artifacts and live reporting |
| [Blaxel](/guide/infrastructure/blaxel) | No | Yes | Hosted sandbox image |
| [Boat](/guide/infrastructure/boat) | No | Yes | Hosted saved snapshot |
| [Cloudflare](/guide/infrastructure/cloudflare) | No | Yes, through a Worker bridge you deploy | Containers in your Cloudflare account |
| [Daytona](/guide/infrastructure/daytona) | No | Yes | Hosted container image |
| [Docker](/guide/infrastructure/docker) | No | Local containers | Local immutable image |
| [E2B](/guide/infrastructure/e2b) | No | Yes | Hosted template |
| [HTTP cache](/guide/infrastructure/http-cache) | Yes | No | Any server with `GET`, `HEAD`, and `PUT` |
| [LangSmith](/guide/infrastructure/langsmith) | No | Yes | Hosted snapshot in your LangSmith workspace |
| [Microsandbox](/guide/infrastructure/microsandbox) | No | Local micro-VMs | Local image |
| [Northflank](/guide/infrastructure/northflank) | No | Yes | Service-backed sandbox image |
| [Object storage](/guide/infrastructure/object-storage) | Yes | No | Your S3, Google Cloud Storage, or Azure bucket |
| [OpenSandbox](/guide/infrastructure/opensandbox) | No | Yes, self-hosted | Your own sandbox service |
| [Runloop](/guide/infrastructure/runloop) | No | Yes | Hosted Devbox Blueprint |
| [Sprites](/guide/infrastructure/sprites) | No | Yes | Disposable Fly.io Linux environment |
| [Tensorlake](/guide/infrastructure/tensorlake) | No | Yes | Hosted registered image or snapshot |
| [Together Code Sandbox](/guide/infrastructure/together) | No | Yes | Hosted snapshot |
| [Vercel Sandbox](/guide/infrastructure/vercel) | No | Yes | Hosted runtime |

[Buildkite](/guide/infrastructure/buildkite) is not a cache or execution
provider. It is an example of CI orchestration for
[triggers](/guide/infrastructure/triggers).

## Publish artifacts

[Artifacts](/guide/infrastructure/artifacts) are a third, independent
capability: Struo packages immutable build products and publishes them to the
configured infrastructure, which decides how people download, install, or
serve them.

## Run locally for one invocation

The local cache stays available after you configure a shared cache. Set
`STRUO_CACHE_PROVIDER=local` to skip the shared cache for one command:

```sh
STRUO_CACHE_PROVIDER=local ./scripts/greet.sh
```

For execution, omit `--remote` to run on the current computer, or pass
`--compute <provider>` with `--remote` to choose a different named execution
provider for one invocation.

## Next steps

- [Connect a Project](/guide/infrastructure/connect), to bind this repository
  to a shared cache in one command.
- [Remote Execution](/guide/infrastructure/remote-execution), to run scripts and
  graph actions in fresh environments.
- [Become an Infrastructure Provider](/guide/infrastructure/providers), if you
  operate a cache service and want to integrate it with Struo.
