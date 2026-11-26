#!/usr/bin/env python3
"""Compare release Struo CLIs, Bazel, and Buck2 on full Linux workspace builds."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import statistics
import subprocess
import time


def generate(root, args, engine):
    root.mkdir(parents=True)
    here = Path(__file__).resolve().parent
    for name in ("module.star", "action.mjs"):
        shutil.copyfile(here / name, root / name)
    targets = []
    libraries = [f"lib{i:03}" for i in range(args.libraries)]
    consumers = [f"app{i:04}" for i in range(args.consumers)]
    for label in libraries + consumers:
        paths = []
        count = args.sources if label in libraries else 1
        for n in range(count):
            path = f"sources/{label}/{n:03}.txt"
            file = root / path
            file.parent.mkdir(parents=True, exist_ok=True)
            file.write_text(f"{label}: source {n}\n" + "".join(
                f"exported_symbol_{n * args.symbols + k}=initial\n"
                for k in range(args.symbols)
            ))
            paths.append(path)
        deps = [] if label in libraries else libraries
        symbols = args.sources * args.symbols if label in libraries else 0
        targets.append((label, paths, deps, symbols))
    targets.append(("root", [], consumers, 0))
    manifest = ['[workspace]\ninclude = ["struo.toml"]\n[modules]\npaths = ["module.star"]\n']
    for label, paths, deps, symbols in targets:
        manifest.append(f'[[target]]\nname = "{label}"\nkind = "fixture"\nsrcs = {json.dumps(paths)}\ndeps = {json.dumps(deps)}\n[target.attrs]\nsymbols = {symbols}\n')
    (root / "struo.toml").write_text("\n".join(manifest))
    if engine in ("bazel", "buck2"):
        rule = "bazel.bzl" if engine == "bazel" else "buck.bzl"
        shutil.copyfile(here / rule, root / rule)
        lines = [f'load(":{rule}", "fixture"' + (', "platforms")\nplatforms(name="platforms")' if engine == "buck2" else ')')]
        for label, paths, deps, symbols in targets:
            lines.append(f'fixture(name={json.dumps(label)}, srcs={json.dumps(paths)}, deps={json.dumps([":"+d for d in deps])}, symbols={symbols}, node={json.dumps(shutil.which("node"))}, runner="action.mjs")')
        (root / ("BUILD.bazel" if engine == "bazel" else "BUCK")).write_text("\n".join(lines))
        if engine == "bazel":
            (root / "MODULE.bazel").write_text('module(name="graph_analysis_benchmark")\n')
            (root / ".bazelversion").write_text("9.3.3\n")
        else:
            (root / ".buckconfig").write_text("[cells]\n root = .\n[build]\n execution_platforms = root//:platforms\n")
    return targets


def outputs(root, targets, engine):
    if engine == "buck2":
        return {path.parent.name.removeprefix("__").removesuffix("__"): path
                for path in (root / "buck-out").rglob("artifact.txt") if path.parent.name.startswith("__")}
    base = root / ("bazel-bin" if engine == "bazel" else ".struo/out")
    return {label: base / label / "artifact.txt" for label, *_ in targets}


def verify(root, targets, engine):
    expected = {}
    actual = outputs(root, targets, engine)
    exports = {label: symbols for label, _, _, symbols in targets}
    for label, paths, deps, _ in targets:
        count = sum(exports[dep] for dep in deps)
        digest = hashlib.sha256(f"{label}\n{count}\n".encode())
        inputs = ["action.mjs"] + sorted(paths) + [f".struo/out/{dep}/artifact.txt" for dep in deps]
        for path in inputs:
            digest.update(expected[path] if path in expected else (root / path).read_bytes())
        output = f".struo/out/{label}/artifact.txt"
        expected[output] = f"{label}:{digest.hexdigest()}\n".encode()
        assert actual[label].read_bytes() == expected[output], actual[label]
    return hashlib.sha256(b"".join(expected.values())).hexdigest()


def build(binary, root, phase, engine):
    state = root.parent / (root.name + "-state")
    state.mkdir(exist_ok=True)
    env = os.environ.copy()
    env.update(STRUO_CACHE_PROVIDER="local", STRUO_ANALYSIS_MEMO="1", STRUO_TARGET_OUTCOMES="1",
               XDG_CACHE_HOME=str(state / "cache"), XDG_CONFIG_HOME=str(state / "config"),
               XDG_STATE_HOME=str(state / "logs"))
    command = [str(binary), "build", "root", "--format", "json", "--quiet",
               "--memory-limit", "2GiB", "--cpu-limit", "4"]
    if engine == "bazel":
        command = [str(binary), f"--install_base={root.parent / 'bazel-install'}", f"--output_user_root={state / 'bazel'}", "--host_jvm_args=-Xmx2g", "--ignore_all_rc_files",
                   "build", "//:root", "--jobs=4", "--local_resources=memory=2048", "--spawn_strategy=local",
                   "--noshow_progress", "--color=no", "--noshow_loading_progress"]
    elif engine == "buck2":
        command = [str(binary), "build", "//:root", "--num-threads", "4", "--local-only", "--console", "simple", "--show-full-json-output"]
    start = time.monotonic()
    with (state / f"{phase}.stdout").open("wb") as out, (state / f"{phase}.stderr").open("wb") as err:
        process = subprocess.Popen(command, cwd=root, env=env, stdout=out, stderr=err)
        _, status, usage = os.wait4(process.pid, 0)
        process.returncode = os.waitstatus_to_exitcode(status)
    elapsed = time.monotonic() - start
    if process.returncode:
        raise RuntimeError(f"{phase}: exit {process.returncode}\n" + (state / f"{phase}.stderr").read_text())
    return dict(phase=phase, seconds=elapsed, cpu_seconds=usage.ru_utime + usage.ru_stime,
                maxrss_kib=usage.ru_maxrss, result=(state / f"{phase}.stdout").read_text())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("before", type=Path)
    parser.add_argument("after", type=Path)
    parser.add_argument("directory", type=Path, help="New disposable directory for fixtures and results")
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--libraries", type=int, default=32)
    parser.add_argument("--consumers", type=int, default=480)
    parser.add_argument("--sources", type=int, default=64)
    parser.add_argument("--symbols", type=int, default=64)
    parser.add_argument("--bazel", type=Path)
    parser.add_argument("--buck2", type=Path)
    args = parser.parse_args()
    node = Path(subprocess.check_output(["node", "-p", "process.execPath"], text=True).strip())
    os.environ["PATH"] = str(node.parent) + os.pathsep + os.environ["PATH"]
    for field in ("runs", "libraries", "consumers", "sources"):
        if getattr(args, field) < 1:
            parser.error(f"--{field} must be positive")
    if args.symbols < 0:
        parser.error("--symbols must be nonnegative")
    args.directory = args.directory.resolve()
    args.directory.mkdir(parents=True, exist_ok=False)
    binaries = {"before": args.before.resolve(), "after": args.after.resolve()}
    for name in ("bazel", "buck2"):
        if getattr(args, name):
            binaries[name] = getattr(args, name).resolve()
    rows = []
    identities = {}
    for name, path in binaries.items():
        with path.open("rb") as binary:
            identities[name] = hashlib.file_digest(binary, "sha256").hexdigest()
    if "bazel" in binaries:
        with (args.directory / "bazel-version.txt").open("w") as log:
            subprocess.run([str(binaries["bazel"]), f"--install_base={args.directory / 'bazel-install'}", "--batch", "version"], cwd=args.directory, stdout=log, stderr=log, check=True)
    for iteration in range(args.runs):
        checksums = {}
        order = list(binaries)
        order = order[iteration % len(order):] + order[:iteration % len(order)]
        for name in order:
            root = args.directory / f"{iteration}-{name}"
            targets = generate(root, args, name)
            for phase in ("cold", "warm", "changed", "warm_changed", "recovery"):
                if phase == "changed":
                    (root / "sources/lib000/000.txt").write_text("changed shared library source\n")
                if phase == "recovery":
                    outputs(root, targets, name)["root"].unlink()
                if phase in ("warm", "warm_changed"):
                    samples = [build(binaries[name], root, f"{phase}-{sample}", name) for sample in range(5)]
                    row = dict(samples[-1], phase=phase, samples=samples)
                    for field in ("seconds", "cpu_seconds", "maxrss_kib"):
                        row[field] = statistics.median(sample[field] for sample in samples)
                else:
                    row = build(binaries[name], root, phase, name)
                row.update(variant=name, iteration=iteration, output_hash=None, verified=False)
                try:
                    row["output_hash"] = verify(root, targets, name)
                    row["verified"] = True
                except (AssertionError, KeyError, FileNotFoundError) as error:
                    if phase != "recovery":
                        raise
                    row["verification_error"] = repr(error)
                if row["verified"]:
                    previous = checksums.setdefault(phase, row["output_hash"])
                    assert previous == row["output_hash"], (phase, "before/after output mismatch")
                rows.append(row)
                (args.directory / "results.json").write_text(json.dumps(dict(
                    binaries=identities, targets=len(targets), resource_scope="client and waited children only; persistent daemons excluded", settings={k: getattr(args, k) for k in
                    ("runs", "libraries", "consumers", "sources", "symbols")}, rows=rows), indent=2))
                print(json.dumps({k: row[k] for k in ("variant", "iteration", "phase", "seconds", "cpu_seconds", "maxrss_kib", "output_hash", "verified")}), flush=True)
            assert checksums["cold"] == checksums["warm"]
            assert checksums["changed"] != checksums["cold"]
            assert checksums["changed"] == checksums["warm_changed"]
            if row["verified"]:
                assert checksums["changed"] == row["output_hash"]
            if name == "bazel":
                subprocess.run([str(binaries[name]), f"--install_base={root.parent / 'bazel-install'}", f"--output_user_root={root.parent / (root.name + '-state') / 'bazel'}", "--host_jvm_args=-Xmx2g", "--ignore_all_rc_files", "shutdown"], cwd=root, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            elif name == "buck2":
                subprocess.run([str(binaries[name]), "kill"], cwd=root, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            else:
                for process in Path("/proc").glob("[0-9]*/cmdline"):
                    try:
                        command = process.read_bytes().split(b"\0")
                        if command[1:4] == [b"-C", os.fsencode(root), b"__change-tracker"]:
                            pid = int(process.parent.name)
                            subprocess.run(["ps", "-o", "pid,args", "-p", str(pid)], check=True)
                            os.kill(pid, signal.SIGTERM)
                    except (FileNotFoundError, ProcessLookupError):
                        pass
    for phase in ("cold", "warm", "changed", "recovery", "warm_changed"):
        for name in binaries:
            group = [r for r in rows if r["phase"] == phase and r["variant"] == name]
            print(name, phase, {field: statistics.median(r[field] for r in group)
                                for field in ("seconds", "cpu_seconds", "maxrss_kib")},
                  "verified", sum(r["verified"] for r in group), "/", len(group))


if __name__ == "__main__":
    main()
