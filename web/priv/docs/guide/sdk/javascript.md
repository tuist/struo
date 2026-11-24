# JavaScript SDK

Use the `@tuist/struo` npm package to store blobs and action results in a
Struo cache from Node.js or the browser, and dispatch work to tabs or GitLab.

The package exposes cache primitives for Node.js and an IndexedDB cache for
browsers. Script execution belongs to the command line. See the
[SDK overview](/guide/sdk/) to compare languages.

## Installation

Install the package from npm:

```sh
npm install @tuist/struo
```

The package requires Node.js 18 or later and includes prebuilt native Struo
libraries for supported platforms. Set `STRUO_LIBRARY_PATH` when you need to
load a custom `libstruo` build.

## Quick start

Store a first blob in the default local cache and read it back:

```js
const { Cache } = require("@tuist/struo");

async function main() {
  const cache = new Cache();
  const digest = await cache.putBlob(Buffer.from("hello"));
  const bytes = await cache.getBlob(digest);

  console.assert(bytes.equals(Buffer.from("hello")));
}
```

In a browser, start from the [browser cache](#browser-cache) instead.

### Open a cache

| API | Use |
| --- | --- |
| `new Cache()` | Opens the [default local cache](/guide/sdk/#choose-a-cache). |
| `new Cache({ localCacheRoot })` | Opens an isolated local cache at a caller-owned root. |
| `new Cache({ workspaceRoot })` | Resolves the same effective provider as the command line for a workspace. |
| `version()` | Synchronously returns the linked Struo version. |

## Blobs

Blobs are content-addressed byte payloads. Bytes can be a `Buffer`,
`Uint8Array`, or string; strings are encoded as UTF-8 before hashing or
storing. `digest()` is synchronous; storage methods return promises.

| API | Use |
| --- | --- |
| `digest(bytes)` | Synchronously returns the content digest for bytes without writing them to the cache. |
| `putBlob(bytes)` | Stores bytes and resolves to their content digest. |
| `putFile(path)` | Stores a file without loading its complete contents into JavaScript memory. |
| `getBlob(digest)` | Resolves to the bytes for a digest as a `Buffer`. |
| `getBlobToFile(digest, path)` | Writes a blob to a file and resolves to its byte count. |
| `hasBlob(digest)` | Resolves to whether a blob exists. |

Prefer `putFile` and `getBlobToFile` for logs, archives, compiler outputs, and
other payloads whose size is not tightly bounded.

## Action keys and results

`ActionKey` builds a synchronous, versioned identity from ordered, labeled
inputs. Input order is significant and must be deterministic:

```js
const { ActionKey } = require("@tuist/struo");

const source = await cache.putFile("inputs/source");
const actionDigest = new ActionKey("example.compile")
  .addBytes("tool", "compiler")
  .addDigest("source", source)
  .digest();
```

Store and read the result of an action you ran yourself. The SDK does not run
commands:

```js
const stdout = await cache.putBlob("compiled");
await cache.putActionResult({ exitCode: 0, stdout }, actionDigest);
const cached = await cache.getActionResult(actionDigest);
```

| API | Use |
| --- | --- |
| `putActionResult(result, actionDigest)` | Stores a cached result for an action digest. |
| `getActionResult(actionDigest)` | Resolves to a cached result, or `null` when none exists. |
| `forgetAction(actionDigest)` | Removes one cached action result and resolves to whether one existed. Referenced blobs are left intact. |
| `stats()` | Resolves to local cache statistics. |

## Browser cache

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

Browser-aware bundlers also select this entry point for `@tuist/struo`. The
browser library computes the same BLAKE3 digests and versioned action keys as
the native library and stores blobs and action results in IndexedDB.

Serve the Wasm asset alongside its JavaScript module. `init()` fetches the
asset relative to that module. If your bundler relocates it, pass
`init({ module_or_path: wasmUrl })`. Your content security policy must allow
Wasm compilation, for example with `script-src 'self' 'wasm-unsafe-eval'`.

After initialization, `digest()` and action-key construction are synchronous.
Storage methods resolve after their IndexedDB transactions commit. `getBlob()`
returns a `Uint8Array` and verifies its content digest. A missing blob rejects
with `StruoError`; a missing action returns `null`. Action records may
reference blobs that are not present, so check or retrieve their blobs before
treating a record as a usable cache hit.

The default database name is `struo`. Same-origin tabs share a cache when they
use the same name; separate names isolate demonstrations. `close()` releases
this instance's connection and keeps its data. `clear()` removes blobs and
actions in that database, including data used by other tabs. `forgetAction()`
removes only the action record.

Browser caches differ from the Node.js cache:

- They accept `databaseName` rather than `localCacheRoot` or `workspaceRoot`,
  and do not expose `putFile`, `getBlobToFile`, or remote providers.
- Blob operations hold the complete payload in memory, so keep payloads small.
- Call `free()` when finished with an `ActionKey` to release its Wasm hash
  state.
- Statistics count unique blob records, uncompressed payload bytes, action
  records, and their UTF-8 JSON bytes, excluding IndexedDB overhead.
- Browsers may evict data; quota and permission failures reject with
  `StruoError`. Keep another copy of data you cannot regenerate, and request
  persistent storage with `navigator.storage.persist()` if needed.

## Browser execution

The separate `@tuist/struo/browser/execution` entry point supports two dispatch
paths: named operations in same-origin tabs and GitLab CI pipelines. Neither
path runs arbitrary native commands inside the browser.

### Across tabs

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
`memoryLimitBytes` before offering to execute it, and one worker receives the
task. This is cooperative admission control, not a browser memory sandbox.
Supply realistic estimates and use Web Workers for CPU-heavy handlers to avoid
blocking a tab's UI.

`execute()` accepts an `AbortSignal`. Cancellation asks the handler to stop;
the worker keeps its reservation until the handler settles. A timeout or
closed executor means completion is unknown. The SDK does not retry claimed
work or promise exactly-once effects after a tab crashes. Unsupported
operations and unavailable workers time out. Keep worker tabs open and expect
browsers to throttle background tabs. Cache lookup and action-result storage
remain explicit caller decisions.

All same-origin pages can access the channel and database. Use this only among
trusted pages; a channel name does not authenticate a worker or isolate
tenants. Handlers must validate their own inputs and return an action result
with blob digests.

### OAuth2 login

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

The client keeps a single-use verifier and state in sessionStorage for up to
ten minutes, checks the callback, and strips the authorization response from
the page URL. Tokens stay in memory, so a full page reload or navigation to
another document requires another login. `auth.logout()` clears local
authentication immediately; await it to revoke current and replacement tokens
through the revocation endpoint. It rejects if revocation fails, but local
logout still applies.

Treat XSS protection as part of your authentication boundary. The SDK cannot
protect tokens from compromised scripts on your origin. Use only trusted
provider URLs and avoid logging tokens, callback URLs, or pipeline variables.

### GitLab pipelines

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
cancellation, or skipping. On a wait error, timeout, or abort, `execute()`
attempts to cancel the pipeline it created; check GitLab if cancellation
fails. A lost submission response may mean a pipeline exists even though the
SDK reports an error, so check the project's pipelines before retrying.
Cancellation and deadlines also apply while waiting for authentication.

This adapter passes refs and variables to an existing CI configuration. It
does not transfer IndexedDB inputs, provision runners, or implement the native
remote-execution protocol. Pipeline refs can move, so the SDK does not cache
pipeline execution as a Struo action. Include compiler versions and immutable
input identities when building action keys in your application.

## Errors and types

Failures reject with `StruoError`, exported from both entry points. Invalid
arguments, such as a non-string path or conflicting cache options, throw
`TypeError`. A missing action result resolves to `null`; a remote service that
cannot answer rejects instead of reporting a miss. See
[misses, failures, and integrity](/guide/sdk/#misses-failures-and-integrity).

Action results use camel case:

```js
{
  exitCode: 0,
  stdout: "<stdout digest>",
  stderr: null,
  outputs: {}
}
```

`stats()` resolves to `blobCount`, `blobBytes`, `actionCount`, and
`actionBytes`. TypeScript declarations ship with the package.

## Next steps

- [SDK overview](/guide/sdk/): understand cache selection, retries, and
  transfer budgets shared by every SDK.
- [Infrastructure](/guide/infrastructure/): configure the remote cache that a
  workspace-bound Node.js client resolves.
- [Rust SDK](/guide/sdk/rust): see the native API that computes the same
  digests and action keys.
