const HOUR = 3_600_000;
const CHUNK = 8 * 1024 * 1024;
const MARKER = /^[a-zA-Z0-9-]{1,64}$/;
const json = (value) => Response.json(value);
const fail = (status) => { throw new Response(null, { status }); };

async function metadata(request) {
  const bytes = await new Response(request.body?.pipeThrough(bounded(64 * 1024))).arrayBuffer();
  try { return JSON.parse(new TextDecoder().decode(bytes)); }
  catch { fail(400); }
}

export function bounded(limit, count = { bytes: 0 }) {
  return new TransformStream({
    transform(chunk, controller) {
      count.bytes += chunk.byteLength;
      if (count.bytes > limit) throw new Error("transfer limit exceeded");
      controller.enqueue(chunk);
    },
  });
}

export async function route(request, env) {
  // Hash both sides to compare fixed-length values, without exposing the secret.
  const digest = async (value) => new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(value)));
  const expected = await digest(`Bearer ${env.BRIDGE_TOKEN}`);
  const actual = await digest(request.headers.get("Authorization") ?? "");
  let difference = 0;
  for (let i = 0; i < expected.length; i++) difference |= expected[i] ^ actual[i];
  if (!env.BRIDGE_TOKEN || difference) return new Response(null, { status: 401 });
  try {
    const url = new URL(request.url);
    if (url.pathname === "/v1/sandboxes") {
      const body = request.method === "POST" ? await metadata(request) : null;
      if (!body && request.method !== "GET") fail(405);
      const marker = body?.marker ?? url.searchParams.get("marker");
      if (!MARKER.test(marker ?? "")) fail(400);
      const stub = env.SANDBOXES.getByName(marker);
      return body
        ? stub.fetch(new Request("https://sandbox/create", { method: "POST", body: JSON.stringify(body) }))
        : stub.fetch(new Request(`https://sandbox/inspect?marker=${marker}`));
    }
    const match = url.pathname.match(/^\/v1\/sandboxes\/([a-zA-Z0-9-]{1,64})(\/.*)?$/);
    if (!match) fail(404);
    return env.SANDBOXES.getByName(match[1]).fetch(new Request(`https://sandbox${match[2] ?? "/"}`, request));
  } catch (error) {
    return error instanceof Response ? error : new Response(null, { status: 502 });
  }
}

export class Session {
  constructor(ctx) { this.ctx = ctx; this.busy = false; }

  async destroy() {
    // Keep a tombstone: a delayed create can never resurrect a cancelled nonce.
    await this.ctx.storage.put("closed", true);
    await this.ctx.storage.setAlarm(Date.now() + 60_000);
    await this.ctx.container.destroy();
    await this.ctx.storage.delete("record");
    await this.ctx.storage.deleteAlarm();
  }

  async fetch(request) {
    const path = new URL(request.url).pathname;
    if (request.method === "DELETE" && path === "/") {
      try { await this.destroy(); return new Response(null, { status: 204 }); }
      catch { return new Response(null, { status: 502 }); }
    }
    if (this.busy) return new Response(null, { status: 409 });
    this.busy = true;
    try {
      return await this.handle(request, path);
    } catch (error) {
      if (!(error instanceof Response)) {
        try { await this.destroy(); } catch { /* The alarm retries destruction. */ }
      }
      return error instanceof Response ? error : new Response(null, { status: 502 });
    } finally { this.busy = false; }
  }

  async handle(request, path) {
    const storage = this.ctx.storage;
    const container = this.ctx.container;
    const closed = await storage.get("closed");
    let record = await storage.get("record");
    if (path === "/inspect" && request.method === "GET") {
      // Proving absence also fences a create request that might arrive late.
      if (!record) await storage.put("closed", true);
      return json(record ? [{ id: record.marker, marker: record.marker }] : []);
    }
    if (path === "/create" && request.method === "POST") {
      if (closed || record) fail(409);
      const config = await metadata(request);
      if (!MARKER.test(config.marker ?? "")) fail(400);
      const image = config.image === "cloudflare/debian-trixie"
        ? config.image : Object.hasOwn(container.images, config.image) ? container.images[config.image] : null;
      if (!image) fail(400);
      for (const field of ["cpu", "memoryBytes", "maxArchiveBytes", "maxCaptureBytes"]) {
        if (!Number.isSafeInteger(config[field]) || config[field] < (field === "cpu" ? 1 : 0)) fail(400);
      }
      record = { ...config, offset: 0, phase: "upload", expires: Date.now() + HOUR };
      await storage.put("record", record);
      await storage.setAlarm(record.expires);
      // A deletion may have arrived while the durable create intent was written.
      if (await storage.get("closed")) fail(409);
      container.start({ image, entrypoint: ["sleep", "infinity"], enableInternet: true,
        instance: { vcpu: config.cpu, memoryMib: Math.max(256, Math.ceil(config.memoryBytes / 1048576)), diskMb: 10000 },
        labels: { "struo-operation": config.marker } });
      await container.setInactivityTimeout(HOUR);
      await this.checked(["mkdir", "-p", "/workspace"]);
      return json({ id: config.marker });
    }
    if (!record || closed || Date.now() >= record.expires || !container.running) fail(410);
    if (request.method === "PUT" && path.startsWith("/inputs/")) {
      if (record.phase !== "upload" || path !== `/inputs/${record.offset}`) fail(409);
      const count = { bytes: 0 };
      const stream = request.body?.pipeThrough(bounded(Math.min(CHUNK, record.maxArchiveBytes - record.offset), count));
      if (!stream) fail(400);
      await this.checked(["tee", "-a", "/tmp/struo-inputs.tar"], { stdin: stream });
      record.offset += count.bytes;
      await storage.put("record", record);
      return new Response(null, { status: 204 });
    }
    if (path === "/stage" && request.method === "POST") {
      if (record.phase !== "upload" || !record.offset) fail(409);
      await this.checked(["tar", "-xf", "/tmp/struo-inputs.tar", "-C", "/workspace"]);
      record.phase = "ready";
      await storage.put("record", record);
      return json({});
    }
    if (path === "/exec" && request.method === "POST") {
      if (record.phase !== "ready") fail(409);
      const command = await metadata(request);
      if (!Array.isArray(command.argv) || !command.argv.length || command.argv.some((v) => typeof v !== "string")) fail(400);
      if (typeof command.cwd !== "string" || (command.cwd !== "/workspace" && !command.cwd.startsWith("/workspace/"))) fail(400);
      record.phase = "executing";
      await storage.put("record", record);
      const process = await container.exec(command.argv, { cwd: command.cwd, env: command.env });
      const count = { bytes: 0 };
      // Drain both channels concurrently, with one capture allowance and disk-backed storage.
      await Promise.all([[process.stdout, "stdout"], [process.stderr, "stderr"]].map(async ([stream, name]) => {
        await this.checked(["tee", `/tmp/struo-${name}`], { stdin: stream.pipeThrough(bounded(record.maxCaptureBytes, count)) });
      }));
      const exitCode = await process.exitCode;
      record.phase = "completed";
      await storage.put("record", record);
      return json({ exitCode });
    }
    if (record.phase !== "completed") fail(409);
    if (request.method === "GET" && ["/stdout", "/stderr"].includes(path)) {
      return this.file(`/tmp/struo-${path.slice(1)}`, record.maxCaptureBytes);
    }
    if (path === "/outputs" && request.method === "POST") {
      const { paths } = await metadata(request);
      if (!Array.isArray(paths) || !paths.length || paths.some((p) => typeof p !== "string" || p.startsWith("/") || p.split("/").includes("..") || p.includes("\0") || p.includes("\\"))) fail(400);
      await this.checked(["tar", "-cf", "/tmp/struo-outputs.tar", "-C", "/workspace", "--", ...paths.map((p) => `./${p}`)]);
      record.outputs = true;
      await storage.put("record", record);
      return json({});
    }
    if (path === "/outputs" && request.method === "GET" && record.outputs) {
      return this.file("/tmp/struo-outputs.tar", record.maxArchiveBytes);
    }
    fail(404);
  }

  async checked(argv, options = {}) {
    const { stdin, ...rest } = options;
    const process = await this.ctx.container.exec(argv, { stdout: "ignore", stderr: "ignore", ...rest, ...(stdin ? { stdin: "pipe" } : {}) });
    const [exitCode] = await Promise.all([process.exitCode, stdin ? stdin.pipeTo(process.stdin) : undefined]);
    if (exitCode !== 0) throw new Error("container operation failed");
  }

  async file(path, limit) {
    const process = await this.ctx.container.exec(["cat", path], { stderr: "ignore" });
    const { readable, writable } = new TransformStream();
    const writer = writable.getWriter();
    void (async () => {
      try {
        await process.stdout.pipeThrough(bounded(limit)).pipeTo(new WritableStream({ write: (chunk) => writer.write(chunk) }));
        if (await process.exitCode !== 0) throw new Error("container read failed");
        await writer.close();
      } catch (error) { await writer.abort(error); }
    })();
    return new Response(readable, { headers: { "Content-Type": "application/octet-stream" } });
  }
}
