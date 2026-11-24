# Struo SDKs

Use the Struo SDKs for Rust, Swift, JavaScript, Ruby, and C to store blobs and
action results in a local or shared Struo cache from your own application.

Use the [command line](/reference/cli/) instead when the goal is to execute
scripts, build targets, run tests, or manage runtime sessions. The cache entry
points do not execute commands or load the repository graph. JavaScript also
offers a separate [browser execution entry point](/guide/sdk/javascript#browser-execution)
for same-origin tab handlers and OAuth2-authenticated GitLab pipelines.

## What every SDK provides

The SDKs expose the same small cache surface:

- Open the default local cache, an isolated local root, or the effective
  provider configured for a workspace.
- Store and retrieve content-addressed byte payloads.
- Store and materialize files without loading their complete contents into the
  host language.
- Build versioned action keys from labeled values and content digests.
- Associate an action digest with a completed action result.
- Remove an action result without deleting its referenced payloads.
- Inspect local cache statistics.

## Choose a language

| Language | Package | Call style | Guide |
| --- | --- | --- | --- |
| Rust | `struo` crate | Asynchronous | [Rust SDK](/guide/sdk/rust) |
| Swift | `Struo.xcframework` and the matching Swift wrapper | Asynchronous | [Swift SDK](/guide/sdk/swift) |
| Ruby | `struo` gem | Synchronous | [Ruby SDK](/guide/sdk/ruby) |
| JavaScript | `@tuist/struo` package for Node.js and browsers | Promise-based | [JavaScript SDK](/guide/sdk/javascript) |
| C | `libstruo` and `struo.h` | Synchronous | [C SDK](/guide/sdk/c) |

Choose the SDK used by the process that owns the integration. All SDKs use
compatible digests and action-result records, so separate tools can share a
configured cache without translating them.

## Choose a cache

Every native SDK can open three kinds of cache:

- **Default local cache.** Follows the
  [X Desktop Group base-directory convention](https://specifications.freedesktop.org/basedir-spec/latest/):
  `$XDG_CACHE_HOME/struo/cas` when `XDG_CACHE_HOME` is set, and
  `$HOME/.cache/struo/cas` otherwise.
- **Explicit local root.** Use it for test isolation or when the caller owns the
  cache lifetime.
- **Workspace cache.** Use it when the integration must share the same
  effective provider as the command line. Selection considers the process
  override, workspace infrastructure, compatible Tuist project configuration,
  and the user default, in that order.

In a browser, await Wasm initialization and open an IndexedDB cache with an
optional database name. Browser caches share content digests and action keys
with native clients, but do not expose filesystem methods or workspace
provider configuration. See the [JavaScript browser cache](/guide/sdk/javascript#browser-cache).

Statistics describe the local tier. Remote uploads are best effort: a
configured remote provider writes through the local tier and keeps the local
write usable when the remote service is unavailable. Streamed inputs larger
than eight mebibytes remain in the local tier.

## Design action keys

Partition each integration with its own action-key namespace and add every
input that can affect the result. Input order is significant and must be
deterministic. The SDK cannot tell whether an application omitted an input or
whether replaying a result is safe.

Start with blob storage, then add action-result storage only after the
application has a deterministic action identity.

## Misses, failures, and integrity

Reads distinguish a miss from a failure. A missing action result, or a blob
existence check that answers false, means the cache confirmed the content is
absent. That includes a remote action result whose referenced blobs the remote
service confirms are gone, which is reported as a miss so the action can run
again and republish it.

A remote service that cannot be reached, times out, rejects the credentials,
or returns a malformed entry produces an error instead. To keep working
through a remote outage, catch the error and decide whether to run the work
without a cached result; do not treat the outage as proof that nothing is
cached.

Idempotent remote reads and content-addressed writes are retried a bounded
number of times with randomized backoff and an overall time limit. When the
remote service says how long to wait, the client waits at least that long
within the same limit. A quota error is retried only when the service gives
such a hint. Refused credentials and malformed requests are never retried.
A failed connection, endpoint lookup, or credential service outage is retried
after a cooldown instead of disabling the remote tier for the rest of the
process. Rejected or missing credentials ask you to sign in again.

Blobs are verified against their digest before they are used, including
downloads and restored files. A local blob that fails the check is removed and
reported as corrupt; with a remote provider it is fetched again once before
the read fails. Reading a whole blob into memory is the caller's choice; copy
large blobs to a file instead.

Error messages and logs name remote endpoints by scheme, host, and port only.
Text a server sends back, such as a status message or response body, is not
repeated because it can contain credentials.

## Transfer budgets

A remote provider bounds the memory buffers and scratch disk its transfers
use. By default it allows 256 mebibytes of transfer buffers and 32 gibibytes
of staged downloads. A streamed download reserves slightly more than twice its
declared size in scratch space. Providers cloned from one another share their
budget; separately constructed providers each have their own, and the command
line shares one budget across the providers it resolves.

Every remote operation needs at least 32 mebibytes and 64 kibibytes of
transfer memory, which the Rust crate exposes as its minimum transfer memory.
Uploading a buffer the caller already holds reserves about twice its size, so
buffers larger than roughly 15 mebibytes need more than the minimum. Work that
does not fit the budget fails with an exhausted error before anything is sent;
it is never treated as a cache miss.

These budgets account for the buffers the cache sizes for its transfers, not
total process memory. Connecting, endpoint discovery, authentication, and
upload compression working memory are outside them.

## Next steps

- [Rust SDK](/guide/sdk/rust): start here when your integration is written in
  Rust or you need blob staging and streaming producers.
- [JavaScript SDK](/guide/sdk/javascript): use the cache from Node.js or a
  browser, including tab and GitLab execution.
- [Infrastructure](/guide/infrastructure/): configure the shared cache that a
  workspace-bound SDK client resolves.
- [Cache provider contract](/reference/infrastructure/cache): see the
  operations a remote cache service implements.
