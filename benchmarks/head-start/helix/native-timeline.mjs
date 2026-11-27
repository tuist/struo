// Converts Cargo's --timings report into the timeline the replay card draws:
// seconds since the build started at which each graph target began and ended.
// Cargo units are folded into Struo's targets: `<name>-<version>` for external
// crates, `cargo_<name>` for workspace crates, and the binary into the root.
//
//   node native-timeline.mjs <cargo-timing.html> <targets.json> <root target>
import fs from "node:fs";

const [html, targetsFile, root] = process.argv.slice(2);
const report = fs.readFileSync(html, "utf8");
const units = JSON.parse(report.match(/const UNIT_DATA = (\[[\s\S]*?\]);\n/)[1]);
const ids = new Set(JSON.parse(fs.readFileSync(targetsFile, "utf8")).map((t) => t.id));
const targets = {};
for (const unit of units) {
  let id = `${unit.name}-${unit.version}`;
  if (!ids.has(id)) id = /"bin"/.test(unit.target) ? root : `cargo_${unit.name.replace(/-/g, "_")}`;
  if (!ids.has(id)) continue;
  const end = unit.start + unit.duration;
  const entry = (targets[id] ??= { start: unit.start, end, compiled: true });
  entry.start = Math.min(entry.start, unit.start);
  entry.end = Math.max(entry.end, end);
}
const seconds = Number(report.match(/Total time:<\/td><td>([\d.]+)s/)[1]);
console.log(JSON.stringify({ seconds, targets }));
