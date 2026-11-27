def _cmake_coverage_adapter():
    return _llvm_coverage_adapter() + '''import argparse
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
base = [options['ctest'], '--test-dir', options['build']] + (['--build-config', options['build_type']] if options['build_type'] else [])
listing_path = output.parent / 'current-listing.json'
with listing_path.open('wb') as stream:
    listing = subprocess.run(base + ['--show-only=json-v1'], stdout=stream)
if listing.returncode:
    raise SystemExit(2)
if listing_path.stat().st_size > 16 * 1024 * 1024:
    raise SystemExit('CTest inventory exceeds observation limit')
tests = json.loads(listing_path.read_text())['tests']
names = [test['name'] for test in tests]
if len(names) != len(set(names)) or not names:
    raise SystemExit('coverage requires a nonempty unambiguous CTest inventory')
if args.struo_inventory:
    shutil.copyfile(listing_path, args.struo_native_results)
    output.write_text(json.dumps({'schema':'struo.test_inventory.v1', 'target':options['target'], 'units':[options['target'] + '::' + name for name in sorted(names)]}))
    raise SystemExit(0)
prefix = options['target'] + '::'
if not args.struo_test_unit or not args.struo_test_unit.startswith(prefix):
    raise SystemExit('coverage requires one exact CTest invocation')
name = args.struo_test_unit[len(prefix):]
test = next(test for test in tests if test['name'] == name)
directory = str(output.parent.resolve() / 'coverage')
shutil.rmtree(directory, ignore_errors=True)
pathlib.Path(directory).mkdir()
native = str(pathlib.Path(args.struo_native_results).resolve())
log = str(output.parent.resolve() / 'runner.log')
trace = directory + '/launcher-processes'
command = base + ['--parallel', '1', '--no-tests=error', '--output-on-failure', '--output-junit', native, '-R', '^' + re.escape(name) + '$']
if options.get('timeout_ms'):
    command += ['--timeout', str(options['timeout_ms'] / 1000)]
with open(log, 'wb') as stream:
    execution = subprocess.run([options['trace'], '-f', '-e', 'trace=process', '-e', 'raw=execve,execveat', '-o', trace, *command], env={**os.environ, 'LLVM_PROFILE_FILE':directory + '/%p.profraw'}, stdout=stream, stderr=stream)
status = 'passed' if execution.returncode == 0 else 'failed'
report = {'schema':'struo.test_results.v1', 'target':options['target'], 'runner':{'type':'cmake_test','metadata':{}}, 'status':status,
          'summary':{'total':1,'passed':int(status == 'passed'),'failed':int(status != 'passed'),'skipped':0,'flaky':0},
          'cases':[{'id':args.struo_test_unit,'name':name,'suite':options['target'],'status':status,'attempts':[{'status':status}], 'runner_metadata':{'unit':'ctest-invocation'}}],
          'artifacts':{'logs':[log],'native_results':[native]}}
try:
    root = ET.parse(native).getroot()
    cases = root.findall('.//testcase')
    if len(cases) != 1 or cases[0].get('name') != name or cases[0].find('skipped') is not None:
        raise ValueError('CTest did not execute exactly the requested invocation')
    path = pathlib.Path(trace)
    if path.stat().st_size > 16 * 1024 * 1024:
        raise ValueError('CTest process trace exceeds observation limit')
    lines = path.read_text().splitlines(True)
    execs = [line.split()[0] for line in lines if re.search(r'\\bexecve(?:at)?\\(', line)]
    if len(execs) != 2 or execs[0] == execs[1]:
        raise ValueError('CTest invocation launched extra or missing executables')
    launcher, child = execs
    launches = [line for line in lines if line.split()[0] == launcher and re.search(r'\\b(?:clone3?|fork|vfork)\\(', line)]
    successful = [line for line in launches if re.search(r'= [0-9]+\\s*$', line)]
    if len(successful) != 1 or not re.search(r'= ' + re.escape(child) + r'\\s*$', successful[0]):
        raise ValueError('unrecognized CTest process boundary')
    pathlib.Path(directory, 'processes').write_text(''.join(line for line in lines if line.split()[0] != launcher))
    options['binary'] = test['command'][0]
    report = llvm_observation(options, report, directory)
except (OSError, ValueError, KeyError, TypeError, ET.ParseError) as error:
    print('incomplete CTest coverage: ' + str(error))
    report['coverage'] = {'tool':'llvm','tool_version':options['version'],'platform':platform.system().lower() + '/' + platform.machine(),'partial':True,'files':[]}
output.write_text(json.dumps(report))
raise SystemExit(0 if execution.returncode == 0 else 1)
'''

def _cmake_coverage_configuration(ctx, provider, cmake, ctest, env):
    sources = _file_globs(_cmake_attr(ctx, "coverage_sources", []))
    python = _resolve_host_executable("python3")
    trace = _resolve_host_executable("strace") if host_os() == "linux" else ""
    cov = _resolve_host_executable(_cmake_attr(ctx, "llvm_cov", "llvm-cov"))
    profdata = _resolve_host_executable(_cmake_attr(ctx, "llvm_profdata", "llvm-profdata"))
    if not sources or not python or not trace or not cov or not profdata or not ctest:
        fail(ctx["label"]["id"] + ": coverage_per_test requires CTest mode, coverage_sources, Python 3, Linux strace and compiler-matching llvm_cov/llvm_profdata")
    inputs = _unique(glob(ctx["srcs"]) + _cmake_globs(_cmake_attr(ctx, "data", [])))
    test_inputs = [path for path in inputs if path not in sources]
    version = host_command([cov, "--version"]).strip()
    compilers = [env.get("CC") or host_env("CC") or "cc", env.get("CXX") or host_env("CXX") or "c++"]
    for argument in _cmake_attr(ctx, "configure_args", []):
        key, separator, value = argument.partition("=")
        if separator and key.startswith("-DCMAKE_") and key.split(":")[0].endswith("_COMPILER"):
            compilers.append(value)
    versions = [host_command([cmake, "--version"]), host_command([ctest, "--version"]), version]
    for compiler in _unique(compilers):
        path = _resolve_host_executable(compiler)
        if not path:
            fail("coverage_per_test cannot resolve compiler identity: " + compiler)
        versions.append(host_command([path, "--version"], merge_stderr = True))
    configuration = _portable_test_configuration(ctx, [], versions, env)
    provider["affected_inputs"] = _unique(inputs + sources)
    provider["test_discovery_inputs"] = test_inputs
    provider["coverage_selection"] = {"tool":"llvm", "tool_version":version, "platform":"linux/" + host_command([python, "-c", "import platform;print(platform.machine())"]).strip(), "inputs":test_inputs, "configuration":{"sources":sources,"runtime":configuration}}
    provider["test_info"]["listing"] = {"supported":True,"strategy":"ctest-invocation"}
    provider["test_info"]["filtering"] = {"case_filtering":"exact"}
    provider["test_inventory"] = ctx["build_dir"] + "/test/inventory.json"
    return {"python":python,"trace":trace,"cov":cov,"profdata":profdata,"sources":sources,"version":version}

def _cmake_coverage_actions(ctx, provider, tools, ctest, env, inputs, build_root, build_type):
    directory = ctx["build_dir"] + "/test"
    adapter = directory + "/coverage_adapter.py"
    config = directory + "/coverage_config.json"
    inventory = directory + "/inventory.json"
    write_path(adapter, _cmake_coverage_adapter())
    write_path(config, _json_encode({"ctest":ctest,"build":build_root,"build_type":build_type,"target":ctx["label"]["id"],"sources":tools["sources"],"cov":tools["cov"],"profdata":tools["profdata"],"trace":tools["trace"],"version":tools["version"],"timeout_ms":_cmake_attr(ctx,"timeout_ms",None)}))
    inputs = _unique(inputs + tools["sources"] + [build_root, adapter, config])
    base = [tools["python"], adapter, "--struo-config", config]
    run_action(argv = base + ["--struo-results",inventory,"--struo-native-results",directory + "/listing.json","--struo-inventory"], inputs = inputs, outputs = [inventory,directory + "/listing.json"], clean_paths = [inventory], create_dirs = [directory], env = env, cacheable = False, identifier = ctx["label"]["id"] + ":coverage-discovery")
    if ctx["capability"] == "test-discovery":
        return
    expand_actions(implementation = "isolated_test_coverage_plan", inputs = [inventory], outputs = [directory + "/test_results.json"], args = {
        "target":ctx["label"]["id"],"inventory":inventory,"directory":directory,"argv":base,"inputs":inputs,"env":env,"runner_args":[],
        "filters":(ctx.get("test") or {}).get("filters") or [],"output":directory + "/test_results.json","success_exit_codes":[0,1,2],"identity":"struo.ctest.coverage.v1\x00" + tools["version"],
        "resources":{"memory_bytes":1024 * 1024 * 1024},
    })
