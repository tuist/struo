# HTTP Build Cache

Share Struo cache results across machines and CI through any HTTP or WebDAV
server that supports `GET`, `HEAD`, and `PUT`, such as Depot or Namespace caches.

Every machine keeps its local cache, consults the server on a local miss, and
publishes new results to it. Use a managed build cache your CI runners already
reach, or a cache server you run yourself.

## Capabilities

| Capability | Support |
| --- | --- |
| Shared cache | Yes |
| Remote execution | No; use an [execution provider](/guide/infrastructure/#available-providers) |
| Artifact publication | No |
| Project setup | Manual [configuration](#configuration) |
| Authentication | [Bearer or basic credentials](#authentication) |

Automated checks cover local HTTP servers; hosted caches are unverified.

## Prerequisites

- A reachable HTTP or WebDAV server that supports `GET`, `HEAD`, and `PUT` on a
  key path, returns `Content-Length` for `GET` and `HEAD`, and makes a `PUT`
  readable only after its whole body arrives.
- Credentials with read/write access, or read-only access for untrusted jobs.

## Configuration

Name the server in the repository root `struo.toml`:

```toml
[infrastructures.build_cache]
kind = "http_cache"
url = "https://cache.example.com"
key_prefix = "struo/"
token_env = "BUILD_CACHE_TOKEN"

[infrastructure.cache]
provider = "build_cache"
```

| Field | Meaning |
| --- | --- |
| `url` | Base URL. A URL without a scheme uses `https://`. |
| `key_prefix` | Optional path Struo writes under, such as a team or cache name. |
| `token_env` | Name of an environment variable holding a bearer token. |
| `username_env`, `password_env` | Names of environment variables holding basic authentication credentials. Set both, and not `token_env`. |
| `read_only` | Read the shared cache without publishing to it. |

`key_prefix` must be a relative path without `.` or `..` segments. Struo does
not follow redirects, so configure the server's final endpoint.

The same `kind = "http_cache"` table works in `~/.config/struo/config.toml`
for a personal default, and as `[cache_provider]` with `kind = "http_cache"`
for a single workspace.

### Authentication

The credential fields name environment variables, never their values, so tokens
stay out of configuration files. A named variable that is unset or empty fails
the command instead of sending an anonymous request. Omit all three credential
fields for a server that needs no authentication.

Struo sends credentials only over HTTPS. Plain `http://` is accepted only
without credentials, or to a loopback address such as `localhost` for local
testing. Struo rejects URLs with embedded user names or passwords, queries, or
fragments.

Short-lived tokens are read when the command starts. A token that expires
during a long command turns later requests into refusals.

### Hosted runner caches

Some runner providers offer an HTTP build cache with credentials their runners
receive automatically. Their documentation describes these interfaces for
sccache and similar tools; Struo has not been tested against either service yet.

- **Depot:** Depot documents `https://cache.depot.dev` as a
  [`WebDAV` cache](https://depot.dev/docs/cache/integrations/sccache), and
  [injects a short-lived `DEPOT_CACHE_TOKEN`](https://depot.dev/docs/cache/authentication)
  into jobs on its GitHub Actions runners. Outside those runners, use a Depot
  organization or user token.

  ```toml
  [infrastructures.depot]
  kind = "http_cache"
  url = "https://cache.depot.dev"
  token_env = "DEPOT_CACHE_TOKEN"
  ```

- **Namespace:** [`nsc cache sccache setup`](https://namespace.so/docs/reference/cli/sccache-setup)
  prints the endpoint, a key prefix, and a short-lived token for a named cache.
  Copy the endpoint and prefix into `url` and `key_prefix`, and export the
  token under the variable `token_env` names.

## Usage

Run your cacheable scripts or builds as usual after configuring the binding.
Verify sharing from a second machine with an empty local cache, the same
configuration and inputs. A hit on the first machine alone can come from its
local cache.

### Use it from CI

Struo does not use a CI service's own cache storage. GitHub Actions cache
entries cannot be overwritten, are scoped to branches, are
[limited to 200 uploads per minute per repository](https://docs.github.com/en/actions/reference/limits),
and are reachable only with credentials the runner gives to actions, so Struo
does not support them. Buildkite Cache saves whole directories, and Struo has no
integration with GitLab CI or CircleCI caches. Build artifacts are not a
substitute: they belong to a run and are fetched by job, not by an action's key.

On any of these services, point Struo at an HTTP cache or an
[object storage bucket](/guide/infrastructure/object-storage) that the jobs
can reach, and pass its credentials as secrets.

### Restrict untrusted jobs

Struo verifies content against its digest, but it cannot verify that an action
result is correct, and these caches are shared across branches. Give jobs that
run untrusted code, such as pull requests from forks, read-only credentials
when the service supports them, and set `STRUO_CACHE_READ_ONLY=1` so Struo does
not publish:

```yaml
env:
  STRUO_CACHE_READ_ONLY: ${{ github.event_name == 'pull_request' && '1' || '0' }}
```

## Limits and cleanup

Struo never deletes keys from the server; configure retention there. A result
whose referenced content has been evicted is treated as a miss.

### Failures

- **Unreachable server, timeouts, `408`, `429`, and `5xx`:** the action runs
  locally and the result is kept in the local cache.
- **`401` or `403`:** the command fails rather than silently running without
  the shared cache. Fix the credentials, or set `STRUO_CACHE_PROVIDER=local`
  to bypass the server for one invocation.
- **Corrupt or truncated content:** it is rejected before it enters the local
  cache, and the command reports the corruption.

Publishing is best effort. A failed upload is logged as a warning, and the
command still succeeds with its local result.

### Service requirements

- Chunked responses without a `Content-Length` are rejected as protocol errors.
- A `PUT` that becomes readable before its body completes turns a hit into a
  failure, because Struo rejects the partial object.
- Content is stored uncompressed.
- Remote reads use `HEAD` before `GET`. Result lookup also uses one `HEAD` per
  referenced blob not already held locally.
- Isolation is whatever the server's access control provides. Use a separate
  prefix with its own credentials for each trust boundary.

Check your server against the
[HTTP cache contract](/reference/infrastructure/cache#http-cache-layout) before
adopting it.

## Next steps

- [Object Storage Cache](/guide/infrastructure/object-storage): share results
  through an S3, Google Cloud Storage, or Azure bucket instead.
- [Cache provider contract](/reference/infrastructure/cache): see the exact
  requests and layout Struo uses.
- [Remote Execution](/guide/infrastructure/remote-execution): run actions on
  another machine and reuse their results through this cache.
