def _rust_coverage_adapter():
    return _llvm_coverage_adapter() + '''import argparse
import sys

parser = argparse.ArgumentParser()
parser.add_argument('--struo-config', required=True)
parser.add_argument('--struo-results', required=True)
parser.add_argument('--struo-native-results', required=True)
parser.add_argument('--struo-test-unit')
parser.add_argument('--struo-inventory', action='store_true')
args = parser.parse_args()
options = json.loads(pathlib.Path(args.struo_config).read_text())
output = pathlib.Path(args.struo_results)
output.parent.mkdir(parents=True, exist_ok=True)
output.unlink(missing_ok=True)
binary = str(pathlib.Path(options['binary']).resolve())
native = str(pathlib.Path(args.struo_native_results).resolve())
cwd = options.get('cwd') or '.'
if args.struo_inventory:
    execution = subprocess.run([binary, '--list', *options['args']], cwd=cwd, text=True, stdout=subprocess.PIPE,
        env={**os.environ, 'LLVM_PROFILE_FILE':str(output.resolve().parent / 'discovery-%p.profraw')})
    pathlib.Path(native).write_text(execution.stdout)
    if execution.returncode:
        raise SystemExit(2)
    names = [line[:-6] for line in execution.stdout.splitlines() if line.endswith(': test')]
    if len(names) != len(set(names)):
        raise SystemExit(2)
    output.write_text(json.dumps({'schema':'struo.test_inventory.v1', 'target':options['target'], 'units':[options['target'] + '::' + name for name in sorted(names)]}))
    raise SystemExit(0)
prefix = options['target'] + '::'
if not args.struo_test_unit or not args.struo_test_unit.startswith(prefix):
    raise SystemExit('coverage requires one exact current unit')
unit = args.struo_test_unit[len(prefix):]
directory = str(output.parent.resolve() / 'coverage')
shutil.rmtree(directory, ignore_errors=True)
pathlib.Path(directory).mkdir()
env = {**os.environ, 'LLVM_PROFILE_FILE': directory + '/%p.profraw', 'STRUO_PROCESS_TRACE': options['trace'], 'STRUO_PROCESS_TRACE_OUTPUT': directory + '/processes'}
log = str(output.parent.resolve() / 'runner.log')
execution = subprocess.run([options['reporter'], binary, str(output.resolve()), log, native, options['target'], cwd, 'no-report-time', *options['args'], unit, '--exact'], env=env)
report = json.loads(output.read_text())
if [case['id'] for case in report.get('cases', [])] != [args.struo_test_unit]:
    report.pop('coverage', None)
    report['coverage_evidence'] = {'status': 'failed', 'paths': [], 'scopes': []}
else:
    report = llvm_observation(options, report, directory)
output.write_text(json.dumps(report))
raise SystemExit(execution.returncode)
'''

def _rust_coverage_tools(ctx, rustc, triple):
    sysroot = host_command([rustc, "--print", "sysroot"]).strip()
    tools = sysroot + "/lib/rustlib/" + triple + "/bin/"
    cov = _resolve_host_executable(_rust_attr(ctx, "llvm_cov", tools + "llvm-cov"))
    profdata = _resolve_host_executable(_rust_attr(ctx, "llvm_profdata", tools + "llvm-profdata"))
    python = _resolve_host_executable("python3")
    trace = _resolve_host_executable("strace") if host_os() == "linux" else ""
    if not cov or not profdata or not python or not trace:
        fail(ctx["label"]["id"] + ": isolated LLVM coverage requires Python 3, Linux strace and matching llvm-cov/llvm-profdata; install the compiler's llvm-tools component or set llvm_cov and llvm_profdata")
    version = host_command([cov, "--version"]).strip() + " " + host_command([rustc, "--version"]).strip()
    return {"cov": cov, "profdata": profdata, "python": python, "trace": trace, "version": version}

def _rust_coverage_configuration(ctx, provider, rustc, triple):
    tools = _rust_coverage_tools(ctx, rustc, triple)
    thread_count = False
    for argument in _rust_attr(ctx, "args", []):
        if thread_count:
            if not argument.isdigit() or int(argument) < 1:
                fail("coverage_per_test requires a positive --test-threads value")
            thread_count = False
        elif argument == "--test-threads":
            thread_count = True
        elif argument.startswith("--test-threads="):
            value = argument[len("--test-threads="):]
            if not value.isdigit() or int(value) < 1:
                fail("coverage_per_test requires a positive --test-threads value")
        elif argument not in ["--nocapture", "--show-output", "--include-ignored", "--ignored"]:
            fail("coverage_per_test cannot safely attribute libtest argument `" + argument + "`; remove it or disable isolated coverage")
    if thread_count:
        fail("coverage_per_test requires a positive --test-threads value")
    sources = _file_globs(_rust_attr(ctx, "coverage_sources", []))
    if not sources:
        fail(ctx["label"]["id"] + ": coverage_per_test requires coverage_sources")
    inputs = _unique(_rust_source_inputs(ctx) + [_rust_crate_root(ctx, "src/lib.rs")] + _rust_runtime_data(ctx) + _rust_extra_inputs(ctx) + _rust_build_script_inputs(ctx))
    provider["affected_inputs"] = _unique((provider.get("affected_inputs") or []) + sources)
    provider["test_discovery_inputs"] = inputs
    provider["coverage_selection"] = {
        "inputs": inputs, "tool": "llvm", "tool_version": tools["version"],
        "platform": "linux/" + host_command([tools["python"], "-c", "import platform;print(platform.machine())"]).strip(),
        "configuration": {"instrumentation": sources, "target": triple, "runtime": _portable_test_configuration(ctx, [], [tools["version"]], _rust_test_env(ctx, _test_output_dir(ctx)))},
    }
    return tools, sources

def _rust_coverage_actions(ctx, provider, tools, sources, binary, reporter, inputs):
    directory = _test_output_dir(ctx)
    adapter = directory + "/coverage_adapter.py"
    config = directory + "/coverage_config.json"
    inventory = directory + "/inventory.json"
    write_path(adapter, _rust_coverage_adapter())
    write_path(config, _json_encode({"binary": binary, "reporter": reporter, "target": ctx["label"]["id"], "args": _rust_attr(ctx, "args", []), "cwd": _rust_test_cwd(ctx), "sources": sources, "cov": tools["cov"], "profdata": tools["profdata"], "trace": tools["trace"], "version": tools["version"]}))
    base = [tools["python"], adapter, "--struo-config", config]
    inputs = _unique(inputs + sources + [adapter, config])
    run_action(
        argv = base + ["--struo-results", inventory, "--struo-native-results", directory + "/listing.txt", "--struo-inventory"],
        inputs = inputs, outputs = [inventory, directory + "/listing.txt"],
        clean_paths = [inventory], create_dirs = [directory, _rust_scratch_dir(ctx) + "/test-home"],
        env = _rust_test_env(ctx, directory), cacheable = False,
        toolchain_identity = "struo.rust.coverage.discovery.v1\x00" + tools["version"], identifier = ctx["label"]["id"] + ":coverage-discovery",
    )
    if ctx["capability"] == "test-discovery":
        return
    expand_actions(
        implementation = "isolated_test_coverage_plan", inputs = [inventory], outputs = [directory + "/test_results.json"],
        args = {"target": ctx["label"]["id"], "inventory": inventory, "directory": directory, "argv": base, "inputs": inputs,
                "env": _rust_test_env(ctx, directory), "runner_args": [], "filters": (ctx.get("test") or {}).get("filters") or [],
                "output": directory + "/test_results.json", "success_exit_codes": [0, 1, 2, 101], "identity": "struo.rust.coverage.v1\x00" + tools["version"]},
    )
