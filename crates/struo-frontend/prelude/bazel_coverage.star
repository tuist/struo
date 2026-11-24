def _bazel_coverage_adapter():
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
output = pathlib.Path(args.struo_results).resolve()
output.parent.mkdir(parents=True, exist_ok=True)
output.unlink(missing_ok=True)
native = pathlib.Path(args.struo_native_results).resolve()
unit = options['target'] + '::invocation'
cwd = options['cwd']
if args.struo_inventory:
    with native.open('wb') as stream:
        listing = subprocess.run([options['bazel'], 'query', 'kind(".*_test rule", ' + options['label'] + ')', '--output=label'], cwd=cwd, stdout=stream)
    if listing.returncode or native.read_text().strip() != options['label']:
        raise SystemExit('Bazel discovery did not resolve exactly the current native test rule')
    output.write_text(json.dumps({'schema':'struo.test_inventory.v1','target':options['target'],'units':[unit]}))
    raise SystemExit(0)
if args.struo_test_unit != unit:
    raise SystemExit('coverage requires the full Bazel test invocation')
directory = output.parent / 'coverage'
shutil.rmtree(directory, ignore_errors=True)
directory.mkdir()
events = directory / 'events.json'
command = [options['bazel'], 'coverage', options['label'], '--noshow_progress', '--test_output=errors',
    '--nocache_test_results', '--test_sharding_strategy=disabled', '--runs_per_test=1', '--flaky_test_attempts=1',
    '--jobs=1', '--combined_report=lcov', '--build_event_json_file=' + str(events)]
command += ['--test_env=' + key for key in options['test_env']]
with native.open('wb') as stream:
    execution = subprocess.run(command, cwd=cwd, stdout=stream, stderr=stream)
status = 'passed' if execution.returncode == 0 else 'failed'
complete = status == 'passed'
files = []
try:
    if events.stat().st_size > 16 * 1024 * 1024:
        raise ValueError('Bazel event stream exceeds limit')
    records = [json.loads(line) for line in events.read_text().splitlines()]
    results = [record for record in records if 'testResult' in record]
    if len(results) != 1 or results[0]['id']['testResult']['label'] != options['label']:
        raise ValueError('Bazel invocation did not produce exactly one native test result')
    result = results[0]['testResult']
    if result['status'] != 'PASSED' or result.get('cachedLocally') or result.get('executionInfo', {}).get('cachedRemotely'):
        raise ValueError('partial, failed or cached native test result')
    def local_file(record):
        from urllib.parse import urlparse, unquote
        uri = urlparse(record['uri'])
        if uri.scheme != 'file' or uri.netloc:
            raise ValueError('native result is not a local artifact')
        path = pathlib.Path(unquote(uri.path))
        if path.stat().st_size > 16 * 1024 * 1024:
            raise ValueError('native coverage artifact exceeds limit')
        return path
    xml = [entry for entry in result.get('testActionOutput', []) if entry['name'] == 'test.xml']
    if len(xml) != 1:
        raise ValueError('missing native completion summary')
    suites = list(ET.parse(local_file(xml[0])).getroot().iter('testsuite'))
    if any(int(suite.get('tests', '-1')) != len(suite.findall('testcase')) or any(int(suite.get(key, '0')) for key in ['skipped', 'failures', 'errors']) for suite in suites):
        raise ValueError('native case count does not match completion summary')
    cases = [case for suite in suites for case in suite.findall('testcase')]
    if not cases or any(case.find('skipped') is not None or case.find('failure') is not None or case.find('error') is not None for case in cases):
        raise ValueError('incomplete native test cases')
    summaries = [record['testSummary'] for record in records if record.get('id', {}).get('testSummary', {}).get('label') == options['label']]
    if len(summaries) != 1 or summaries[0]['overallStatus'] != 'PASSED' or summaries[0].get('totalRunCount') != 1:
        raise ValueError('missing complete native invocation')
    reports = [entry for entry in result.get('testActionOutput', []) if entry['name'] == 'test.lcov']
    if len(reports) != 1:
        raise ValueError('missing coverage from the current native test result')
    indexed = {}
    current = None
    for line in local_file(reports[0]).read_text().splitlines():
        if line.startswith('SF:'):
            path = pathlib.Path(line[3:])
            path = (pathlib.Path(cwd) / path).resolve()
            if path in indexed:
                raise ValueError('ambiguous native coverage file')
            current = indexed[path] = {}
        elif line.startswith('DA:'):
            number, count, *_ = line[3:].split(',')
            number, count = int(number), int(count)
            if current is None or number < 1 or count < 0 or number in current:
                raise ValueError('invalid native coverage counter')
            current[number] = count
        elif line == 'end_of_record':
            current = None
    root = pathlib.Path(options['root'])
    for source in options['sources']:
        path = pathlib.Path(source)
        counts = indexed[path]
        if not counts:
            raise ValueError('source absent from instrumentation')
        numbers = sorted(counts)
        files.append({'path':path.relative_to(root).as_posix(),'line_numbers':numbers,'execution_counts':[counts[line] for line in numbers]})
except (OSError, ValueError, KeyError, TypeError, ET.ParseError) as error:
    print('incomplete Bazel coverage: ' + str(error))
    complete = False
report = {'schema':'struo.test_results.v1','target':options['target'],'runner':{'type':'bazel','metadata':{'unit':'atomic-invocation'}},'status':status,
    'summary':{'total':1,'passed':int(status == 'passed'),'failed':int(status != 'passed'),'skipped':0,'flaky':0},
    'cases':[{'id':unit,'name':options['label'],'suite':options['target'],'status':status,'attempts':[{'status':status}],'runner_metadata':{'unit':'atomic-invocation'}}],
    'artifacts':{'logs':[str(native)],'native_results':[str(native)]},
    'coverage':{'tool':'bazel-lcov','tool_version':options['version'],'platform':options['platform'],'partial':not complete,'files':files}}
output.write_text(json.dumps(report))
raise SystemExit(0 if execution.returncode == 0 else 1)
'''

def _bazel_coverage_impl(ctx):
    if not ctx["attr"].get("_bazel_resolved"):
        fail(ctx["label"]["id"] + ": bazel_test must be materialized by a bazel_workspace resolver")
    bazel = _bazel_resolve_executable(ctx)
    python = _resolve_host_executable("python3")
    sources = _file_globs(ctx["attr"].get("coverage_sources") or [])
    toolchain = _bazel_toolchain_identity(ctx)
    if not python or not sources or not toolchain:
        fail("Bazel coverage requires Python 3, coverage_sources and complete toolchain_inputs")
    inputs = glob(["**/*"], exclude = [".git/**/*", ".struo/**/*", "bazel-*/**/*"])
    test_inputs = [path for path in inputs if path not in sources]
    env = _bazel_env(ctx)
    test_env = ctx["attr"].get("test_env") or {}
    env.update(test_env)
    version = host_command([bazel, "--version"], cwd = _bazel_workspace_absolute_dir(ctx), env = env).strip()
    platform = host_os() + "/" + host_command([python, "-c", "import platform;print(platform.machine())"]).strip()
    directory = ctx["build_dir"] + "/test"
    inventory = directory + "/inventory.json"
    results = directory + "/test_results.json"
    provider = {"label_id":ctx["label"]["id"],"target_kind":"bazel_test","bazel_test":True,"bazel_target":True,"bazel_label":ctx["attr"]["bazel_label"],
        "affected_inputs":inputs,"test_discovery_inputs":test_inputs,"test_inventory":inventory,
        "coverage_selection":{"tool":"bazel-lcov","tool_version":version,"platform":platform,"inputs":test_inputs,"configuration":{"label":ctx["attr"]["bazel_label"],"sources":sources,"env":ctx["attr"].get("env") or {},"test_env":test_env,"toolchain":toolchain}},
        "test_info":{"schema":"struo.test_info.v1","target":ctx["label"]["id"],"runner":{"type":"bazel","metadata":{"unit":"atomic-invocation"}},
            "command":{"argv":[bazel,"coverage",ctx["attr"]["bazel_label"]],"env":env,"cwd":ctx["label"]["package"] or "."},
            "outputs":{"results":results,"logs":[],"native_results":[],"coverage":[]},"listing":{"supported":True,"strategy":"atomic-native-invocation"},
            "filtering":{"case_filtering":"unsupported"},"sharding":{"supported":False},"retries":{"supported":False,"default_attempts":1},
            "execution":{"cacheable":False,"run_from_workspace_root":True},"labels":[],"metadata":{}}}
    if ctx["capability"] not in ["test", "test-discovery"]:
        return provider
    adapter = directory + "/coverage_adapter.py"
    config = directory + "/coverage_config.json"
    write_path(adapter, _bazel_coverage_adapter())
    write_path(config, _json_encode({"bazel":bazel,"root":workspace_root(),"cwd":_bazel_workspace_absolute_dir(ctx),"target":ctx["label"]["id"],
        "label":ctx["attr"]["bazel_label"],"sources":[workspace_root() + "/" + path for path in sources],"version":version,"platform":platform,"test_env":test_env}))
    base = [python, adapter, "--struo-config", config]
    run_action(argv = base + ["--struo-inventory","--struo-results",inventory,"--struo-native-results",directory + "/listing.log"], inputs = inputs + [adapter, config], outputs = [inventory, directory + "/listing.log"], clean_paths = [inventory], create_dirs = [directory], env = env, cacheable = False, resources = {"memory_bytes":2 * 1024 * 1024 * 1024}, identifier = ctx["label"]["id"] + ":coverage-discovery")
    if ctx["capability"] == "test-discovery":
        return provider
    expand_actions(implementation = "isolated_test_coverage_plan", inputs = [inventory], outputs = [results], args = {
        "target":ctx["label"]["id"],"inventory":inventory,"directory":directory,"argv":base,"inputs":inputs + [adapter,config],"env":env,"runner_args":[],
        "filters":(ctx.get("test") or {}).get("filters") or [],"output":results,"success_exit_codes":[0,1],"identity":"struo.bazel.coverage.v1\x00" + version,"resources":{"memory_bytes":2 * 1024 * 1024 * 1024}})
    return provider
