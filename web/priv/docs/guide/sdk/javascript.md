---
prev: false
next: false
---

# JavaScript Software Development Kit

The JavaScript library is the `@tuist/struo` npm package. It exposes cache
primitives for Node.js applications, tools, and browsers. Script execution
belongs to the command line and is not part of this library. See the
[language-library overview](/guide/sdk/) to compare the available bindings.

```js
const { Cache } = require("@tuist/struo");

async function main() {
  const cache = new Cache();
  const digest = await cache.putBlob(Buffer.from("hello"));
  const bytes = await cache.getBlob(digest);

  console.assert(bytes.equals(Buffer.from("hello")));
}
```

## Installation

Install the package from npm:

```sh
npm install @tuist/struo
```

The package includes prebuilt native Struo libraries for supported
platforms. Set `STRUO_LIBRARY_PATH` when you need to load a custom
`libstruo` build.

## Node.js Cache

`Cache` can open the default local cache, an explicit local root, or the
effective provider for a workspace. The default follows the
[X Desktop Group base-directory convention](https://specifications.freedesktop.org/basedir-spec/latest/):
its cache root is `$XDG_CACHE_HOME/struo/cas` when `XDG_CACHE_HOME` is set, and
`$HOME/.cache/struo/cas` otherwise.

`version()` and `digest(bytes)` are synchronous. Cache storage operations
return promises and must be awaited. When `bytes` is a string, the library
encodes it using [Unicode Transformation Format, 8-bit](https://www.unicode.org/faq/utf_bom.html#UTF8)
before hashing or storing it.

| Application programming interface | Use |
| --- | --- |
| `new Cache()` | Opens the default local cache using the operating-system convention. |
| `new Cache({ localCacheRoot })` | Opens an isolated local cache at a caller-owned root. |
| `new Cache({ workspaceRoot })` | Resolves the effective provider for a workspace. |
| `version()` | Synchronously returns the linked Struo version. |
| `digest(bytes)` | Synchronously returns the content digest for bytes without writing them to the cache. |
| `putBlob(bytes)` | Stores bytes and returns a promise for their content digest. |
| `putFile(path)` | Stores a file without loading its complete contents into JavaScript memory. |
| `getBlob(digest)` | Reads bytes for a digest and returns a promise. |
| `getBlobToFile(digest, path)` | Writes a blob to a file and returns a promise for its byte count. |
| `hasBlob(digest)` | Returns a promise for whether a blob exists. |
| `putActionResult(result, actionDigest)` | Stores a cached result for an action digest and returns a promise. |
| `getActionResult(actionDigest)` | Returns a promise for a cached result when one exists. |
| `forgetAction(actionDigest)` | Removes one cached action result and returns a promise. Referenced blobs are left intact. |
| `stats()` | Returns a promise for local cache statistics. |

Prefer `putFile` and `getBlobToFile` for logs, archives, compiler outputs, and
other payloads whose size is not tightly bounded.

## Browser Cache

Import the browser entry point and await Wasm initialization before creating
a cache or computing digests:

```js
import { init, Cache, ActionKey } from "@tuist/struo/browser";

await init();
const cache = new Cache({ databaseName: "blog-demo" });
const input = await cache.putBlob("hello");
const key = new ActionKey("demo").addDigest("input", input);
await cache.putActionResult({ exitCode: 0, stdout: input }, key.digest());

const result = await cache.getActionResult(key.digest());
const bytes = await cache.getBlob(result.stdout);
console.log(new TextDecoder().decode(bytes));
console.log(await cache.stats());

key.free();
await cache.close();
```

Browser-aware bundlers also select this entry point for `@tuist/struo`.
The browser library uses the same Rust implementation of BLAKE3 digests and
versioned action keys as the native library. IndexedDB stores the blob
payloads and action results. Writing identical content reuses its digest and
keeps one blob record, including when tabs write it concurrently.

Serve the Wasm asset alongside its JavaScript module. `init()` fetches the
asset relative to that module. If your bundler relocates it, pass
`init({ module_or_path: wasmUrl })`. Your content security policy must allow
Wasm compilation, for example with `script-src 'self' 'wasm-unsafe-eval'`.

After initialization, `digest()` and action-key construction remain
synchronous. Storage methods return promises that resolve after their
IndexedDB transactions commit. `getBlob()` returns `Uint8Array` and verifies
its content digest. A missing blob rejects with `StruoError`; a missing action
returns `null`. As with the native local cache, action records may reference
blobs that are not present, so check or retrieve their blobs before treating
the record as a usable cache hit.

The default database name is `struo`. Same-origin tabs share a cache when
they use the same name. Separate names isolate demonstrations. `close()`
releases this instance's connection and preserves its data for a later page
load. `clear()` removes blobs and actions in that database, including data
used by other tabs. `forgetAction()` removes only the action record.

Browser caches accept `databaseName` rather than `localCacheRoot` or
`workspaceRoot`. They do not expose `putFile`, `getBlobToFile`, or native
remote-provider configuration. Blob operations hold the complete payload in
memory, so keep interactive examples small. Call `free()` when finished with
an `ActionKey` to release its Wasm hash state.

Browser statistics count unique blob records, uncompressed payload bytes,
action records, and their UTF-8 JSON bytes. They exclude IndexedDB's storage
overhead and differ from the native cache's compressed on-disk sizes.
Browsers may evict data; quota and permission failures reject with
`StruoError`. Keep another copy of data you cannot regenerate. Applications
can request persistent storage through `navigator.storage.persist()`, but
the browser decides whether to grant it.

## Browser Execution

The separate `@tuist/struo/browser/execution` entry point supports two dispatch
paths: named operations in same-origin tabs and GitLab CI pipelines. Neither
path runs arbitrary native commands inside the browser.

### Across Tabs

In a worker tab, install the operations your application permits:

```js
import { init, Cache } from "@tuist/struo/browser";
import { serveTab } from "@tuist/struo/browser/execution";

await init();
const cache = new Cache({ databaseName: "blog-demo" });
const worker = serveTab({
  memoryLimitBytes: 64 * 1024 * 1024,
  handlers: {
    uppercase: async ({ inputs }, { signal }) => {
      signal.throwIfAborted();
      const source = new TextDecoder().decode(await cache.getBlob(inputs.source));
      const output = await cache.putBlob(source.toUpperCase());
      return { exitCode: 0, outputs: { "output.txt": output } };
    },
  },
});
// Call worker.close() when this tab should stop accepting work.
```

From another tab at the same origin:

```js
import { init, Cache } from "@tuist/struo/browser";
import { TabExecutor } from "@tuist/struo/browser/execution";

await init();
const cache = new Cache({ databaseName: "blog-demo" });
const source = await cache.putBlob("hello from another tab");
const executor = new TabExecutor();
try {
  const { result, workerId } = await executor.execute({
    operation: "uppercase", inputs: { source }, args: [],
    memoryBytes: 1024 * 1024,
  }, { timeoutMs: 10_000 });
  console.log(workerId, new TextDecoder().decode(await cache.getBlob(result.outputs["output.txt"])));
} finally {
  executor.close();
  await cache.close();
}
```

Replace the example handler with a browser-compatible compiler to distribute
compilation. Both sides must use the same database and `channelName` (default:
`struo-execution`). BroadcastChannel carries task metadata and result digests,
not blob payloads. Messages have a 64 KiB limit.

Workers reserve each task's declared `memoryBytes` against their own
`memoryLimitBytes` before offering to execute it. One worker receives the
client's claim; the others release their reservations. This is cooperative
admission control, not a browser memory sandbox. Supply realistic estimates
and use Web Workers for CPU-heavy handlers to avoid blocking a tab's UI.
Workers announce released capacity so clients can rediscover unclaimed tasks
until their deadline. Clients do not resubmit tasks that already have a worker.

`execute()` accepts an `AbortSignal`. Cancellation asks the handler to stop;
the worker retains its reservation until the handler settles. A timeout or
closed executor means completion is unknown. The SDK does not retry claimed
work or promise exactly-once effects after a tab crashes. Unsupported operations and unavailable
workers time out. Keep workers open and expect browsers to throttle background
tabs. Cache lookup and action-result storage remain explicit caller decisions.

All same-origin pages can access the channel and database. Use this only among
trusted pages; a channel name does not authenticate a worker or isolate tenants.
Handlers must validate their own operation-specific inputs and return an action
result with blob digests.

### OAuth2 Login

Register a public (non-confidential) OAuth application with your provider and
an exact same-origin callback URL. Do not put a client secret in a web page.
The authorization and token endpoints must support authorization-code PKCE
with S256; token and revocation endpoints must permit your origin through CORS.
Use HTTPS. HTTP loopback URLs work for local development.

For GitLab, register an application with the `api` scope and configure:

```js
import { OAuth2Client } from "@tuist/struo/browser/execution";

const auth = new OAuth2Client({
  authorizationEndpoint: "https://gitlab.com/oauth/authorize",
  tokenEndpoint: "https://gitlab.com/oauth/token",
  revocationEndpoint: "https://gitlab.com/oauth/revoke",
  clientId: "YOUR_PUBLIC_CLIENT_ID",
  redirectUri: `${location.origin}/oauth/callback`,
  scopes: ["api"],
});

// In a login click handler:
// location.assign(await auth.authorizationURL());

// On the callback page, recreate auth with the same configuration:
if (location.pathname === "/oauth/callback" && location.search) {
  await auth.completeAuthorization();
}
```

The client stores a single-use verifier and state in sessionStorage for up to
ten minutes, checks the callback, then removes them. It strips the authorization
response from the current page URL after validation. Tokens remain in memory,
so a full page reload requires another login. Reuse the authenticated client
within your application; do not navigate to another document after the callback
and expect its tokens to persist. Refresh requests share one in-flight exchange.
`auth.logout()` clears local authentication immediately. Await it to finish
in-flight token exchanges and revoke both current tokens and any replacement
tokens through the configured revocation endpoint. It rejects if revocation
fails; local logout still applies.

Treat XSS protection as part of your authentication boundary. The SDK cannot
protect tokens from compromised scripts on your origin. Use only trusted
provider URLs and avoid logging tokens, callback URLs or pipeline variables.

### GitLab Pipelines

Configure a project with a CI job that performs the compilation and publishes
its output as an artifact. The logged-in user needs permission to run pipelines
on the selected ref. Dispatching creates a real pipeline and may consume paid
runner time, so call it only after the user requests execution.

```js
import { GitLabExecutor } from "@tuist/struo/browser/execution";

const remote = new GitLabExecutor({ project: "your-group/your-project", auth });
const pipeline = await remote.execute({
  ref: "main",
  variables: { BUILD_MODE: "demo" },
}, { timeoutMs: 300_000, signal: controller.signal });

if (pipeline.status !== "success") throw new Error(`Pipeline ${pipeline.status}`);
const jobs = await remote.jobs(pipeline.id);
const job = jobs.find((job) => job.name === "compile" && job.status === "success");
if (!job) throw new Error("No successful compile job");
const artifact = await remote.storeArtifact({
  jobId: job.id, path: "dist/output.js", cache,
  maxBytes: 4 * 1024 * 1024,
});
console.log(new TextDecoder().decode(await cache.getBlob(artifact)));
```

Create `controller` with `new AbortController()` and reuse an initialized cache
and the authenticated `auth` client. Supply `url` to target a self-managed
GitLab instance. Artifact downloads follow redirects, which may require CORS
support on the artifact host too. The byte limit applies before cache storage.
The job listing returns at most 100 jobs.

`dispatch()` returns a pipeline without waiting. Use `get()` and `cancel()`
to manage it yourself, or `execute()` to poll until success, failure,
cancellation or skipping. On a wait error, timeout or abort, `execute()` attempts
to cancel the pipeline it created. Check GitLab if cancellation fails. A lost
submission response may mean a pipeline exists even though the SDK reports an
error; do not retry without checking the project's pipelines.

Request cancellation and execution deadlines also apply while waiting for
authentication. Aborting a request stops that wait without cancelling a shared
token refresh that other callers may need.

This adapter passes refs and variables to an existing CI configuration. It
does not transfer IndexedDB inputs, provision runners, or implement the native
remote-execution protocol. Pipeline refs can move, so the SDK does not cache
pipeline execution as a deterministic Struo action. Include compiler versions
and immutable input identities when building action keys in your application.

## Action Keys

`ActionKey` builds a synchronous, versioned identity from ordered, labeled
inputs:

```js
const { ActionKey } = require("@tuist/struo");

const source = await cache.putFile("inputs/source");
const actionDigest = new ActionKey("example.compile")
  .addBytes("tool", "compiler")
  .addDigest("source", source)
  .digest();
```

Input order is significant and must be deterministic.

## Types

Action results use JavaScript-friendly camel case:

```js
{
  exitCode: 0,
  stdout: "<stdout digest>",
  stderr: null,
  outputs: {}
}
```

`stats()` returns `blobCount`, `blobBytes`, `actionCount`, and
`actionBytes`.
