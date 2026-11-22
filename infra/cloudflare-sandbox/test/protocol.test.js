import assert from "node:assert/strict";
import { test } from "node:test";
import { spawn, execFileSync } from "node:child_process";
import { mkdtemp, mkdir, writeFile, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Readable, Writable } from "node:stream";
import { route, Session } from "../src/protocol.js";

function fixture() {
  const data = new Map();
  const files = new Map();
  const calls = [];
  const storage = {
    get: async (key) => structuredClone(data.get(key)),
    put: async (key, value) => { data.set(key, structuredClone(value)); },
    delete: async (key) => { data.delete(key); },
    setAlarm: async (time) => { data.set("alarm", time); },
    deleteAlarm: async () => { data.delete("alarm"); },
  };
  const container = {
    running: false, images: { pinned: "registry/image@sha256:123" },
    start(config) { this.running = true; calls.push(config); },
    async setInactivityTimeout() {},
    async destroy() { this.running = false; },
    async exec(argv, options = {}) {
      calls.push({ argv, options });
      if (argv[0] === "tee") {
        const chunks = argv[1] === "-a" ? [files.get(argv.at(-1)) ?? new Uint8Array()] : [];
        let finish;
        const exitCode = new Promise((resolve) => { finish = resolve; });
        const stdin = new WritableStream({
          write(chunk) { chunks.push(chunk); },
          close() { files.set(argv.at(-1), Buffer.concat(chunks)); finish(0); },
          abort() { finish(1); },
        });
        return { stdin, exitCode };
      }
      const bytes = (value) => new Blob([value]).stream();
      if (argv[0] === "cat") return { stdout: bytes(files.get(argv[1]) ?? ""), exitCode: Promise.resolve(files.has(argv[1]) ? 0 : 1) };
      if (argv[0] === "user-command") return { stdout: bytes("out"), stderr: bytes("error"), exitCode: Promise.resolve(7) };
      return { exitCode: Promise.resolve(0) };
    },
  };
  const ctx = { storage, container };
  const session = new Session(ctx);
  const env = { BRIDGE_TOKEN: "test-token", SANDBOXES: { getByName: () => ({ fetch: (request) => session.fetch(request) }) } };
  const request = (path, method = "GET", value, token = "test-token") => route(new Request(`https://bridge/v1/sandboxes${path}`, {
    method, headers: { Authorization: `Bearer ${token}` },
    ...(value === undefined ? {} : { body: typeof value === "string" || value instanceof Uint8Array ? value : JSON.stringify(value) }),
  }), env);
  const create = (extra = {}) => request("", "POST", { marker: "owned", image: "pinned", cpu: 2, memoryBytes: 536870912, maxArchiveBytes: 1024, maxCaptureBytes: 100, ...extra });
  const ready = async (extra = {}) => {
    assert.equal((await create(extra)).status, 200);
    assert.equal((await request("/owned/inputs/0", "PUT", "archive")).status, 204);
    assert.equal((await request("/owned/stage", "POST", {})).status, 200);
  };
  return { request, create, ready, data, files, calls, container, session };
}

test("authentication precedes provisioning and image selection is allowlisted", async () => {
  const f = fixture();
  assert.equal((await f.request("", "POST", {}, "wrong")).status, 401);
  assert.equal((await f.create({ image: "untrusted/image" })).status, 400);
  assert.equal(f.container.running, false);
  assert.equal((await f.create()).status, 200);
  assert.equal(f.calls[0].image, "registry/image@sha256:123");
  assert.equal(f.calls[0].instance.vcpu, 2);
  assert.equal(f.calls[0].instance.memoryMib, 512);
  assert.ok(f.data.get("alarm") > Date.now());
});

test("chunk offsets, argv, separate binary captures, exit code and explicit cleanup", async () => {
  const f = fixture();
  await f.ready();
  assert.equal((await f.request("/owned/inputs/0", "PUT", "replay")).status, 409);
  const command = { argv: ["user-command", "a b", "$(bad)"], cwd: "/workspace", env: { EXAMPLE: "value" } };
  const response = await f.request("/owned/exec", "POST", command);
  assert.deepEqual(await response.json(), { exitCode: 7 });
  assert.deepEqual(f.calls.find((call) => call.argv?.[0] === "user-command").argv, command.argv);
  assert.equal(await (await f.request("/owned/stdout")).text(), "out");
  assert.equal(await (await f.request("/owned/stderr")).text(), "error");
  assert.equal((await f.request("/owned/exec", "POST", command)).status, 409);
  assert.equal((await f.request("/owned", "DELETE")).status, 204);
  assert.equal(f.container.running, false);
  assert.equal(f.data.get("closed"), true);
  assert.equal((await f.create()).status, 409);
  assert.deepEqual(await (await f.request("?marker=owned")).json(), []);
});

test("oversized capture destroys the whole sandbox, including child processes", async () => {
  const f = fixture();
  await f.ready({ maxCaptureBytes: 7 });
  assert.equal((await f.request("/owned/exec", "POST", { argv: ["user-command"], cwd: "/workspace", env: {} })).status, 502);
  assert.equal(f.container.running, false);
  assert.equal(f.data.get("closed"), true);
});

test("failed destruction remains discoverable and retains an alarm until confirmed", async () => {
  const f = fixture();
  await f.ready();
  f.container.destroy = async () => { throw new Error("unavailable"); };
  assert.equal((await f.request("/owned", "DELETE")).status, 502);
  assert.deepEqual(await (await f.request("?marker=owned")).json(), [{ id: "owned", marker: "owned" }]);
  assert.ok(f.data.get("alarm"));
  f.container.destroy = async () => { f.container.running = false; };
  await f.session.destroy();
  assert.equal(f.data.has("record"), false);
});

test("an absent ownership lookup fences a delayed create after a lost response", async () => {
  const f = fixture();
  assert.deepEqual(await (await f.request("?marker=owned")).json(), []);
  assert.equal((await f.create()).status, 409);
  assert.equal(f.container.running, false);
});

test("output path traversal is rejected and missing file streams fail closed", async () => {
  const f = fixture();
  await f.ready();
  await f.request("/owned/exec", "POST", { argv: ["user-command"], cwd: "/workspace", env: {} });
  assert.equal((await f.request("/owned/outputs", "POST", { paths: ["../escape"] })).status, 400);
  f.files.delete("/tmp/struo-stdout");
  await assert.rejects((await f.request("/owned/stdout")).arrayBuffer());
});

test("local process-backed bridge roundtrips a dependency archive and declared outputs", async () => {
  const root = await mkdtemp(join(tmpdir(), "struo-cloudflare-"));
  const f = fixture();
  const active = new Set();
  try {
    await mkdir(join(root, "source/node_modules"), { recursive: true });
    await writeFile(join(root, "source/node_modules/dependency.cjs"), "module.exports = 42;");
    const archive = join(root, "inputs.tar");
    execFileSync("tar", ["-cf", archive, "-C", join(root, "source"), "node_modules"]);
    f.container.exec = async (argv, options = {}) => {
      const translate = (word) => word.replace(/^\/workspace(?=\/|$)/, join(root, "workspace"))
        .replace(/^\/tmp\/struo-/, join(root, "struo-"));
      const child = spawn(argv[0], argv.slice(1).map(translate), {
        cwd: options.cwd ? translate(options.cwd) : root,
        env: { PATH: process.env.PATH, ...options.env },
        stdio: [options.stdin ? "pipe" : "ignore", options.stdout === "ignore" ? "ignore" : "pipe", options.stderr === "ignore" ? "ignore" : "pipe"],
      });
      active.add(child);
      const exitCode = new Promise((resolve, reject) => {
        child.once("error", reject);
        child.once("close", (code) => { active.delete(child); resolve(code); });
      });
      return { exitCode, stdin: child.stdin && Writable.toWeb(child.stdin), stdout: child.stdout && Readable.toWeb(child.stdout), stderr: child.stderr && Readable.toWeb(child.stderr) };
    };
    assert.equal((await f.create({ maxArchiveBytes: 1024 * 1024 })).status, 200);
    const bytes = await readFile(archive);
    // Two uploads exercise append offsets independently of HTTP request limits.
    assert.equal((await f.request("/owned/inputs/0", "PUT", bytes.subarray(0, 512))).status, 204);
    assert.equal((await f.request("/owned/inputs/512", "PUT", bytes.subarray(512))).status, 204);
    assert.equal((await f.request("/owned/stage", "POST", {})).status, 200);
    const code = "const fs=require('fs'); const value=require('./node_modules/dependency.cjs'); fs.mkdirSync('reports'); fs.writeFileSync('reports/result.txt', String(value)); process.stdout.write('out'); process.stderr.write('err'); process.exitCode=7;";
    assert.deepEqual(await (await f.request("/owned/exec", "POST", { argv: [process.execPath, "-e", code], cwd: "/workspace", env: {} })).json(), { exitCode: 7 });
    assert.equal(await (await f.request("/owned/stdout")).text(), "out");
    assert.equal(await (await f.request("/owned/stderr")).text(), "err");
    assert.equal((await f.request("/owned/outputs", "POST", { paths: ["reports"] })).status, 200);
    const output = new Uint8Array(await (await f.request("/owned/outputs")).arrayBuffer());
    await writeFile(join(root, "outputs.tar"), output);
    assert.equal(execFileSync("tar", ["-xOf", join(root, "outputs.tar"), "./reports/result.txt"]).toString(), "42");
    assert.equal((await f.request("/owned", "DELETE")).status, 204);
  } finally {
    for (const child of active) child.kill("SIGKILL");
    await rm(root, { recursive: true, force: true });
  }
});
