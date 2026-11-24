def _jvm_coverage_adapter():
    return _native_coverage_process_support() + '''import argparse
import xml.etree.ElementTree as ET

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
log = str(output.parent / 'runner.log')
native = args.struo_native_results
argv = list(options['argv'])
env = dict(os.environ)
if args.struo_inventory:
    env['STRUO_TEST_INVENTORY'] = str(output.resolve())
    command = argv + [str(output), log, native, options['target'], options['runner'], *options['filters']]
    raise SystemExit(subprocess.run(command, env=env).returncode)
prefix = options['target'] + '::'
if not args.struo_test_unit or not args.struo_test_unit.startswith(prefix):
    raise SystemExit('coverage requires one exact current JVM unit')
env['STRUO_TEST_UNIT'] = args.struo_test_unit
directory = str(output.parent.resolve() / 'coverage')
shutil.rmtree(directory, ignore_errors=True)
pathlib.Path(directory).mkdir()
profile = directory + '/jacoco.exec'
argv.insert(1, '-javaagent:' + str(pathlib.Path(options['agent']).resolve()) + '=destfile=' + profile + ',append=false')
argv += [str(output), log, native, options['target'], options['runner'], *options['filters']]
traced = [options['trace'], '-f', '-e', 'trace=process', '-e', 'raw=execve,execveat', '-o', directory + '/processes', *argv]
execution = subprocess.run(traced, env=env)
report = json.loads(output.read_text())
files = []
complete = report.get('status') == 'passed' and [case['id'] for case in report.get('cases', [])] == [args.struo_test_unit]
try:
    complete = complete and single_native_process(directory)
    xml = directory + '/jacoco.xml'
    command = [options['argv'][0], '-jar', options['cli'], 'report', profile, '--xml', xml]
    for classes in options['classfiles']:
        command += ['--classfiles', classes]
    subprocess.run(command, check=True)
    if pathlib.Path(xml).stat().st_size > 16 * 1024 * 1024:
        raise ValueError('coverage report exceeds observation limit')
    indexed = {}
    for package in ET.parse(xml).getroot().findall('package'):
        for source in package.findall('sourcefile'):
            key = package.get('name') + '/' + source.get('name')
            if key in indexed:
                raise ValueError('ambiguous JVM coverage source')
            indexed[key] = source
    root = pathlib.Path.cwd().resolve()
    for source in options['sources']:
        path = pathlib.Path(source).resolve()
        package = re.search(r'^\\s*package\\s+([A-Za-z_][\\w.]*)', path.read_text(), re.MULTILINE)
        key = (package.group(1).replace('.', '/') if package else '') + '/' + path.name
        measured = indexed[key]
        lines = {}
        for line in measured.findall('line'):
            number, count, missed = int(line.get('nr')), int(line.get('ci')), int(line.get('mi'))
            if number < 1 or min(count, missed) < 0 or number in lines:
                raise ValueError('invalid JVM coverage counters')
            if count + missed:
                lines[number] = 1 if count else 0
        if not lines:
            raise ValueError('empty instrumented JVM source')
        numbers = sorted(lines)
        files.append({'path': path.relative_to(root).as_posix(), 'line_numbers': numbers, 'execution_counts': [lines[line] for line in numbers]})
except (OSError, ValueError, KeyError, TypeError, ET.ParseError, subprocess.SubprocessError) as error:
    print('incomplete JVM coverage: ' + str(error))
    complete = False
report['coverage'] = {'tool':'jacoco', 'tool_version':options['version'], 'platform':platform.system().lower() + '/' + platform.machine(), 'partial':not complete, 'files':files}
output.write_text(json.dumps(report))
raise SystemExit(execution.returncode)
'''

_JVM_COVERAGE_ATTRS = [
    attr("coverage_per_test", "bool", default = "false", docs = "Measure each exact JVM execution unit with JaCoCo and native process-tree validation. Currently requires Linux strace and Python 3.", configurable = False),
    attr("coverage_sources", "list<string>", default = "[]", docs = "Product Java or Kotlin source globs required in the instrumented class universe.", configurable = False),
    attr("jacoco_agent", "string", docs = "Package-relative JaCoCo runtime agent JAR, required for isolated coverage.", configurable = False),
    attr("jacoco_cli", "string", docs = "Package-relative matching JaCoCo command-line JAR, required for isolated coverage.", configurable = False),
]

def _jvm_coverage_configuration(ctx, provider, argv, env, inputs, classfiles, runner):
    attr = ctx["attr"]
    sources = _file_globs(attr.get("coverage_sources") or [])
    agent = _package_relative(ctx, attr.get("jacoco_agent") or "")
    cli = _package_relative(ctx, attr.get("jacoco_cli") or "")
    python = _resolve_host_executable("python3")
    trace = _resolve_host_executable("strace") if host_os() == "linux" else ""
    if not sources or not attr.get("jacoco_agent") or not attr.get("jacoco_cli") or not python or not trace:
        fail(ctx["label"]["id"] + ": coverage_per_test requires coverage_sources, package-relative jacoco_agent/jacoco_cli, Python 3 and Linux strace")
    version = host_command([argv[0], "-jar", cli, "version"], cwd = workspace_root()).strip()
    identity = {"collector": version, "agent": host_file_sha256(workspace_root() + "/" + agent), "cli": host_file_sha256(workspace_root() + "/" + cli), "runtime": host_command([argv[0], "-version"], merge_stderr = True).strip()}
    platform = "linux/" + host_command([python, "-c", "import platform;print(platform.machine())"]).strip()
    provider["coverage_selection"] = {"tool": "jacoco", "tool_version": version, "platform": platform, "inputs": inputs,
                                      "configuration": {"runtime": _portable_test_configuration(ctx, [], [identity], env), "sources": sources}}
    provider["affected_inputs"] = _unique((provider.get("affected_inputs") or []) + sources)
    provider["test_discovery_inputs"] = inputs
    return {"argv": argv, "target": ctx["label"]["id"], "runner": runner, "filters": ([attr["test_class"]] if attr.get("test_class") else []) + (attr.get("args") or []),
            "classfiles": classfiles, "sources": sources, "agent": agent, "cli": cli, "version": version, "trace": trace, "python": python}

def _jvm_coverage_actions(ctx, configuration, inputs, env):
    directory = _test_output_dir(ctx)
    inventory = directory + "/inventory.json"
    adapter = directory + "/coverage_adapter.py"
    config = directory + "/coverage_config.json"
    write_path(adapter, _jvm_coverage_adapter())
    write_path(config, _json_encode(configuration))
    base = [configuration["python"], adapter, "--struo-config", config]
    inputs = _unique(inputs + configuration["sources"] + [adapter, config, configuration["agent"], configuration["cli"]])
    run_action(
        argv = base + ["--struo-results", inventory, "--struo-native-results", directory + "/listing.txt", "--struo-inventory"],
        inputs = inputs, outputs = [inventory], clean_paths = [inventory], create_dirs = [directory, directory + "/home"],
        resources = {"memory_bytes": 1024 * 1024 * 1024},
        env = env, cacheable = False, toolchain_identity = "struo.jvm.coverage.discovery.v1\x00" + configuration["version"], identifier = ctx["label"]["id"] + ":coverage-discovery",
    )
    if ctx["capability"] == "test-discovery":
        return
    expand_actions(
        implementation = "isolated_test_coverage_plan", inputs = [inventory], outputs = [directory + "/test_results.json"],
        args = {"target": ctx["label"]["id"], "inventory": inventory, "directory": directory, "argv": base, "inputs": inputs,
                "env": env, "runner_args": [], "filters": (ctx.get("test") or {}).get("filters") or [], "output": directory + "/test_results.json",
                "resources": {"memory_bytes": 1024 * 1024 * 1024},
                "success_exit_codes": [0, 1, 2], "identity": "struo.jvm.coverage.v1\x00" + configuration["version"]},
    )
