import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import fs from "node:fs/promises";
import http from "node:http";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";

test("new connections pay a handshake RTT; reused connections only pay the request RTT", async () => {
  const directory = await fs.mkdtemp(path.join(os.tmpdir(), "head-start-proxy-"));
  const log = path.join(directory, "requests.jsonl");
  const upstream = http.createServer((req, res) => {
    req.resume();
    res.writeHead(200, { "content-length": "2" });
    res.end("ok");
  });
  upstream.listen(0, "127.0.0.1");
  await once(upstream, "listening");
  const reservation = net.createServer();
  reservation.listen(0, "127.0.0.1");
  await once(reservation, "listening");
  const port = reservation.address().port;
  await new Promise((resolve) => reservation.close(resolve));
  const rtt = 300;
  const proxy = spawn(process.execPath, [
    fileURLToPath(new URL("./proxy.mjs", import.meta.url)),
    "--listen", String(port),
    "--upstream", String(upstream.address().port),
    "--log", log,
    "--rtt-ms", String(rtt),
  ]);
  const agent = new http.Agent({ keepAlive: true, maxSockets: 1 });
  try {
    for (let attempt = 0; ; attempt++) {
      try {
        const socket = net.connect(port, "127.0.0.1");
        await once(socket, "connect");
        socket.destroy();
        break;
      } catch (error) {
        if (attempt === 100 || proxy.exitCode !== null) throw error;
        await delay(20);
      }
    }
    const request = async (connection) => {
      const start = performance.now();
      const response = await new Promise((resolve, reject) => {
        http.get({ host: "127.0.0.1", port, agent, headers: { connection }, signal: AbortSignal.timeout(5000) }, resolve)
          .on("error", reject);
      });
      const chunks = [];
      for await (const chunk of response) chunks.push(chunk);
      assert.equal(response.statusCode, 200);
      assert.equal(Buffer.concat(chunks).toString(), "ok");
      return performance.now() - start;
    };
    const fresh = await request("keep-alive");
    const reused = await request("close");
    const reconnected = await request("close");
    assert.ok(fresh >= 1.8 * rtt, `fresh: ${fresh} ms`);
    assert.ok(reconnected >= 1.8 * rtt, `reconnected: ${reconnected} ms`);
    assert.ok(reused >= 0.8 * rtt && reused < 1.8 * rtt, `reused: ${reused} ms`);
    console.log(JSON.stringify({ rtt, fresh, reused, reconnected }));
    proxy.kill("SIGTERM");
    await once(proxy, "exit");
    const rows = (await fs.readFile(log, "utf8")).trim().split("\n").map(JSON.parse);
    assert.equal(rows.length, 3);
    assert.equal(rows[0].conn, rows[1].conn);
    assert.notEqual(rows[1].conn, rows[2].conn);
    assert.ok(rows[0].ms >= 1.8 * rtt, "logged latency includes the handshake wait");
  } finally {
    agent.destroy();
    if (proxy.exitCode === null) {
      proxy.kill("SIGTERM");
      await once(proxy, "exit");
    }
    upstream.closeAllConnections();
    await new Promise((resolve) => upstream.close(resolve));
    await fs.rm(directory, { recursive: true, force: true });
  }
});
