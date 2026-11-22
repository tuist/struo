import assert from "node:assert/strict";
import { before, after, test } from "node:test";
import { browserFixture } from "./test/browser-fixture.mjs";
import { oauthProvider } from "./test/oauth-provider.mjs";

const wasm = [0,97,115,109,1,0,0,0,1,5,1,96,0,1,127,3,2,1,0,7,10,1,6,97,110,115,119,101,114,0,0,10,6,1,4,0,65,42,11];
let fixture;
let provider;
before(async () => { fixture = await browserFixture(); provider = await oauthProvider(fixture.origin); });
after(async () => { await provider?.close(); await fixture?.close(); });

test("two worker tabs compile once, exchange only digests, and preserve the output after reload", async () => {
  const context = await fixture.browser.newContext();
  try {
    const client = await context.newPage();
    const workers = await Promise.all([context.newPage(), context.newPage()]);
    await Promise.all([client, ...workers].map((page) => page.goto(fixture.origin)));
    for (const worker of workers) {
      await worker.evaluate(async () => {
        const { init, Cache } = await import("/lib/browser.mjs");
        const { serveTab } = await import("/lib/execution.mjs");
        await init();
        const cache = new Cache({ databaseName: "compilation" });
        window.calls = 0;
        window.worker = serveTab({ memoryLimitBytes: 1024, handlers: {
          compile: async ({ inputs }) => {
            window.calls++;
            const module = await WebAssembly.compile(await cache.getBlob(inputs.source));
            const instance = await WebAssembly.instantiate(module);
            return { exitCode: 0, outputs: { "answer.txt": await cache.putBlob(`${instance.exports.answer()}\n`) } };
          },
        } });
      });
    }
    const written = await client.evaluate(async (payload) => {
      const { init, Cache, ActionKey } = await import("/lib/browser.mjs");
      const { TabExecutor } = await import("/lib/execution.mjs");
      await init();
      const cache = new Cache({ databaseName: "compilation" });
      const source = await cache.putBlob(new Uint8Array(payload));
      const key = new ActionKey("wasm-compile.v1").addDigest("source", source);
      const executor = new TabExecutor();
      const { result, workerId } = await executor.execute({ operation: "compile", inputs: { source }, args: [], memoryBytes: 1024 });
      await cache.putActionResult(result, key.digest());
      const output = new TextDecoder().decode(await cache.getBlob(result.outputs["answer.txt"]));
      const action = key.digest();
      key.free(); executor.close(); await cache.close();
      return { output, workerId, action };
    }, wasm);
    assert.equal(written.output, "42\n");
    const calls = await Promise.all(workers.map((page) => page.evaluate(() => window.calls)));
    assert.equal(calls.reduce((a, b) => a + b, 0), 1);
    const identities = await Promise.all(workers.map((page) => page.evaluate(() => window.worker.workerId)));
    assert(identities.includes(written.workerId));
    await client.reload();
    assert.equal(await client.evaluate(async (action) => {
      const { init, Cache } = await import("/lib/browser.mjs");
      await init();
      const cache = new Cache({ databaseName: "compilation" });
      const result = await cache.getActionResult(action);
      const output = new TextDecoder().decode(await cache.getBlob(result.outputs["answer.txt"]));
      await cache.close();
      return output;
    }, written.action), "42\n");
  } finally { await context.close(); }
});

test("budgets retain cancelled work until it stops; unsupported operations and absent workers time out", async () => {
  const context = await fixture.browser.newContext();
  try {
    const worker = await context.newPage();
    const client = await context.newPage();
    await Promise.all([worker.goto(fixture.origin), client.goto(fixture.origin)]);
    await worker.evaluate(async () => {
      const { serveTab } = await import("/lib/execution.mjs");
      window.calls = 0;
      window.stopped = false;
      window.worker = serveTab({ channelName: "budget", memoryLimitBytes: 100, handlers: {
        slow: async (_task, { signal }) => {
          window.calls++;
          await new Promise((resolve) => { window.release = resolve; });
          window.stopped = signal.aborted;
          return { exitCode: 0 };
        },
      } });
    });
    await client.evaluate(() => {
      window.task = { operation: "slow", inputs: {}, args: [], memoryBytes: 100 };
    });
    const first = client.evaluate(async () => {
      const { TabExecutor } = await import("/lib/execution.mjs");
      window.executor = new TabExecutor({ channelName: "budget" });
      window.abort = new AbortController();
      try { await window.executor.execute(window.task, { signal: window.abort.signal }); return false; }
      catch (error) { return error.name === "AbortError"; }
    });
    await worker.waitForFunction(() => window.calls === 1);
    await client.evaluate(() => window.abort.abort());
    assert.equal(await first, true);
    assert.equal(await client.evaluate(async () => {
      try { await window.executor.execute(window.task, { timeoutMs: 100 }); return false; }
      catch (error) { return error.message.includes("timed out"); }
    }), true);
    assert.equal(await worker.evaluate(() => window.calls), 1);
    await worker.evaluate(() => window.release());
    await worker.waitForFunction(() => window.stopped);
    const next = client.evaluate(() => window.executor.execute(window.task));
    await worker.waitForFunction(() => window.calls === 2);
    await worker.evaluate(() => window.release());
    assert.equal((await next).result.exitCode, 0);
    assert.equal(await client.evaluate(async () => {
      try { await window.executor.execute({ ...window.task, operation: "unknown" }, { timeoutMs: 100 }); return false; }
      catch (error) { return error.message.includes("timed out"); }
    }), true);
    await worker.evaluate(() => window.worker.close());
    assert.equal(await client.evaluate(async () => {
      try { await window.executor.execute(window.task, { timeoutMs: 100 }); return false; }
      catch (error) { window.executor.close(); return error.message.includes("timed out"); }
    }), true);
  } finally { await context.close(); }
});

async function login(page, query = "") {
  await page.goto(fixture.origin);
  const authorizationURL = await page.evaluate(async ({ url, query }) => {
    const { OAuth2Client } = await import("/lib/execution.mjs");
    window.config = {
      authorizationEndpoint: `${url}/oauth/authorize`, tokenEndpoint: `${url}/oauth/token${query}`,
      revocationEndpoint: `${url}/oauth/revoke`, clientId: "test-client", redirectUri: `${location.origin}/callback`, scopes: ["api"],
    };
    window.auth = new OAuth2Client(window.config);
    sessionStorage.setItem("test-config", JSON.stringify(window.config));
    return window.auth.authorizationURL();
  }, { url: provider.url, query });
  await page.goto(authorizationURL);
  const callback = page.url();
  await page.evaluate(async () => {
    const { OAuth2Client } = await import("/lib/execution.mjs");
    window.auth = new OAuth2Client(JSON.parse(sessionStorage.getItem("test-config")));
    await window.auth.completeAuthorization();
  });
  return callback;
}

test("real browser redirect, S256 exchange, refresh, remote compilation, artifact CAS and logout against a protocol fixture", async () => {
  const page = await fixture.browser.newPage();
  try {
    const start = provider.events.length;
    await login(page, "?short=1");
    assert.equal(new URL(page.url()).search, "");
    const result = await page.evaluate(async ({ url, payload }) => {
      const { init, Cache } = await import("/lib/browser.mjs");
      const { GitLabExecutor } = await import("/lib/execution.mjs");
      await init();
      const cache = new Cache();
      const remote = new GitLabExecutor({ url, project: "example/compiler", auth: window.auth });
      const pipeline = await remote.execute({ ref: "main", variables: { WASM_BASE64: btoa(String.fromCharCode(...payload)) } }, { pollIntervalMs: 10 });
      const jobs = await remote.jobs(pipeline.id);
      const blob = await remote.storeArtifact({ jobId: jobs[0].id, path: "answer.txt", cache, maxBytes: 16 });
      const output = new TextDecoder().decode(await cache.getBlob(blob));
      let limited = false;
      try { await remote.storeArtifact({ jobId: jobs[0].id, path: "answer.txt", cache, maxBytes: 2 }); }
      catch (error) { limited = error.message.includes("byte limit"); }
      const persistedTokens = Object.values(sessionStorage).some((value) => value.includes("access_token") || value.includes("refresh_token"));
      await window.auth.logout();
      let loggedOut = false;
      try { await remote.get(pipeline.id); } catch { loggedOut = true; }
      await cache.close();
      return { status: pipeline.status, output, limited, persistedTokens, loggedOut };
    }, { url: provider.url, payload: wasm });
    assert.deepEqual(result, { status: "success", output: "42\n", limited: true, persistedTokens: false, loggedOut: true });
    const events = provider.events.slice(start);
    for (const event of ["authorize", "exchange:S256-verified", "refresh", "compile:answer=42", "revoke"]) assert(events.includes(event), event);
  } finally { await page.close(); }
});

test("OAuth rejects state substitution and callback replay; remote timeout cancels the owned pipeline", async () => {
  const page = await fixture.browser.newPage();
  try {
    const callback = await login(page);
    assert.equal(await page.evaluate(async (callback) => {
      try { await window.auth.completeAuthorization(callback); return false; }
      catch { return true; }
    }, callback), true);
    const result = await page.evaluate(async (url) => {
      const { OAuth2Client, GitLabExecutor } = await import("/lib/execution.mjs");
      const auth = window.auth;
      const remote = new GitLabExecutor({ url, project: "example/compiler", auth });
      let timeout = false;
      try { await remote.execute({ ref: "pending" }, { timeoutMs: 150, pollIntervalMs: 10 }); }
      catch (error) { timeout = error.name === "TimeoutError"; }
      const challenger = new OAuth2Client(JSON.parse(sessionStorage.getItem("test-config")));
      await challenger.authorizationURL();
      let stateRejected = false;
      try { await challenger.completeAuthorization(`${location.origin}/callback?code=stolen&state=wrong`); }
      catch (error) { stateRejected = error.message.includes("state"); }
      return { timeout, stateRejected };
    }, provider.url);
    assert.deepEqual(result, { timeout: true, stateRejected: true });
    assert(provider.events.some((event) => event.startsWith("cancel:")));
    assert.equal(await page.evaluate(async () => {
      try { await window.auth.completeAuthorization(`${location.origin}/callback?code=replay&state=wrong`); return false; }
      catch { return true; }
    }), true);
  } finally { await page.close(); }
});

test("OAuth refresh is coalesced and cannot restore a logged-out session", async () => {
  const page = await fixture.browser.newPage();
  try {
    await login(page, "?short=1&delay=1");
    const start = provider.events.length;
    assert.equal(await page.evaluate(async () => {
      const tokens = await Promise.all([window.auth.getAccessToken(), window.auth.getAccessToken()]);
      return tokens[0] === tokens[1];
    }), true);
    assert.equal(provider.events.slice(start).filter((event) => event === "refresh").length, 1);
    await login(page, "?short=1&delay=1");
    const result = await page.evaluate(async () => {
      const refresh = window.auth.getAccessToken().then(() => false, () => true);
      await window.auth.logout();
      const rejected = await refresh;
      let denied = false;
      try { await window.auth.getAccessToken(); } catch { denied = true; }
      return { rejected, denied };
    });
    assert.deepEqual(result, { rejected: true, denied: true });
  } finally { await page.close(); }
});

test("OAuth rejects denied consent, expired callbacks and a substituted PKCE verifier", async () => {
  const page = await fixture.browser.newPage();
  try {
    await login(page);
    const denied = await page.evaluate(async () => {
      const url = new URL(await window.auth.authorizationURL());
      try { await window.auth.completeAuthorization(`${location.origin}/callback?error=access_denied&state=${url.searchParams.get("state")}`); }
      catch (error) { return error.message.includes("denied"); }
      return false;
    });
    assert.equal(denied, true);
    assert.equal(await page.evaluate(async () => {
      const url = new URL(await window.auth.authorizationURL());
      const key = Object.keys(sessionStorage).find((key) => key.startsWith("struo.oauth2:"));
      const pending = JSON.parse(sessionStorage.getItem(key));
      pending.createdAt -= 600_001;
      sessionStorage.setItem(key, JSON.stringify(pending));
      try { await window.auth.completeAuthorization(`${location.origin}/callback?code=expired&state=${url.searchParams.get("state")}`); }
      catch (error) { return error.message.includes("expiry"); }
      return false;
    }), true);
    const url = await page.evaluate(() => window.auth.authorizationURL());
    const callback = (await page.request.get(url)).url();
    assert.equal(await page.evaluate(async (callback) => {
      const key = Object.keys(sessionStorage).find((key) => key.startsWith("struo.oauth2:"));
      const pending = JSON.parse(sessionStorage.getItem(key));
      pending.verifier = "substituted";
      sessionStorage.setItem(key, JSON.stringify(pending));
      try { await window.auth.completeAuthorization(callback); }
      catch (error) { return error.message.includes("HTTP 400"); }
      return false;
    }, callback), true);
  } finally { await page.close(); }
});

test("an older refresh cannot clear tokens from a newer login", async () => {
  const page = await fixture.browser.newPage();
  const resume = provider.pauseRefresh();
  try {
    await login(page, "?short=1");
    const request = page.waitForRequest((request) => request.url().includes("/oauth/token") && request.postData()?.includes("grant_type=refresh_token"));
    const refresh = page.evaluate(() => window.auth.getAccessToken().then(() => false, () => true));
    await request;
    const url = await page.evaluate(() => window.auth.authorizationURL());
    const callback = (await page.request.get(url)).url();
    await page.evaluate((callback) => window.auth.completeAuthorization(callback), callback);
    resume();
    assert.equal(await refresh, true);
    assert.equal(await page.evaluate(async () => typeof await window.auth.getAccessToken() === "string"), true);
  } finally { resume(); await page.close(); }
});

test("concurrent tab requests use both workers without dropping or duplicating tasks", async () => {
  const context = await fixture.browser.newContext();
  try {
    const client = await context.newPage();
    const workers = await Promise.all([context.newPage(), context.newPage()]);
    await Promise.all([client, ...workers].map((page) => page.goto(fixture.origin)));
    for (const page of workers) await page.evaluate(async () => {
      const { serveTab } = await import("/lib/execution.mjs");
      window.calls = [];
      window.worker = serveTab({ channelName: "parallel", memoryLimitBytes: 100, handlers: {
        run: async ({ args }) => {
          window.calls.push(args[0]);
          await new Promise((resolve) => setTimeout(resolve, 40));
          return { exitCode: Number(args[0]) };
        },
      } });
    });
    const outcomes = await client.evaluate(async () => {
      const { TabExecutor } = await import("/lib/execution.mjs");
      const executor = new TabExecutor({ channelName: "parallel" });
      try {
        return await Promise.all([7, 19].map((code) => executor.execute({
          operation: "run", inputs: {}, args: [String(code)], memoryBytes: 100,
        }, { timeoutMs: 700 }).then(({ result }) => result.exitCode, () => "timeout")));
      } finally { executor.close(); }
    });
    assert.deepEqual(outcomes, [7, 19]);
    const calls = await Promise.all(workers.map((page) => page.evaluate(() => window.calls)));
    assert.deepEqual(calls.flat().sort(), ["19", "7"]);
    assert.deepEqual(calls.map((calls) => calls.length), [1, 1]);
  } finally { await context.close(); }
});

test("tab cancellation rejects even when the abort reason is falsy", async () => {
  const page = await fixture.browser.newPage();
  try {
    await page.goto(fixture.origin);
    assert.deepEqual(await page.evaluate(async () => {
      const { TabExecutor } = await import("/lib/execution.mjs");
      const executor = new TabExecutor({ channelName: "falsy" });
      try {
        const results = [];
        for (const reason of [null, false, 0, ""]) {
          const controller = new AbortController();
          const pending = executor.execute({ operation: "absent", inputs: {}, args: [], memoryBytes: 1 }, { signal: controller.signal });
          controller.abort(reason);
          results.push(await pending.then(() => false, (error) => Object.is(error, reason)));
        }
        return results;
      } finally { executor.close(); }
    }), [true, true, true, true]);
  } finally { await page.close(); }
});

test("GitLab aborts and deadlines settle before authentication and leave shared authentication usable", async () => {
  const page = await fixture.browser.newPage();
  try {
    await page.goto(fixture.origin);
    const result = await page.evaluate(async () => {
      const { GitLabExecutor } = await import("/lib/execution.mjs");
      let release;
      let authCalls = 0;
      const authentication = new Promise((resolve) => { release = resolve; });
      const auth = { getAccessToken: () => { authCalls++; return authentication; } };
      const remote = new GitLabExecutor({ project: "example/compiler", auth });
      const controller = new AbortController();
      const pending = remote.get(1, { signal: controller.signal }).then(() => "resolved", (error) => error.name);
      controller.abort();
      const deadline = remote.execute({ ref: "main" }, { timeoutMs: 30 })
        .then(() => "resolved", (error) => error.cause?.name ?? error.name);
      const results = await Promise.race([
        Promise.all([pending, deadline]),
        new Promise((resolve) => setTimeout(() => resolve(["still pending"]), 150)),
      ]);
      release("test-token");
      const token = await auth.getAccessToken();
      const preAborted = new AbortController();
      preAborted.abort();
      const before = authCalls;
      await remote.get(1, { signal: preAborted.signal }).catch(() => {});
      return { results, token, preAbortedSkippedAuth: authCalls === before };
    });
    assert.deepEqual(result, { results: ["AbortError", "TimeoutError"], token: "test-token", preAbortedSkippedAuth: true });
  } finally { await page.close(); }
});

test("logout waits for refresh and revokes the replacement token at the provider", async () => {
  const page = await fixture.browser.newPage();
  const resume = provider.pauseRefresh();
  try {
    await login(page, "?short=1");
    const response = page.waitForResponse((response) => response.url().includes("/oauth/token"));
    await page.evaluate(() => {
      window.refreshed = window.auth.getAccessToken().then(() => false, () => true);
    });
    await resume.started;
    await page.evaluate(() => {
      window.loggedOut = false;
      window.logout = window.auth.logout().then(() => { window.loggedOut = true; });
    });
    const before = await page.evaluate(() => window.loggedOut);
    resume();
    const tokens = await (await response).json();
    await page.evaluate(() => window.logout);
    assert.equal(before, false);
    assert.equal(await page.evaluate(() => window.refreshed), true);
    const check = await page.request.get(`${provider.url}/api/v4/projects/example%2Fcompiler/pipelines/1`, {
      headers: { Authorization: `Bearer ${tokens.access_token}` },
    });
    assert.equal(check.status(), 401);
  } finally { resume(); await page.close(); }
});
