def _apple_coverage_process_hook():
    return '''#include <dlfcn.h>
#include <fcntl.h>
#include <spawn.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

static void record(const char *name) {
    const char *path = getenv(name);
    if (!path) return;
    int fd = open(path, O_WRONLY | O_CREAT | O_APPEND, 0600);
    if (fd < 0) return;
    pid_t pid = getpid();
    (void)write(fd, &pid, sizeof(pid));
    (void)close(fd);
}
__attribute__((constructor)) static void loaded(void) { record("STRUO_COVERAGE_LOADED"); }
static pid_t tracked_fork(void) {
    record("STRUO_COVERAGE_CHILD");
    return fork();
}
static pid_t tracked_vfork(void) {
    record("STRUO_COVERAGE_CHILD");
    __attribute__((musttail)) return vfork();
}
static int tracked_system(const char *command) {
    record("STRUO_COVERAGE_CHILD");
    return system(command);
}
static FILE *tracked_popen(const char *command, const char *mode) {
    record("STRUO_COVERAGE_CHILD");
    return popen(command, mode);
}
static int tracked_spawn(pid_t *pid, const char *file, const posix_spawn_file_actions_t *actions,
                         const posix_spawnattr_t *attributes, char *const argv[], char *const env[]) {
    record("STRUO_COVERAGE_CHILD");
    return posix_spawn(pid, file, actions, attributes, argv, env);
}
static int tracked_spawnp(pid_t *pid, const char *file, const posix_spawn_file_actions_t *actions,
                          const posix_spawnattr_t *attributes, char *const argv[], char *const env[]) {
    record("STRUO_COVERAGE_CHILD");
    return posix_spawnp(pid, file, actions, attributes, argv, env);
}
static int tracked_execve(const char *file, char *const argv[], char *const env[]) {
    record("STRUO_COVERAGE_CHILD");
    return execve(file, argv, env);
}
#define INTERPOSE(replacement, original) \\
    __attribute__((used)) static struct { const void *replace; const void *original; } \\
    interpose_##original __attribute__((section("__DATA,__interpose"))) = { \\
        (const void *)&replacement, (const void *)&original }
INTERPOSE(tracked_fork, fork);
INTERPOSE(tracked_vfork, vfork);
INTERPOSE(tracked_system, system);
INTERPOSE(tracked_popen, popen);
INTERPOSE(tracked_spawn, posix_spawn);
INTERPOSE(tracked_spawnp, posix_spawnp);
INTERPOSE(tracked_execve, execve);
'''

def _apple_coverage_adapter():
    return _llvm_coverage_adapter() + '''import argparse
import struct

parser = argparse.ArgumentParser()
parser.add_argument('configuration')
args = parser.parse_args()
options = json.loads(pathlib.Path(args.configuration).read_text())
output = pathlib.Path(options['results'])
native = json.loads(output.read_text())
cases = native.get('cases', [])
complete = native.get('status') == 'passed' and bool(cases) and all(case.get('status') == 'passed' for case in cases)
directory = options['directory']
try:
    loaded = pathlib.Path(directory, 'loaded')
    complete = complete and loaded.stat().st_size == struct.calcsize('i') and not pathlib.Path(directory, 'children').exists()
    if native['runner']['type'] != 'swift_testing':
        log = pathlib.Path(options['native']).read_text()
        counts = re.findall(r'Executed ([0-9]+) tests?, with 0 failures', log)
        complete = complete and bool(counts) and int(counts[-1]) == len(cases)
except (OSError, ValueError, KeyError):
    complete = False
unit = options['target'] + '::invocation'
status = native['status']
report = {'schema':'struo.test_results.v1','target':options['target'],'runner':{'type':native['runner']['type'],'metadata':{'unit':'atomic-invocation'}},'status':status,
    'summary':{'total':1,'passed':int(status == 'passed'),'failed':int(status != 'passed'),'skipped':0,'flaky':0},
    'cases':[{'id':unit,'name':options['target'],'suite':options['target'],'status':status,'attempts':[{'status':status}],'runner_metadata':{'unit':'atomic-invocation'}}],
    'artifacts':native.get('artifacts', {})}
report = llvm_observation(options, report, directory, process_complete=complete)
report['coverage']['platform'] = options['platform']
pathlib.Path(options['observation']).write_text(json.dumps(report))
'''

def _apple_coverage_configuration(ctx, provider, swiftc, sources, test_inputs):
    python = _resolve_host_executable("python3")
    if not sources or not python:
        fail("coverage_per_test requires Python 3 and declared instrumented coverage_sources")
    if any([source in test_inputs for source in sources]):
        fail("coverage_sources must not contain the test bundle's own registration sources")
    env = swiftc["env"]
    xcrun = host_which("xcrun")
    cov = host_command([xcrun, "--find", "llvm-cov"], env = env).strip()
    profdata = host_command([xcrun, "--find", "llvm-profdata"], env = env).strip()
    version = host_command([cov, "--version"], env = env).strip() + " " + host_command([swiftc["swiftc_path"], "--version"], env = env).strip()
    platform = host_command([python, "-c", "import platform;print(platform.system().lower() + '/' + platform.machine())"]).strip()
    provider["affected_inputs"] = _unique(provider["affected_inputs"] + sources)
    provider["test_discovery_inputs"] = test_inputs
    provider["test_inventory"] = ctx["build_dir"] + "/test/inventory.json"
    provider["coverage_selection"] = {"tool":"llvm","tool_version":version,"platform":platform,"inputs":test_inputs,
        "configuration":{"sources":sources,"runtime":_portable_test_configuration(ctx, [], [swiftc["identity"], version], ctx["attr"].get("test_env") or {})}}
    provider["test_info"]["listing"] = {"supported":True,"strategy":"atomic-invocation"}
    provider["test_info"]["filtering"] = {"case_filtering":"exact"}
    provider["test_info"]["sharding"] = {"supported":False,"granularity":"target"}
    provider["test_info"]["metadata"]["unit"] = "atomic-invocation"
    return {"python":python,"cov":cov,"profdata":profdata,"version":version,"platform":platform,"sources":sources}

def _apple_coverage_hook_action(ctx, clang, codesign, triple):
    source = declare_output("coverage/process_hook.c")
    library = declare_output("coverage/process_hook.dylib")
    write_path(source, _apple_coverage_process_hook())
    run_action(argv = [clang["clang_path"], "-dynamiclib", "-target", triple, "-isysroot", clang["sdk_path"], source, "-o", library],
        inputs = [source], outputs = [library], env = clang["env"], toolchain_identity = clang["identity"], identifier = ctx["label"]["id"] + ":coverage-hook")
    run_action(argv = [codesign["codesign_path"], "--force", "--sign", "-", "--timestamp=none", library], inputs = [library], outputs = [library], env = codesign["env"], identifier = ctx["label"]["id"] + ":coverage-hook-sign")
    return library

def _apple_coverage_finish(ctx, tools, binary, results, native_results, hook):
    directory = ctx["build_dir"] + "/test/coverage"
    observation = directory + "/observation.json"
    adapter = declare_output("coverage/adapter.py")
    config = declare_output("coverage/config.json")
    write_path(adapter, _apple_coverage_adapter())
    write_path(config, _json_encode({"binary":binary,"target":ctx["label"]["id"],"results":results,"observation":observation,"native":native_results,"directory":directory,
        "cov":tools["cov"],"profdata":tools["profdata"],"version":tools["version"],"platform":tools["platform"],"sources":tools["sources"]}))
    run_action(argv = [tools["python"],adapter,config], inputs = [results,native_results,directory,adapter,config,hook,binary] + tools["sources"],
        outputs = [observation], cacheable = False, identifier = ctx["label"]["id"] + ":coverage-convert")
    merge_test_coverage(ctx["label"]["id"], ctx["build_dir"] + "/test/inventory.json", [observation], results, partial = bool((ctx.get("test") or {}).get("filters")))

def _apple_coverage_native_specs(specs):
    indexed = {spec["name"]: spec for spec in specs}
    compilable = ["apple_library", "apple_framework", "apple_application", "apple_executable"]
    for spec in specs:
        if spec["kind"] != "apple_test_bundle":
            continue
        pending = list(spec.get("deps") or [])
        seen = {}
        sources = []
        for _ in range(len(specs) + 1):
            following = []
            for reference in pending:
                name = reference[2:] if reference.startswith("./") else reference
                dependency = indexed.get(name)
                if not dependency or name in seen:
                    continue
                seen[name] = True
                if dependency["kind"] not in compilable:
                    continue
                attrs = dependency["attrs"]
                attrs["swift_flags"] = _unique((attrs.get("swift_flags") or []) + ["-profile-generate", "-profile-coverage-mapping"])
                attrs["clang_flags"] = _unique((attrs.get("clang_flags") or []) + ["-fprofile-instr-generate", "-fcoverage-mapping"])
                sources.extend([source for source in _file_globs(dependency.get("srcs") or []) if any([source.endswith(extension) for extension in [".swift", ".m", ".mm", ".c", ".cc", ".cpp", ".cxx"]])])
                following.extend(dependency.get("deps") or [])
            if not following:
                break
            pending = following
        if sources:
            spec["attrs"]["coverage_per_test"] = True
            spec["attrs"]["coverage_sources"] = _unique(sources)
