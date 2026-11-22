import { ActionKey, Cache, digest, init, type ActionResult } from "@tuist/struo/browser";
import { Cache as BrowserCache } from "@tuist/struo";
import { TabExecutor, serveTab, OAuth2Client, GitLabExecutor } from "@tuist/struo/browser/execution";

await init();
const selected: Cache = new BrowserCache({ databaseName: "conditional-export" });
await selected.close();
const cache = new Cache({ databaseName: "example" });
const blob: string = await cache.putBlob(new Uint8Array([1, 2, 3]));
const bytes: Uint8Array = await cache.getBlob(blob);
const key = new ActionKey("example").addBytes("input", bytes).addDigest("blob", digest(bytes));
const result: ActionResult = { exitCode: 0, stdout: blob };
await cache.putActionResult(result, key.digest());
key.free();
await cache.clear();
await cache.close();

// @ts-expect-error Browser caches do not accept filesystem roots.
new Cache({ localCacheRoot: "/tmp" });
// @ts-expect-error Browser caches do not materialize filesystem paths.
cache.putFile("/tmp/input");

const worker = serveTab({ memoryLimitBytes: 1024, handlers: { echo: async () => ({ exitCode: 0 }) } });
const executor = new TabExecutor();
await executor.execute({ operation: "echo", inputs: {}, args: [], memoryBytes: 1024 });
executor.close();
worker.close();
const auth = new OAuth2Client({
  clientId: "public-client", redirectUri: `${location.origin}/callback`, scopes: ["api"],
  authorizationEndpoint: "https://gitlab.com/oauth/authorize", tokenEndpoint: "https://gitlab.com/oauth/token",
});
const remote = new GitLabExecutor({ project: "example/compiler", auth });
await remote.get(123);
