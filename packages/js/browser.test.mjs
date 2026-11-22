import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { chromium } from "playwright";
import { browserFixture } from "./test/browser-fixture.mjs";

let browser;
let fixture;
let origin;

before(async () => {
  fixture = await browserFixture();
  ({ browser, origin } = fixture);
});

after(async () => {
  await fixture?.close();
});

test("Wasm initialization, released digest compatibility, and ordered action keys", async () => {
  const page = await browser.newPage();
  try {
    await page.goto(origin);
    const result = await page.evaluate(async () => {
      const sdk = await import("/lib/browser.mjs");
      let uninitialized;
      try { sdk.digest("hello"); } catch (error) { uninitialized = error instanceof sdk.StruoError; }
      let failedInitialization;
      try {
        await sdk.init({ module_or_path: "/missing.wasm" });
      } catch { failedInitialization = true; }
      await Promise.all([sdk.init(), sdk.init()]);
      const key = new sdk.ActionKey("compile")
        .addBytes("tool", "swiftc")
        .addDigest("source", sdk.digest("source"));
      const golden = key.digest();
      const stable = key.digest() === golden;
      key.addBytes("mode", "release");
      const extended = key.digest();
      key.free();
      const reversed = new sdk.ActionKey("compile")
        .addDigest("source", sdk.digest("source"))
        .addBytes("tool", "swiftc");
      const reordered = reversed.digest();
      reversed.free();
      return {
        uninitialized, failedInitialization, golden, stable, extended, reordered,
        hello: sdk.digest("hello"),
        empty: sdk.digest(""),
        utf8: sdk.digest("é") === sdk.digest(new Uint8Array([0xc3, 0xa9])),
        subarray: sdk.digest(new Uint8Array([9, 104, 101, 108, 108, 111, 8]).subarray(1, 6)),
      };
    });
    assert.equal(result.uninitialized, true);
    assert.equal(result.failedInitialization, true);
    assert.equal(result.hello, "ea8f163db38682925e4491c5e58d4bb3506ef8c14eb78a86e908c5624a67200f");
    assert.equal(result.empty, "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262");
    assert.equal(result.golden, "c67ca7a97ebabdf6129cf60224eb10b9a235a4244bb023495bb4bd22ab9ad747");
    assert.equal(result.stable, true);
    assert.equal(result.utf8, true);
    assert.equal(result.subarray, result.hello);
    assert.notEqual(result.extended, result.golden);
    assert.notEqual(result.reordered, result.golden);
  } finally { await page.close(); }
});

test("IndexedDB deduplicates concurrent writes and preserves actions across tabs and reloads", async () => {
  const context = await browser.newContext();
  try {
    const writer = await context.newPage();
    const reader = await context.newPage();
    await Promise.all([writer.goto(origin), reader.goto(origin)]);
    const written = await writer.evaluate(async () => {
      const { init, Cache, digest } = await import("/lib/browser.mjs");
      await init();
      const cache = new Cache({ databaseName: "shared" });
      const view = new Uint8Array([99, 0, 255, 7, 99]).subarray(1, 4);
      const pending = cache.putBlob(view);
      view.fill(42);
      const binary = await pending;
      const repeated = await Promise.all(Array.from({ length: 8 }, () => cache.putBlob("hello")));
      const empty = await cache.putBlob("");
      const action = digest("action");
      const result = { exitCode: 7, stdout: repeated[0], stderr: null, outputs: { "output.bin": binary } };
      await cache.putActionResult(result, action);
      const stats = await cache.stats();
      await cache.close();
      return { binary, empty, action, result, stats, deduplicated: new Set(repeated).size === 1 };
    });
    assert.equal(written.deduplicated, true);
    assert.equal(written.stats.blobCount, 3);
    assert.equal(written.stats.blobBytes, 8);
    assert.equal(written.stats.actionCount, 1);
    assert.equal(written.stats.actionBytes, new TextEncoder().encode(JSON.stringify(written.result)).length);
    for (const page of [reader, writer]) {
      await page.reload();
      const read = await page.evaluate(async ({ binary, empty, action }) => {
        const { init, Cache } = await import("/lib/browser.mjs");
        await init();
        const cache = new Cache({ databaseName: "shared" });
        const isolated = new Cache({ databaseName: "isolated" });
        const data = {
          bytes: Array.from(await cache.getBlob(binary)),
          empty: (await cache.getBlob(empty)).length,
          action: await cache.getActionResult(action),
          exists: await cache.hasBlob(binary),
          isolated: await isolated.hasBlob(binary),
        };
        await Promise.all([cache.close(), isolated.close()]);
        return data;
      }, written);
      assert.deepEqual(read.bytes, [0, 255, 7]);
      assert.equal(read.empty, 0);
      assert.deepEqual(read.action, written.result);
      assert.equal(read.exists, true);
      assert.equal(read.isolated, false);
    }
  } finally { await context.close(); }
});

test("misses, deletion, reset, and invalid inputs preserve the cache contract", async () => {
  const page = await browser.newPage();
  try {
    await page.goto(origin);
    const result = await page.evaluate(async () => {
      const { init, Cache, digest, StruoError } = await import("/lib/browser.mjs");
      await init();
      const cache = new Cache();
      const missing = digest("absent");
      const failures = [];
      for (const run of [
        () => cache.getBlob(missing),
        () => cache.hasBlob("BAD"),
        () => cache.getActionResult("a".repeat(63)),
        () => cache.putActionResult({ exitCode: 0, stdout: "bad" }, missing),
      ]) {
        try { await run(); failures.push(false); }
        catch (error) { failures.push(error instanceof StruoError); }
      }
      let optionsRejected = false;
      try { new Cache({ workspaceRoot: "/repo" }); } catch (error) { optionsRejected = error instanceof TypeError; }
      const absent = await cache.getActionResult(missing);
      const blob = await cache.putBlob("keep me");
      await cache.putActionResult({ exitCode: -1 }, missing);
      const normalized = await cache.getActionResult(missing);
      const forgotten = await cache.forgetAction(missing);
      const twice = await cache.forgetAction(missing);
      const retained = await cache.hasBlob(blob);
      await cache.putActionResult({ exitCode: 0 }, missing);
      await cache.clear();
      const stats = await cache.stats();
      await cache.close();
      let closed;
      try { await cache.stats(); } catch (error) { closed = error instanceof StruoError; }
      return { failures, optionsRejected, absent, normalized, forgotten, twice, retained, stats, closed };
    });
    assert.deepEqual(result.failures, [true, true, true, true]);
    assert.equal(result.optionsRejected, true);
    assert.equal(result.absent, null);
    assert.deepEqual(result.normalized, { exitCode: -1, stdout: null, stderr: null, outputs: {} });
    assert.equal(result.forgotten, true);
    assert.equal(result.twice, false);
    assert.equal(result.retained, true);
    assert.deepEqual(result.stats, { blobCount: 0, blobBytes: 0, actionCount: 0, actionBytes: 0 });
    assert.equal(result.closed, true);
  } finally { await page.close(); }
});

test("a transaction aborted after a successful write rejects and leaves no blob", async () => {
  const page = await browser.newPage();
  try {
    await page.goto(origin);
    const result = await page.evaluate(async () => {
      const { init, Cache, digest, StruoError } = await import("/lib/browser.mjs");
      await init();
      const cache = new Cache();
      const original = IDBObjectStore.prototype.put;
      IDBObjectStore.prototype.put = function (...args) {
        const request = original.apply(this, args);
        const tx = this.transaction;
        request.addEventListener("success", () => tx.abort());
        return request;
      };
      let rejected = false;
      try {
        await cache.putBlob("aborted");
      } catch (error) {
        rejected = error instanceof StruoError;
      } finally {
        IDBObjectStore.prototype.put = original;
      }
      const exists = await cache.hasBlob(digest("aborted"));
      const next = await cache.putBlob("retry");
      const recovered = new TextDecoder().decode(await cache.getBlob(next));
      await cache.close();
      return { rejected, exists, recovered };
    });
    assert.deepEqual(result, { rejected: true, exists: false, recovered: "retry" });
  } finally { await page.close(); }
});

test("IndexedDB survives a browser process restart", async () => {
  const profile = await mkdtemp(join(tmpdir(), "struo-browser-"));
  let context;
  const options = { executablePath: process.env.CHROMIUM_PATH || undefined, args: ["--no-sandbox"] };
  try {
    context = await chromium.launchPersistentContext(profile, options);
    let page = await context.newPage();
    await page.goto(origin);
    const blob = await page.evaluate(async () => {
      const { init, Cache } = await import("/lib/browser.mjs");
      await init();
      const cache = new Cache({ databaseName: "restart" });
      const blob = await cache.putBlob("retained after process exit");
      await cache.close();
      return blob;
    });
    await context.close();
    context = await chromium.launchPersistentContext(profile, options);
    page = await context.newPage();
    await page.goto(origin);
    assert.equal(await page.evaluate(async (blob) => {
      const { init, Cache } = await import("/lib/browser.mjs");
      await init();
      const cache = new Cache({ databaseName: "restart" });
      const output = new TextDecoder().decode(await cache.getBlob(blob));
      await cache.close();
      return output;
    }, blob), "retained after process exit");
  } finally { await context?.close(); await rm(profile, { recursive: true, force: true }); }
});
