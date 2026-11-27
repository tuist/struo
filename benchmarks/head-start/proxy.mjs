// Sits between Struo and the cache server. It appends one JSON line per
// request (method, status, bytes each way, milliseconds from arrival to the end
// of the response) and can emulate a network path to the server:
//
//   --rtt-ms     round-trip time. Each request waits one RTT before it is
//                forwarded, and each new connection one more, for the TCP
//                handshake, before its first request is forwarded.
//   --mbit       link bandwidth in megabits per second, shared by all
//                connections, applied to uploads and downloads separately.
//
//   node proxy.mjs --listen 18094 --upstream 18093 --log requests.jsonl [--rtt-ms 10 --mbit 1000]
import http from "node:http";
import fs from "node:fs";
import { parseArgs } from "node:util";

const { values: options } = parseArgs({
  options: {
    listen: { type: "string" },
    upstream: { type: "string" },
    log: { type: "string" },
    "rtt-ms": { type: "string", default: "0" },
    mbit: { type: "string", default: "0" },
  },
});
const rtt = Number(options["rtt-ms"]);
const bytesPerMs = (Number(options.mbit) * 1e6) / 8 / 1000;
const log = fs.createWriteStream(options.log, { flags: "a" });
// Queueing for an upstream connection counts as latency.
const agent = new http.Agent({ keepAlive: true, maxSockets: 8 });
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

// One link per direction: a chunk leaves once every earlier chunk has, at the
// link's rate. Scheduling against the link's own clock keeps the average rate
// exact even when timers fire late.
function link() {
  let free = 0;
  return async (bytes) => {
    if (!bytesPerMs) return;
    const now = performance.now();
    free = Math.max(free, now) + bytes / bytesPerMs;
    if (free - now >= 1) await sleep(free - now);
  };
}
const uplink = link();
const downlink = link();

async function pump(from, to, send, count) {
  for await (const chunk of from) {
    await send(chunk.length);
    count(chunk.length);
    if (!to.write(chunk)) await new Promise((resolve) => to.once("drain", resolve));
  }
  to.end();
}

let connections = 0;
const server = http.createServer(async (req, res) => {
  const start = performance.now();
  let up = 0;
  let down = 0;
  const conn = req.socket.id;
  const done = (status) =>
    log.write(JSON.stringify({ t: Date.now(), conn, method: req.method, status, up, down, ms: performance.now() - start }) + "\n");
  await req.socket.handshake;
  if (rtt) await sleep(rtt);
  const out = http.request(
    { host: "127.0.0.1", port: options.upstream, method: req.method, path: req.url, headers: req.headers, agent },
    (response) => {
      res.writeHead(response.statusCode, response.headers);
      pump(response, res, downlink, (n) => (down += n)).then(() => done(response.statusCode), () => done(599));
    },
  );
  out.on("error", () => {
    if (!res.headersSent) res.writeHead(502).end();
    done(502);
  });
  pump(req, out, uplink, (n) => (up += n)).catch(() => out.destroy());
});
server.on("connection", (socket) => {
  socket.id = ++connections;
  socket.handshake = rtt ? sleep(rtt) : Promise.resolve();
});
server.listen(options.listen, "127.0.0.1");
// The harness stops the proxy with SIGTERM; flush the log first.
process.on("SIGTERM", () => log.end(() => process.exit(0)));
