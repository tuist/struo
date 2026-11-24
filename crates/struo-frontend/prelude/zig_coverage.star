def _zig_coverage_adapter():
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
unit = options['target'] + '::suite'
binary = pathlib.Path(options['binary']).resolve(strict=True)
native = pathlib.Path(args.struo_native_results)
if args.struo_inventory:
    native.write_text(json.dumps({'argv':[str(binary)],'unit':'atomic-invocation'}))
    output.write_text(json.dumps({'schema':'struo.test_inventory.v1','target':options['target'],'units':[unit]}))
    raise SystemExit(0)
if args.struo_test_unit != unit:
    raise SystemExit('coverage requires the complete Zig invocation')
directory = output.parent / 'coverage'
shutil.rmtree(directory, ignore_errors=True)
sources = [str(pathlib.Path(source).resolve()) for source in options['sources']]
command = [options['kcov'],'--debug=2','--clean','--cobertura-only','--skip-solibs','--include-path=' + ','.join(sources),str(directory),str(binary)]
with native.open('wb') as stream:
    execution = subprocess.run(command, stdout=stream, stderr=stream, timeout=options.get('timeout_ms') / 1000 if options.get('timeout_ms') else None)
status = 'passed' if execution.returncode == 0 else 'failed'
complete = status == 'passed'
files = []
try:
    if native.stat().st_size > 16 * 1024 * 1024:
        raise ValueError('Zig observation exceeds limit')
    log = native.read_text()
    roots = re.findall(r'^PT forked ([0-9]+)$', log, re.M)
    if len(roots) != 1 or 'PT fork/clone ' in log or 'PT new child ' in log or not re.search(r'^PT exit 0 .* for ' + roots[0] + r' \\(first child\\)$', log, re.M):
        raise ValueError('missing or additional native execution boundary')
    if options['version'] not in ['kcov 43', 'kcov v43']:
        raise ValueError('unverified kcov process-trace version')
    if not re.search(r'^All [1-9][0-9]* tests passed\\.$', log, re.M):
        raise ValueError('missing complete native Zig summary')
    xml = directory / 'cov.xml'
    if xml.stat().st_size > 16 * 1024 * 1024:
        raise ValueError('coverage export exceeds limit')
    root = ET.parse(xml).getroot()
    origin = root.findtext('./sources/source')
    indexed = {}
    for entry in root.findall('.//class'):
        path = pathlib.Path(origin, entry.attrib['filename']).resolve()
        if path in indexed:
            raise ValueError('duplicate instrumented file')
        counts = {}
        for line in entry.findall('./lines/line'):
            number, hits = int(line.attrib['number']), int(line.attrib['hits'])
            if number < 1 or hits < 0 or number in counts:
                raise ValueError('invalid coverage counter')
            counts[number] = hits
        indexed[path] = counts
    for source in sources:
        absolute = pathlib.Path(source)
        counts = indexed[absolute]
        if not counts:
            raise ValueError('missing instrumented lines')
        numbers = sorted(counts)
        files.append({'path':absolute.relative_to(pathlib.Path.cwd()).as_posix(),'line_numbers':numbers,'execution_counts':[counts[line] for line in numbers]})
except (OSError, ValueError, KeyError, TypeError, ET.ParseError) as error:
    print('incomplete Zig coverage: ' + str(error))
    complete = False
report = {'schema':'struo.test_results.v1','target':options['target'],'runner':{'type':'zig_test','metadata':{'unit':'atomic-invocation'}},'status':status,
    'summary':{'total':1,'passed':int(status == 'passed'),'failed':int(status != 'passed'),'skipped':0,'flaky':0},
    'cases':[{'id':unit,'name':options['name'],'suite':options['target'],'status':status,'attempts':[{'status':status}],'runner_metadata':{'unit':'atomic-invocation'}}],
    'artifacts':{'logs':[str(native)],'native_results':[str(native)]},
    'coverage':{'tool':'kcov','tool_version':options['version'],'platform':options['platform'],'partial':not complete,'files':files}}
output.write_text(json.dumps(report))
raise SystemExit(0 if execution.returncode == 0 else 1)
'''

def _zig_coverage_configuration(ctx, provider):
    sources = _zig_globs(_zig_attr(ctx, "coverage_sources", []))
    python = _resolve_host_executable("python3")
    kcov = _resolve_host_executable(_zig_attr(ctx, "kcov", "kcov"))
    if host_os() != "linux" or not python or not kcov or not sources:
        fail("coverage_per_test requires Linux, Python 3, kcov and declared coverage_sources")
    if _zig_attr(ctx, "test_runner", "") or _zig_attr(ctx, "args", []) or _zig_attr(ctx, "target", ""):
        fail("coverage_per_test requires the complete native host Zig invocation without a custom runner or arguments")
    version = host_command([kcov, "--version"], merge_stderr = True).strip()
    zig, identity = _zig_toolchain(ctx)
    inputs = _unique(_zig_source_inputs(ctx, _zig_main(ctx, "")) + _zig_data_inputs(ctx) + _zig_collect_dep_sources(ctx["deps"]) + _zig_collect_dep_data(ctx["deps"]))
    test_inputs = [path for path in inputs if path not in sources]
    provider["affected_inputs"] = _unique(inputs + sources)
    provider["test_discovery_inputs"] = test_inputs
    provider["test_inventory"] = ctx["build_dir"] + "/test/inventory.json"
    platform = "linux/" + host_command([python, "-c", "import platform;print(platform.machine())"]).strip()
    provider["coverage_selection"] = {"tool":"kcov","tool_version":version,"platform":platform,"inputs":test_inputs,
        "configuration":{"runtime":_portable_test_configuration(ctx, [], [identity, version], _zig_run_env(ctx)),"sources":sources}}
    provider["test_info"]["listing"] = {"supported":True,"strategy":"atomic-invocation"}
    provider["test_info"]["filtering"] = {"case_filtering":"exact"}
    provider["test_info"]["metadata"]["unit"] = "atomic-invocation"
    return {"python":python,"kcov":kcov,"sources":sources,"version":version,"platform":platform,"inputs":inputs}

def _zig_coverage_actions(ctx, provider, tools):
    directory = ctx["build_dir"] + "/test"
    adapter = directory + "/coverage_adapter.py"
    config = directory + "/coverage_config.json"
    inventory = provider["test_inventory"]
    write_path(adapter, _zig_coverage_adapter())
    write_path(config, _json_encode({"binary":provider["test_binary"],"target":ctx["label"]["id"],"name":ctx["label"]["name"],"sources":tools["sources"],
        "kcov":tools["kcov"],"version":tools["version"],"platform":tools["platform"],"timeout_ms":_zig_attr(ctx,"timeout_ms",None)}))
    inputs = _unique(tools["inputs"] + tools["sources"] + [provider["test_binary"],adapter,config])
    argv = [tools["python"],adapter,"--struo-config",config]
    env = _zig_run_env(ctx, directory)
    run_action(argv = argv + ["--struo-results",inventory,"--struo-native-results",directory + "/listing.json","--struo-inventory"], inputs = inputs, outputs = [inventory,directory + "/listing.json"], clean_paths = [inventory], create_dirs = [directory], env = env, cacheable = False, identifier = ctx["label"]["id"] + ":coverage-discovery")
    if ctx["capability"] == "test-discovery":
        return
    expand_actions(implementation = "isolated_test_coverage_plan", inputs = [inventory], outputs = [directory + "/test_results.json"], args = {
        "target":ctx["label"]["id"],"inventory":inventory,"directory":directory,"argv":argv,"inputs":inputs,"env":env,"runner_args":[],
        "filters":(ctx.get("test") or {}).get("filters") or [],"output":directory + "/test_results.json","success_exit_codes":[0,1,2],
        "identity":"struo.zig.coverage.v1\x00" + tools["version"],
    })
