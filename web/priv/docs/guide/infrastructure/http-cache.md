# HTTP Cache

An HTTP cache server can hold Struo's shared cache. Every machine keeps its
local cache, consults the server on a local miss, and publishes new results to
it. Struo needs only three requests from the server: `GET`, `HEAD`, and `PUT`
on a key path, which build caches with a `WebDAV` or plain HTTP interface
provide.

Use it with a managed build cache that your CI runners already reach, such as
the caches some hosted runner providers include, or with a cache server you
run yourself.

**Capabilities:** shared caching. This provider does not execute actions.

## Prerequisites

Use a reachable HTTP or WebDAV server that supports `GET`, `HEAD`, and `PUT`,
returns content lengths, and makes writes readable only after they complete.
Obtain credentials with read/write access, or read-only access for untrusted jobs.

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

The table names environment variables, never their values, so tokens stay out
of configuration files. A named variable that is unset or empty fails the
command instead of sending an anonymous request. Omit all three credential
fields for a server that needs no authentication.

Struo sends credentials only over HTTPS. Plain `http://` is accepted only
without credentials, or to a loopback address such as `localhost` for local
testing. Use environment variables for credentials; Struo rejects URLs with
embedded user names or passwords, queries, or fragments. `key_prefix` must
be a relative path without `.` or `..` segments. Struo does not follow
redirects, so configure the server's final endpoint.

The same `kind = "http_cache"` table works in `~/.config/struo/config.toml`
for a personal default, and as `[cache_provider]` with `kind = "http_cache"`
for a single workspace.

### Hosted runner caches

Some runner providers offer an HTTP build cache with credentials their
runners receive automatically. Their documentation describes these
interfaces for sccache and similar tools; Struo has not been tested against
either service yet.

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

### CI services

Struo does not use a CI service's own cache storage. GitHub Actions has a
cache service, but its entries cannot be overwritten, are scoped to branches,
are [limited to 200 uploads per minute per repository](https://docs.github.com/en/actions/reference/limits), and are reachable only
with credentials the runner gives to actions. That does not fit a cache that
publishes one object per output, so Struo does not support it. Buildkite Cache
saves and restores whole directories through `buildkite-agent cache`. Struo
has no integration with GitLab CI or CircleCI caches.

On any of these services, point Struo at an HTTP cache or an
[object storage bucket](/guide/infrastructure/object-storage) that the jobs
can reach, and pass its credentials as secrets. Build artifacts are not a
substitute: they belong to a run and are fetched by job, not looked up by an
action's key.

Restrict publishing from jobs that run untrusted code, such as pull requests
from forks. Struo verifies content against its digest, but it cannot verify
that an action result is correct, and these caches are shared across
branches. Give those jobs read-only credentials when the service supports
them, and set `STRUO_CACHE_READ_ONLY=1` so Struo does not publish:

```yaml
env:
  STRUO_CACHE_READ_ONLY: ${{ github.event_name == 'pull_request' && '1' || '0' }}
```

## Limits and cleanup

### Failures

- **Unreachable server, timeouts, `408`, `429`, and `5xx`:** the action runs
  locally and the result is kept in the local cache.
- **`401` or `403`:** the command fails rather than silently running without
  the shared cache. Fix the credentials, or set `STRUO_CACHE_PROVIDER=local`
  to bypass the server for one invocation.
- **Corrupt or truncated content:** it is rejected before it enters the local
  cache, and the command reports the corruption.

Publishing is best effort. A failed upload is logged as a warning, and the
command still succeeds with its local result. Struo never deletes keys from
the server; configure retention there. A result whose referenced content has
been evicted is treated as a miss.

### Compatibility

Struo's automated checks cover local HTTP servers. Depot, Namespace and other
hosted caches have not been verified against their live services. Check your
server against the [HTTP cache contract](/reference/infrastructure/cache#http-cache-layout)
before adopting it.

### Service requirements

- The server must return `Content-Length` for `GET` and `HEAD`. Chunked
  responses without a length are rejected as protocol errors.
- The server must not make a `PUT` readable before its whole body arrives.
  Struo verifies every download, so a partial object is rejected, but it
  turns a hit into a failure.
- Content is stored uncompressed.
- Remote reads use `HEAD` before `GET` to reserve memory and staging space
  before opening the body. Result lookup also uses one `HEAD` per referenced
  blob not already held locally.
- Short-lived tokens are read when the command starts. A token that expires
  during a long command turns later requests into refusals.
- Isolation is whatever the server's access control provides. Use a separate
  prefix with its own credentials for each trust boundary.
