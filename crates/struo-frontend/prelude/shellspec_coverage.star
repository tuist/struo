def _shellspec_coverage_hook():
    return '''shellspec_coverage_setup() {
  printf '%s\\n' "$BASHPID" >> "$STRUO_COVERAGE_ROOTS"
  shellspec_coverage_start() { :; }
  shellspec_coverage_stop() { :; }
  shellspec_coverage_env() { export BASH_ENV="$STRUO_COVERAGE_ENV"; }
  . "$STRUO_COVERAGE_ENV"
  export BASH_ENV="$STRUO_COVERAGE_ENV"
}
'''

def _shellspec_coverage_adapter():
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
if args.struo_inventory:
    with native.open('wb') as stream:
        listing = subprocess.run([options['shellspec'], '--shell', options['bash'], '--list', 'specfiles'] + options['args'] + options['specs'], stdout=stream, stderr=subprocess.STDOUT)
    if listing.returncode or not native.read_text().strip():
        raise SystemExit('ShellSpec discovery did not produce a complete native inventory')
    output.write_text(json.dumps({'schema':'struo.test_inventory.v1','target':options['target'],'units':[unit]}))
    raise SystemExit(0)
if args.struo_test_unit != unit:
    raise SystemExit('coverage requires the full ShellSpec invocation')
directory = output.parent / 'coverage'
shutil.rmtree(directory, ignore_errors=True)
directory.mkdir()
hook = directory / 'struo_full_coverage.sh'
hook.write_text(options['hook'])
environment = directory / 'environment.sh'
environment.write_text("""set -o functrace
trap 'echo "kcov@${BASH_SOURCE:-}@${LINENO}@" >&$KCOV_BASH_XTRACEFD' DEBUG
""")
roots = directory / 'roots'
env = dict(os.environ, STRUO_COVERAGE_ROOTS=str(roots), STRUO_COVERAGE_ENV=str(environment))
profile = directory / 'kcov'
junit = directory / 'junit'
trace = directory / 'processes'
command = [options['strace'], '-f', '-qq', '-e', 'trace=process', '-o', str(trace), options['shellspec'],
    '--shell', options['bash'], '--kcov', '--kcov-path', options['kcov'], '--covdir', str(profile),
    '--kcov-options', '--clean --cobertura-only --include-path=' + ','.join(options['sources']),
    '--load-path', str(directory), '--require', 'struo_full_coverage', '--output', 'junit', '--reportdir', str(junit)]
command += options['args'] + options['specs']
with native.open('wb') as stream:
    execution = subprocess.run(command, stdout=stream, stderr=subprocess.STDOUT, env=env)
status = 'passed' if execution.returncode == 0 else 'failed'
complete = status == 'passed'
files = []
try:
    for path in [trace, roots, junit / 'results_junit.xml', profile / 'cov.xml']:
        if path.stat().st_size > 16 * 1024 * 1024:
            raise ValueError('native artifact exceeds limit')
    processes = trace.read_text().splitlines()
    invocations = set(roots.read_text().splitlines())
    descendants = set(invocations)
    if not invocations:
        raise ValueError('full lifecycle hook did not run')
    for _ in range(len(processes)):
        before = len(descendants)
        for line in processes:
            pid = line.split()[0]
            child = re.search(r'(?:clone[3]?|fork|vfork)(?:\\(| resumed>).* = ([0-9]+)$', line)
            if pid in descendants and child:
                descendants.add(child[1])
        if len(descendants) == before:
            break
    starts = {pid: 0 for pid in invocations}
    for line in processes:
        pid = line.split()[0]
        if pid in descendants and re.search(r'\\bexecve(?:at)?\\(', line):
            if pid not in starts or starts[pid]:
                raise ValueError('test invocation executed an external child process')
            starts[pid] += 1
    if any(count != 1 for count in starts.values()):
        raise ValueError('incomplete native process trace')
    summary = ET.parse(junit / 'results_junit.xml').getroot()
    suites = list(summary.iter('testsuite'))
    cases = [case for suite in suites for case in suite.findall('testcase')]
    if not cases or any(int(suite.get('tests', '-1')) != len(suite.findall('testcase')) or any(int(suite.get(key, '0')) for key in ['skipped','failures','errors']) for suite in suites):
        raise ValueError('native invocation did not complete every case')
    if any(case.find(key) is not None for case in cases for key in ['failure','error','skipped']):
        raise ValueError('incomplete native case')
    report = ET.parse(profile / 'cov.xml').getroot()
    source_roots = [pathlib.Path(source.text) for source in report.findall('./sources/source')]
    if len(source_roots) != 1:
        raise ValueError('ambiguous coverage source root')
    indexed = {}
    for item in report.findall('.//class'):
        path = (source_roots[0] / item.attrib['filename']).resolve()
        if path in indexed:
            raise ValueError('duplicate coverage source')
        indexed[path] = item.findall('./lines/line')
    root = pathlib.Path(options['root'])
    for source in options['sources']:
        path = pathlib.Path(source)
        lines = indexed[path]
        numbers = [int(line.attrib['number']) for line in lines]
        counts = [int(line.attrib['hits']) for line in lines]
        if not numbers or min(numbers) < 1 or min(counts) < 0 or len(set(numbers)) != len(numbers):
            raise ValueError('missing or invalid instrumented lines')
        files.append({'path':path.relative_to(root).as_posix(),'line_numbers':numbers,'execution_counts':counts})
except (OSError, ValueError, KeyError, TypeError, ET.ParseError) as error:
    print('incomplete ShellSpec coverage: ' + str(error))
    complete = False
report = {'schema':'struo.test_results.v1','target':options['target'],'runner':{'type':'shellspec','metadata':{'unit':'atomic-invocation'}},'status':status,
    'summary':{'total':1,'passed':int(status == 'passed'),'failed':int(status != 'passed'),'skipped':0,'flaky':0},
    'cases':[{'id':unit,'name':options['target'],'suite':options['target'],'status':status,'attempts':[{'status':status}],'runner_metadata':{'unit':'atomic-invocation'}}],
    'artifacts':{'logs':[str(native)],'native_results':[str(native)]},
    'coverage':{'tool':'kcov','tool_version':options['version'],'platform':options['platform'],'partial':not complete,'files':files}}
output.write_text(json.dumps(report))
raise SystemExit(0 if execution.returncode == 0 else 1)
'''

def _shellspec_coverage_impl(ctx):
    attrs = ctx["attr"]
    python = _resolve_host_executable("python3")
    shellspec = _resolve_host_executable(attrs.get("shellspec") or "shellspec")
    bash = _resolve_host_executable("bash")
    kcov = _resolve_host_executable(attrs.get("kcov") or "kcov")
    strace = _resolve_host_executable("strace")
    sources = _file_globs(attrs.get("coverage_sources") or [])
    if host_os() != "linux" or not all([python, shellspec, bash, kcov, strace]) or not sources:
        fail("ShellSpec coverage requires Linux, Python 3, Bash, kcov, strace and coverage_sources")
    args = attrs.get("args") or []
    for arg in args:
        if arg.startswith("-") and arg.split("=")[0] not in ["--require", "--load-path", "--helperdir", "--default-path", "--warnings", "--no-warnings"]:
            fail("ShellSpec coverage does not accept runner modes or filters: " + arg)
    srcs = glob(ctx["srcs"])
    specs = [path for path in srcs if path.endswith("_spec.sh")]
    if not specs:
        fail("ShellSpec coverage requires current spec sources")
    inputs = _unique(srcs + [_package_relative(ctx, path) for path in attrs.get("data") or []] + _file_globs([".shellspec", ".shellspec-local", "spec/**/*"]))
    test_inputs = [path for path in inputs if path not in sources]
    version = " ".join([host_command([binary, "--version"]).strip().split("\n")[0] for binary in [shellspec, bash, kcov]])
    platform = host_os() + "/" + host_command([python, "-c", "import platform;print(platform.machine())"]).strip()
    directory = ctx["build_dir"] + "/test"
    inventory = directory + "/inventory.json"
    results = directory + "/test_results.json"
    env = {"PATH":host_env("PATH"),"HOME":execution_path(directory + "/home")}
    env.update(attrs.get("env") or {})
    provider = {"label_id":ctx["label"]["id"],"target_kind":"shellspec_test","affected_inputs":_unique(inputs + sources),"test_discovery_inputs":test_inputs,"test_inventory":inventory,
        "coverage_selection":{"tool":"kcov","tool_version":version,"platform":platform,"inputs":test_inputs,"configuration":{"sources":sources,"args":args,"env":attrs.get("env") or {}}},
        "test_info":{"schema":"struo.test_info.v1","target":ctx["label"]["id"],"runner":{"type":"shellspec","metadata":{"unit":"atomic-invocation"}},
            "command":{"argv":[shellspec] + args + specs,"env":env,"cwd":"."},"outputs":{"results":results,"logs":[],"native_results":[],"coverage":[]},
            "listing":{"supported":True,"strategy":"atomic-native-invocation"},"filtering":{"case_filtering":"unsupported"},"sharding":{"supported":False},
            "retries":{"supported":False,"default_attempts":1},"execution":{"cacheable":False,"run_from_workspace_root":True,"timeout_ms":attrs.get("timeout_ms")},"labels":attrs.get("labels") or [],"metadata":{}}}
    if ctx["capability"] not in ["test", "test-discovery"]:
        return provider
    adapter = directory + "/coverage_adapter.py"
    config = directory + "/coverage_config.json"
    write_path(adapter, _shellspec_coverage_adapter())
    write_path(config, _json_encode({"shellspec":shellspec,"bash":bash,"kcov":kcov,"strace":strace,"root":workspace_root(),"target":ctx["label"]["id"],
        "args":args,"specs":specs,"sources":[workspace_root() + "/" + source for source in sources],"version":version,"platform":platform,"hook":_shellspec_coverage_hook()}))
    base = [python, adapter, "--struo-config", config]
    inputs = _unique(inputs + sources + [adapter, config])
    run_action(argv = base + ["--struo-inventory","--struo-results",inventory,"--struo-native-results",directory + "/listing.log"], inputs = inputs, outputs = [inventory,directory + "/listing.log"], clean_paths = [inventory], create_dirs = [directory,directory + "/home"], env = env, cacheable = False, identifier = ctx["label"]["id"] + ":coverage-discovery")
    if ctx["capability"] == "test-discovery":
        return provider
    expand_actions(implementation = "isolated_test_coverage_plan", inputs = [inventory], outputs = [results], args = {
        "target":ctx["label"]["id"],"inventory":inventory,"directory":directory,"argv":base,"inputs":inputs,"env":env,"runner_args":[],
        "filters":(ctx.get("test") or {}).get("filters") or [],"output":results,"success_exit_codes":[0,1],"identity":"struo.shellspec.coverage.v1\x00" + version})
    return provider
