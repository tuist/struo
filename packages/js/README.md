# Struo JavaScript Software Development Kit

`@tuist/struo` exposes cache primitives to Node.js and browsers. Node.js loads
the native Struo library. Browsers use Wasm for content digests and action keys,
with IndexedDB for storage. Cache entry points do not execute scripts.
The separate `@tuist/struo/browser/execution` entry point dispatches work to
same-origin tabs or GitLab pipelines authenticated through OAuth2 PKCE.

```js
const { Cache } = require("@tuist/struo");

async function example() {
  const cache = new Cache();
  const digest = await cache.putBlob(Buffer.from("hello"));
  const bytes = await cache.getBlob(digest);

  console.assert(bytes.equals(Buffer.from("hello")));
}
```

The package looks for a bundled native library under `prebuilds/`. Set
`STRUO_LIBRARY_PATH` to load a custom `libstruo` build.

Use `new Cache({ workspaceRoot })` to share the effective provider configured
for a repository, or `new Cache({ localCacheRoot })` for isolation. Use
`putFile` and `getBlobToFile` for large payloads so the host does not need to
hold their complete contents in JavaScript memory. `ActionKey` builds a stable
digest from ordered, labeled inputs.

## Browsers

```js
import { init, Cache, ActionKey } from "@tuist/struo/browser";

await init();
const cache = new Cache({ databaseName: "my-blog-demo" });
const blob = await cache.putBlob("hello");
const key = new ActionKey("demo").addDigest("input", blob);
await cache.putActionResult({ exitCode: 0, stdout: blob }, key.digest());
console.log(new TextDecoder().decode(await cache.getBlob(blob)));
console.log(await cache.stats());
key.free();
await cache.close();
```

Browser-aware bundlers also select this entry point when importing
`@tuist/struo`. Serve the generated Wasm asset alongside its JavaScript module;
`init()` fetches it relative to that module. If your bundler relocates assets,
pass `init({ module_or_path: wasmUrl })`. A content security policy must allow
Wasm compilation, commonly through `script-src 'self' 'wasm-unsafe-eval'`.

Cache operations return promises that resolve after the IndexedDB transaction
commits. `getBlob` returns `Uint8Array`; strings use UTF-8. The browser API
holds each blob in memory and does not expose filesystem methods or native
workspace providers. Digests and action keys match the native SDK.

Same-origin tabs share data when they use the same `databaseName`. `close()`
releases this instance's connection without deleting data. `clear()` removes
both blobs and actions in that database, including data used by other tabs.
`forgetAction()` leaves blobs intact. Statistics count unique blobs and their
uncompressed payload bytes, plus JSON action-record bytes; they do not measure
IndexedDB's disk overhead. Browsers may evict storage, and quota or permission
failures reject with `StruoError`. Do not use a cache as the only copy of data.

## Browser execution

Import `TabExecutor` and `serveTab` from `@tuist/struo/browser/execution`.
Register named handlers in a worker tab, then dispatch tasks containing input
digests and an explicit memory estimate. Handlers read inputs and write outputs
through the shared IndexedDB cache. They can run browser-compatible compilers,
including Wasm compilers, but cannot launch native processes.

`OAuth2Client` implements public-client authorization-code login with S256 PKCE,
state validation, refresh and optional revocation. It keeps tokens in memory.
`GitLabExecutor` can dispatch and poll a pipeline, cancel it and retrieve bounded
artifacts into the browser cache. You must register a public OAuth application
and configure a GitLab project with a pipeline that performs your compilation.
Your provider must allow browser CORS requests. The SDK does not upload a
workspace or translate a Struo action into a pipeline.

See the [JavaScript SDK guide](https://struo.run/docs/guide/sdk/javascript#browser-execution)
for complete examples, cancellation semantics and security boundaries.

To build from a checkout, run `mise run sdk:build-browser` from the repository
root. This installs the matching Wasm tools and produces the browser assets.
Then run `npm ci`, `npx playwright install chromium --with-deps`, and
`npm run test:browser` from this package. Run `npm run test:types` to check the
browser types without Node.js globals.
