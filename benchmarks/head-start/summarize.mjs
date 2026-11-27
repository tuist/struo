// Summarizes a Head Start run. Reads <state>/runs/<scenario>/<tool>[-<profile>]-<run>.{ms,jsonl,log}
// and writes, next to meta.json and history.json in <results>:
//
//   summary.json   per scenario and tool: median and per-run seconds, and for
//                  Struo the cache traffic (requests, connections, p50/p95/p99
//                  request latency, MiB each way)
//   timeline.json  the root target's graph and when each target finished in
//                  the native build and in Struo's median run, for the replay card
//   RESULTS.md     the same numbers as tables
//
//   node summarize.mjs <state dir> <results dir>
import fs from "node:fs";
import path from "node:path";

const [state, results] = process.argv.slice(2);
const meta = JSON.parse(fs.readFileSync(path.join(results, "meta.json"), "utf8"));
const history = JSON.parse(fs.readFileSync(path.join(results, "history.json"), "utf8"));
const read = (file) => fs.readFileSync(file, "utf8");
const lines = (file) => read(file).split("\n").filter(Boolean).map((l) => JSON.parse(l));
const percentile = (values, p) => {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted.length ? sorted[Math.min(sorted.length - 1, Math.max(0, Math.ceil((p / 100) * sorted.length) - 1))] : null;
};
const median = (values) => percentile(values, 50);
const round = (n, digits = 1) => (n == null ? null : Math.round(n * 10 ** digits) / 10 ** digits);
const mib = (bytes) => round(bytes / 2 ** 20);

const targets = new Map(JSON.parse(read(path.join(state, "targets.json"))).map((t) => [t.id, t]));
const root = meta.root_target;
const nodes = new Map();
const visit = (id) => {
  if (nodes.has(id) || !targets.has(id)) return;
  nodes.set(id, targets.get(id).deps.filter((d) => targets.has(d)));
  nodes.get(id).forEach(visit);
};
visit(root);

// Struo: a target finishes with its last action; it was compiled if any of its
// actions missed the cache. Seconds from session start, graph loading included.
const struoTimeline = (logFile) => {
  const records = lines(logFile);
  const start = Date.parse(records[0].timestamp);
  const byTarget = {};
  for (const r of records) {
    if (r.fields?.message !== "completed cacheable declared graph action" || !nodes.has(r.fields.target)) continue;
    const t = (Date.parse(r.timestamp) - start) / 1000;
    const e = (byTarget[r.fields.target] ??= { start: t, end: t, compiled: false });
    e.start = round(Math.min(e.start, t), 2);
    e.end = round(Math.max(e.end, t), 2);
    e.compiled ||= r.fields.cache !== "hit";
  }
  return { seconds: round((Date.parse(records.at(-1).timestamp) - start) / 1000, 2), targets: byTarget };
};

const profiles = meta.profiles.split(/\s+/).map((p) => {
  const [name, rtt, mbit] = p.split(":");
  return { name, rtt_ms: Number(rtt), mbit: Number(mbit) };
});
const scenarioOrder = ["fresh", ...meta.edits.split(/\s+/).map((f) => `edit-${f.split("/")[0]}`), "cold", ...meta.edits.split(/\s+/).map((f) => `incremental-edit-${f.split("/")[0]}`)];
const runsDir = path.join(state, "runs");
const summary = {};
const timeline = { root, nodes: Object.fromEntries(nodes), native: JSON.parse(read(path.join(state, "native-timeline.json"))), struo: {} };

for (const scenario of scenarioOrder.filter((s) => fs.existsSync(path.join(runsDir, s)))) {
  const dir = path.join(runsDir, scenario);
  const files = fs.readdirSync(dir).filter((f) => f.endsWith(".ms"));
  const tools = [...new Set(files.map((f) => f.replace(/-\d+\.ms$/, "")))];
  summary[scenario] = {};
  for (const tool of tools) {
    const runs = files.filter((f) => f.replace(/-\d+\.ms$/, "") === tool).map((f) => f.replace(/\.ms$/, "")).sort();
    const seconds = runs.map((r) => Number(read(path.join(dir, `${r}.ms`))) / 1000);
    const entry = { median_seconds: round(median(seconds), 2), seconds: seconds.map((s) => round(s, 2)) };
    if (tool.startsWith("struo")) {
      const requests = runs.map((r) => lines(path.join(dir, `${r}.jsonl`)));
      const ms = requests.flat().map((r) => r.ms);
      const timelines = runs.map((r) => struoTimeline(path.join(dir, `${r}.log`)));
      Object.assign(entry, {
        cache_requests: median(requests.map((r) => r.length)),
        connections: median(requests.map((r) => new Set(r.map((x) => x.conn)).size)),
        failed_requests: requests.flat().filter((r) => r.status >= 500).length,
        request_ms: { p50: round(percentile(ms, 50), 2), p95: round(percentile(ms, 95), 2), p99: round(percentile(ms, 99), 2) },
        downloaded_mib: mib(median(requests.map((r) => r.reduce((s, x) => s + x.down, 0)))),
        uploaded_mib: mib(median(requests.map((r) => r.reduce((s, x) => s + x.up, 0)))),
        compiled_targets: median(timelines.map((t) => Object.values(t.targets).filter((e) => e.compiled).length)),
      });
      // The card replays the run with the median duration.
      const order = seconds.map((s, i) => [s, i]).sort((a, b) => a[0] - b[0]);
      (timeline.struo[scenario] ??= {})[tool.replace(/^struo-/, "")] = timelines[order[Math.floor(order.length / 2)][1]];
    }
    summary[scenario][tool] = entry;
  }
}

fs.writeFileSync(path.join(results, "summary.json"), JSON.stringify({ profiles, scenarios: summary }, null, 2) + "\n");
fs.writeFileSync(path.join(results, "timeline.json"), JSON.stringify(timeline) + "\n");

const s = (n) => (n == null ? "" : `${n.toFixed(1)} s`);
const titles = {
  fresh: "Fresh checkout",
  cold: "Fresh checkout, no shared cache",
};
const title = (scenario) =>
  titles[scenario] ??
  (scenario.startsWith("incremental-edit-")
    ? `Built checkout, \`${scenario.slice(17)}\` edited`
    : `Fresh checkout, \`${scenario.slice(5)}\` edited`);
const native = meta.native.split(" ")[0];
const md = [
  `# ${meta.project}: Head Start results`,
  "",
  `${meta.struo} against ${meta.native}, measured ${meta.date.slice(0, 10)} on ${meta.machine}.`,
  `${meta.project} at [\`${meta.revision.slice(0, 12)}\`](${meta.repository}/commit/${meta.revision}) (${meta.revision_date.slice(0, 10)}), building \`${meta.root_target}\` with \`${meta.native_build}\`.`,
  `Median of ${meta.runs} runs. ${meta.setup}`,
  "",
  "## Build time",
  "",
  `| Scenario | ${native} | ${profiles.map((p) => `Struo, ${p.name}`).join(" | ")} |`,
  `| --- | --- | ${profiles.map(() => "---").join(" | ")} |`,
  ...["fresh", ...Object.keys(summary).filter((k) => k.startsWith("edit-"))].filter((k) => summary[k]).map(
    (k) => `| ${title(k)} | ${s(summary[k].native?.median_seconds)} | ${profiles.map((p) => s(summary[k][`struo-${p.name}`]?.median_seconds)).join(" | ")} |`,
  ),
  ...["cold", ...Object.keys(summary).filter((k) => k.startsWith("incremental-"))].filter((k) => summary[k]).map(
    (k) => `| ${title(k)} | ${s(summary[k].native?.median_seconds)} | ${s(summary[k]["struo-loopback"]?.median_seconds)} | ${profiles.slice(1).map(() => "").join(" | ")} |`,
  ),
  "",
  "## Network profiles and cache traffic",
  "",
  "Per fresh-checkout Struo build. Request latency runs from the request reaching the proxy to the last byte of its response.",
  "",
  "| Scenario | Profile | Round trip | Bandwidth | Requests | Connections | p50 | p95 | p99 | Down | Up |",
  "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
  ...Object.entries(summary).flatMap(([k, tools]) =>
    profiles.filter((p) => tools[`struo-${p.name}`] && !k.startsWith("incremental") && k !== "cold").map((p) => {
      const e = tools[`struo-${p.name}`];
      return `| ${title(k)} | ${p.name} | ${p.rtt_ms} ms | ${p.mbit ? `${p.mbit} Mbit/s` : "unlimited"} | ${e.cache_requests} | ${e.connections} | ${e.request_ms.p50} ms | ${e.request_ms.p95} ms | ${e.request_ms.p99} ms | ${e.downloaded_mib} MiB | ${e.uploaded_mib} MiB |`;
    }),
  ),
  "",
  "## Commit history",
  "",
  `The last ${history.commits} commits on the first-parent line (${history.from.slice(0, 10)} to ${history.to.slice(0, 10)}), mapped onto the ${history.targets}-target graph of \`${history.root}\`:`,
  "",
  `- ${history.commits_invalidating_nothing_percent}% change no file of any target in the graph, so a shared cache restores the whole build.`,
  `- The median commit invalidates ${history.invalidated_median} targets and the 90th percentile ${history.invalidated_p90}, counting every dependent.`,
  `- ${history.commits_touching_manifests_percent}% touch ${meta.project}'s dependency manifests, which can also change external packages; that is not counted above.`,
  "",
  "| Target | Commits changing it | Commits invalidating it |",
  "| --- | --- | --- |",
  ...Object.entries(history.direct_changes_per_target)
    .sort((a, b) => b[1] - a[1])
    .map(([id, n]) => `| \`${id}\` | ${n} | ${history.invalidations_per_target[id]} |`),
  "",
];
fs.writeFileSync(path.join(results, "RESULTS.md"), md.join("\n"));
