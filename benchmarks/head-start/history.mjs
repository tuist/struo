// Replays a project's recent commits against its Struo graph: which targets
// each commit's changed files belong to, and how many targets that invalidates
// once every dependent is included. Uses only the graph Struo reports, so it
// works for any project Struo can load.
//
//   node history.mjs <checkout> <struo binary> <root target> <commits> <manifest globs> > history.json
//
// Targets own the files their `srcs` globs match, an upper bound: Struo
// rebuilds only when a file the tool actually read changes. Commits that touch
// a file matching <manifest globs> (space-separated, such as lockfiles) can
// also change external dependencies, which this does not model; they are
// counted separately.
import { execFileSync, execFile } from "node:child_process";
import { promisify } from "node:util";

const [checkout, struo, root, limit, manifestGlobs = ""] = process.argv.slice(2);
const run = promisify(execFile);
const git = (...args) => execFileSync("git", ["-C", checkout, ...args], { encoding: "utf8", maxBuffer: 1 << 28 });

const glob = (pattern) =>
  new RegExp(
    "^" +
      pattern
        .split(/(\*\*\/|\*\*|\*|\?)/)
        .map((part) => ({ "**/": "(?:.*/)?", "**": ".*", "*": "[^/]*", "?": "[^/]" })[part] ?? part.replace(/[.+^${}()|[\]\\]/g, "\\$&"))
        .join("") +
      "$",
  );

const all = new Map(JSON.parse(execFileSync(struo, ["-C", checkout, "query", "targets", "--format", "json"], { encoding: "utf8", maxBuffer: 1 << 28 })).map((t) => [t.id, t]));
const deps = new Map();
const visit = (id) => {
  if (deps.has(id) || !all.has(id)) return;
  deps.set(id, all.get(id).deps.filter((d) => all.has(d)));
  deps.get(id).forEach(visit);
};
visit(root);

// Sources come from `struo query target`, eight at a time.
const ids = [...deps.keys()];
const sources = new Map();
for (let i = 0; i < ids.length; i += 8) {
  await Promise.all(
    ids.slice(i, i + 8).map(async (id) => {
      const { stdout } = await run(struo, ["-C", checkout, "query", "target", id, "--format", "json"], { maxBuffer: 1 << 26 });
      const srcs = (JSON.parse(stdout).srcs ?? []).filter((s) => !s.startsWith("/"));
      if (srcs.length) sources.set(id, srcs.map(glob));
    }),
  );
}
const manifests = manifestGlobs.split(/\s+/).filter(Boolean).map(glob);

const dependents = new Map(ids.map((id) => [id, []]));
for (const [id, list] of deps) for (const dep of list) dependents.get(dep).push(id);
const closure = (changed) => {
  const seen = new Set(changed);
  const queue = [...changed];
  while (queue.length) for (const next of dependents.get(queue.pop())) if (!seen.has(next)) seen.add(next), queue.push(next);
  return seen;
};

const log = git("log", "--first-parent", "-n", limit, "--format=%H%x09%cI", "HEAD").trim().split("\n").map((l) => l.split("\t"));
const commits = log.map(([sha, date]) => {
  const files = git("diff-tree", "--no-commit-id", "--name-only", "-r", "-m", "--first-parent", sha).split("\n").filter(Boolean);
  const changed = [...sources].filter(([, patterns]) => files.some((f) => patterns.some((p) => p.test(f)))).map(([id]) => id);
  return { sha: sha.slice(0, 12), date, files: files.length, changed, invalidated: closure(changed).size, manifests: files.some((f) => manifests.some((p) => p.test(f))) };
});

const counts = Object.fromEntries(ids.map((id) => [id, 0]));
const direct = Object.fromEntries([...sources.keys()].map((id) => [id, 0]));
for (const c of commits) {
  for (const id of closure(c.changed)) counts[id]++;
  for (const id of c.changed) direct[id]++;
}
const sorted = commits.map((c) => c.invalidated).sort((a, b) => a - b);
const at = (p) => sorted[Math.min(sorted.length - 1, Math.ceil(p * sorted.length) - 1)];
const share = (n) => Math.round((n / commits.length) * 1000) / 10;

console.log(JSON.stringify({
  root,
  targets: ids.length,
  targets_with_workspace_sources: sources.size,
  commits: commits.length,
  from: commits.at(-1).date,
  to: commits[0].date,
  invalidated_median: at(0.5),
  invalidated_p90: at(0.9),
  commits_invalidating_nothing_percent: share(commits.filter((c) => !c.invalidated).length),
  commits_touching_manifests_percent: share(commits.filter((c) => c.manifests).length),
  invalidations_per_target: counts,
  direct_changes_per_target: direct,
  per_commit: commits,
}, null, 1));
