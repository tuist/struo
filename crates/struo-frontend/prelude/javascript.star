_JAVASCRIPT_TOOL = tool("node", executables = ["node"])
_VITEST_TOOL = tool("vitest", executables = ["vitest"])
_JEST_TOOL = tool("jest", executables = ["jest"])
_JAVASCRIPT_TEST_CONFIG = ["package.json", "package-lock.json", "pnpm-lock.yaml", "yarn.lock"]
_JAVASCRIPT_TEST_DEPENDENCIES = ["node_modules/**/*"]
# Test runners never read these, and they differ between installs or
# checkouts: pnpm's install state carries a timestamp and the absolute store
# path, pnpm's `.bin` shims embed absolute paths, and tools write their own
# caches under `.cache`. The runner is resolved through its package, not `.bin`.
_JAVASCRIPT_PACKAGE_MANAGER_STATE = [
    "**/node_modules/.modules.yaml",
    "**/node_modules/.pnpm-workspace-state*.json",
    "**/node_modules/.bin/**",
    "**/node_modules/.cache/**",
]

def _javascript_attr(ctx, name, default):
    return _configured_attr(ctx, name, default)

def _javascript_node(ctx):
    requested = _javascript_attr(ctx, "node", "node")
    resolved = _resolve_host_executable(requested)
    if not resolved:
        fail(ctx["label"]["id"] + ": Node.js executable `" + requested + "` was not found")
    return resolved

def _javascript_stage_project(ctx, dependency, files):
    stage = ctx["build_dir"] + "/project"
    copy_path(
        dependency["javascript_root"],
        stage,
        kind = "tree",
        inputs = [dependency["javascript_root"]],
        identifier = ctx["label"]["id"] + ":stage-dependencies",
        source_files = _action_source_files([dependency["javascript_root"]]),
    )
    package = dependency["javascript_source_root"]
    prefix = package + "/" if package else ""
    for source in files:
        relative = source[len(prefix):] if prefix and source.startswith(prefix) else source
        copy_path(
            source,
            stage + "/" + relative,
            inputs = [source],
            identifier = ctx["label"]["id"] + ":stage:" + relative,
            source_files = _action_source_files([source]),
        )
    return stage

def _javascript_build_impl(ctx):
    dependencies = [dep for dep in ctx["deps"] if dep.get("javascript_dependency_set")]
    if len(dependencies) != 1:
        fail(ctx["label"]["id"] + ": expected exactly one javascript_dependency_set dependency")
    dependency = dependencies[0]
    package_dir = _javascript_attr(ctx, "package_dir", ".")
    outputs = _javascript_attr(ctx, "outputs", [])
    for path in [package_dir] + outputs:
        if not path or path.startswith("/") or "\\" in path or ":" in path or ".." in path.split("/"):
            fail(ctx["label"]["id"] + ": package_dir and outputs must be relative paths inside the staged workspace")
    if not outputs or [path for path in outputs if "." in path.split("/") or "" in path.split("/")]:
        fail(ctx["label"]["id"] + ": outputs must name build products, not the workspace root")
    files = glob(ctx["srcs"])
    stage = _javascript_stage_project(ctx, dependency, files)
    tools = dependency["javascript_tools"]
    node = _javascript_node(ctx)
    node_version = host_command([node, "--version"], cwd = workspace_root() + "/" + ctx["label"]["package"]).strip()
    home = ctx["scratch_dir"] + "/home"
    products = [stage + "/" + path for path in outputs]
    env = {
        "CI": "1",
        "HOME": execution_path(home),
        "PATH": _parent_dir(node) + ":" + _parent_dir(tools["executable"]) + ":/usr/bin:/bin",
    }
    if tools["name"] == "pnpm":
        # The dependency target already installed this tree and leaves out
        # pnpm's install state, which `run` and `exec` would otherwise check
        # and answer with a fresh install. pnpm sets the same opt-out, under
        # its pnpm 10 and pnpm 11 names, for the scripts it runs itself.
        env["npm_config_verify_deps_before_run"] = "false"
        env["pnpm_config_verify_deps_before_run"] = "false"
    env.update(_javascript_attr(ctx, "env", {}))
    args = _javascript_attr(ctx, "args", [])
    script = _javascript_attr(ctx, "script", "")
    command = _javascript_attr(ctx, "command", "")
    if bool(script) == bool(command):
        fail(ctx["label"]["id"] + ": set exactly one of `script`, a package.json script, or `command`, an installed package binary such as `expo`")
    if script:
        argv = [tools["executable"], "run", script]
        if args and tools["name"] == "npm":
            argv.append("--")
    elif tools["name"] == "npm":
        # `--no` refuses to download a binary the lockfile did not install.
        argv = [tools["executable"], "exec", "--no", "--", command]
    elif tools["name"] == "pnpm":
        argv = [tools["executable"], "exec", command]
    elif tools["name"] == "yarn" and not tools.get("version", "").startswith("1."):
        argv = [tools["executable"], "run", "--binaries-only", command]
    else:
        # Yarn 1 and Bun run installed binaries through `run`, where a
        # package.json script of the same name takes precedence.
        argv = [tools["executable"], "run", command]
    argv.extend(args)
    run_action(
        display_name = "Build JavaScript package · " + ctx["label"]["name"],
        source_files = _action_source_files(files),
        argv = argv,
        inputs = [stage],
        outputs = products,
        clean_paths = products,
        create_dirs = [home],
        cwd = stage + "/" + package_dir,
        env = env,
        toolchain_identity = tools["identity"] + "\x00build-node\x00" + node + "\x00" + node_version,
        identifier = ctx["label"]["id"] + ":javascript-build",
    )
    return {"label_id": ctx["label"]["id"], "target_kind": "javascript_build", "products": products}

javascript_build = target_kind(
    docs = "Runs a package manager script in a private copy of a locked JavaScript workspace and caches its declared build products.",
    attrs = [
        attr("script", "string", default = "\"\"", docs = "Package script passed to the locked dependency set's package manager with `run`. Set exactly one of `script` and `command`.", configurable = False),
        attr("command", "string", default = "\"\"", docs = "Installed package binary, such as `expo`, run through the package manager: `npm exec --no --`, `pnpm exec`, or `run` for Yarn and Bun. Set exactly one of `script` and `command`.", configurable = False),
        attr("package_dir", "string", default = "\".\"", docs = "Workspace-relative package directory used as the working directory.", configurable = False),
        attr("outputs", "list<string>", required = True, docs = "Build product paths relative to the staged workspace root, not package_dir.", configurable = False),
        attr("args", "list<string>", default = "[]", docs = "Additional script or command arguments.", configurable = True),
        attr("env", "map<string,string>", default = "{}", docs = "Explicit non-secret build environment.", configurable = True),
        attr("node", "string", default = "\"node\"", docs = "Node.js executable.", configurable = False),
    ],
    deps = [dep("deps", ["javascript_dependency_set"], "Locked workspace installation.", min_count = 1, max_count = 1)],
    capabilities = [capability("build", ["default", "products"])],
    tools = [_JAVASCRIPT_TOOL],
    examples = [example("react-native-application-minimal", name = "JavaScript library declarations", use_when = "Build a TypeScript library alongside a React Native application.", platforms = ["macos"])],
    impl = _javascript_build_impl,
)

def _javascript_env(ctx, test_dir, node):
    env = {"HOME": execution_path(test_dir + "/home"), "CI": "1"}
    node_dir = _parent_dir(node)
    if node_dir:
        env["PATH"] = node_dir
    for name in _javascript_attr(ctx, "env_inherit", []):
        value = host_env(name)
        if value:
            env[name] = value
    for name, value in _javascript_attr(ctx, "env", {}).items():
        env[name] = value
    return env

def _javascript_installed_package_entry(resolved, default):
    normalized = resolved.replace("\\", "/")
    bin_dir = _parent_dir(normalized)
    node_modules_dir = _parent_dir(bin_dir)
    prefix = "node_modules/"
    if not bin_dir.endswith("/.bin") or not default.startswith(prefix):
        return ""
    return _resolve_host_executable(node_modules_dir + "/" + default[len(prefix):])

def _javascript_runner(ctx, runner_type, default):
    requested = ctx["attr"].get("runner")
    if requested:
        if "/" not in requested and "\\" not in requested:
            resolved = _resolve_host_executable(requested)
            if resolved:
                return (resolved, False, False)
        absolute = requested.startswith("/") or (len(requested) > 2 and requested[1] == ":" and (requested[2] == "/" or requested[2] == "\\"))
        package_path = requested if absolute else _package_relative(ctx, requested)
        resolved = _resolve_host_executable(package_path)
        if not resolved:
            fail(ctx["label"]["id"] + ": " + runner_type + " runner `" + requested + "` was not found")
        installed_entry = _javascript_installed_package_entry(resolved, default)
        if installed_entry:
            return (installed_entry, installed_entry.endswith(".js") or installed_entry.endswith(".mjs"), False)
        return (resolved if absolute else package_path, requested.endswith(".js") or requested.endswith(".mjs"), not absolute)

    package_path = _package_relative(ctx, default)
    if _resolve_host_executable(package_path):
        return (package_path, True, True)
    resolved = _resolve_host_executable(runner_type)
    if resolved:
        installed_entry = _javascript_installed_package_entry(resolved, default)
        if installed_entry:
            return (installed_entry, installed_entry.endswith(".js") or installed_entry.endswith(".mjs"), False)
        return (resolved, False, False)
    fail(ctx["label"]["id"] + ": " + runner_type + " runner was not found; install it in the workspace or make `" + runner_type + "` available on PATH")

def _javascript_discovery_inputs(ctx):
    return _unique(glob(ctx["srcs"]) + _file_globs(_javascript_attr(ctx, "config", _JAVASCRIPT_TEST_CONFIG)) + _file_globs(_javascript_attr(ctx, "data", [])))

def _javascript_test_inputs(ctx, runner, runner_is_workspace_input, discovery_inputs):
    runner_inputs = glob([runner]) if runner_is_workspace_input else []
    return _unique(discovery_inputs + _file_globs(_javascript_attr(ctx, "dependencies", _JAVASCRIPT_TEST_DEPENDENCIES), exclude = _JAVASCRIPT_PACKAGE_MANAGER_STATE) + runner_inputs)

def _javascript_test_adapter():
    return _javascript_coverage_adapter() + '''import { spawnSync } from "node:child_process"
import fs from "node:fs"
import { createRequire } from "node:module"
import path from "node:path"

const workspaceRoot = process.cwd()

function parseArgs(argv) {
  const options = { sources: [], units: [], runnerArgs: [], coverage_sources: [] }
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index]
    if (argument === "--struo-inventory") { options.inventory = true; continue }
    if (argument === "--") {
      options.runnerArgs = argv.slice(index + 1)
      break
    }
    const key = argument.replace(/^--struo-/, "").replaceAll("-", "_")
    const value = argv[index + 1]
    index += 1
    if (key === "source") options.sources.push(value)
    else if (key === "test_unit") options.units.push(value)
    else if (key === "coverage_source") options.coverage_sources.push(value)
    else options[key] = value
  }
  return options
}

function escapePattern(value) {
  const special = "^$.*+?()[]{}|"
  return [...value].map(character =>
    special.includes(character) || character.charCodeAt(0) === 92
      ? String.fromCharCode(92) + character
      : character
  ).join("")
}

function decodeUnits(options) {
  const prefix = options.target + "::"
  return options.units.map(unit => {
    if (!unit.startsWith(prefix)) throw new Error("test unit does not belong to target: " + unit)
    const decoded = JSON.parse(decodeURIComponent(unit.slice(prefix.length)))
    if (!Array.isArray(decoded) || decoded.length !== 2) throw new Error("invalid test unit: " + unit)
    return { file: decoded[0], name: decoded[1] }
  })
}

function normalizedStatus(status) {
  if (status === "passed") return "passed"
  if (["pending", "todo", "disabled", "skipped"].includes(status)) return "skipped"
  return "failed"
}

const ANSI_ESCAPE = new RegExp(String.fromCharCode(27) + "[[][0-9;]*m", "g")

function attemptFor(status, assertion) {
  const attempt = { status }
  if (status !== "skipped" && Number.isFinite(assertion.duration) && assertion.duration >= 0) {
    attempt.duration_ms = Math.round(assertion.duration)
  }
  const messages = (assertion.failureMessages || []).filter(message => typeof message === "string" && message.length)
  if (status === "failed" && messages.length) {
    const message = messages.join(String.fromCharCode(10)).replace(ANSI_ESCAPE, "")
    attempt.failure = { message: Array.from(message).slice(0, 4000).join("") }
  }
  return attempt
}

function relativeFile(value) {
  const relative = path.relative(workspaceRoot, path.resolve(value))
  return relative.split(path.sep).join("/")
}

// Jest indexes every file below its roots, including package copies Struo
// stages under `.struo`, and a command-line ignore list replaces the
// configured one. When the runner is Jest's own entry point, resolve its
// configuration the way jest-cli does and pass the configured ignores along
// with Struo's state directory. A wrapper runner or a multi-project
// configuration whose projects ignore different paths cannot be expressed as
// one command-line list, so those keep their own configuration.
async function jestIgnoreArgs(options) {
  const runner = fs.realpathSync(options.runner).split(path.sep).join("/")
  if (!/\/(jest|jest-cli)\/bin\/jest\.js$/.test(runner)) return []
  let configs
  try {
    const fromCli = createRequire(createRequire(runner).resolve("jest-cli"))
    const argv = await fromCli("yargs")(options.runnerArgs).options(fromCli("jest-cli").yargsOptions).argv
    const projects = argv.projects?.length ? argv.projects : [process.cwd()]
    ;({ configs } = await fromCli("jest-config").readConfigs(argv, projects))
  } catch (error) {
    console.error("struo: could not read the Jest configuration, so Struo's state directory stays visible to Jest: " + (error instanceof Error ? error.message : String(error)))
    return []
  }
  const configured = configs.map(config => config.modulePathIgnorePatterns || [])
  if (configs.length > 1 && configured.some(patterns => patterns.length)) {
    console.error("struo: Jest projects configure their own modulePathIgnorePatterns, so Struo's state directory stays visible to Jest")
    return []
  }
  const patterns = new Set(configured.flat())
  patterns.add("^" + escapePattern(path.join(workspaceRoot, ".struo") + path.sep))
  return [...patterns].map(pattern => "--modulePathIgnorePatterns=" + pattern)
}

async function main() {
  const options = parseArgs(process.argv.slice(2))
  if (options.cwd) {
    for (const key of ['results', 'native_results', 'coverage_hook']) {
      if (options[key]) options[key] = path.resolve(options[key])
    }
    options.sources = options.sources.map(file => path.resolve(file))
    const sources = options.coverage_sources.map(file => path.resolve(file))
    process.chdir(options.cwd)
    options.coverage_sources = sources.map(file => path.relative(process.cwd(), file))
  }
  if (options.inventory) fs.rmSync(options.results, { force: true })
  if (options.unit === "suite" && options.units.length) throw new Error("suite units do not support test-unit filtering")
  const units = decodeUnits(options)
  const files = units.length ? [...new Set(units.map(unit => options.cwd ? path.resolve(workspaceRoot, unit.file) : unit.file))] : options.sources
  const names = units.map(unit => unit.name)
  const namePattern = options.inventory ? "(?!)" : names.length ? "^(?:" + names.map(escapePattern).join("|") + ")$" : null
  let runnerArgs
  if (options.runner_type === "vitest") {
    runnerArgs = ["run", ...options.runnerArgs, "--no-cache", "--reporter=json", "--outputFile=" + options.native_results, ...files]
    if (namePattern) runnerArgs.push("--testNamePattern", namePattern)
  } else {
    runnerArgs = [...options.runnerArgs, ...await jestIgnoreArgs(options), "--no-cache", "--runInBand", "--json", "--outputFile=" + options.native_results, ...(files.length ? ["--runTestsByPath", ...files] : [])]
    if (namePattern) runnerArgs.push("--testNamePattern", namePattern)
  }
  const command = options.runner_via_node === "true" ? process.execPath : options.runner
  const coverage = coverageConfiguration(options, runnerArgs)
  const args = options.runner_via_node === "true" ? [options.runner, ...runnerArgs] : runnerArgs
  const started = performance.now()
  fs.rmSync(options.native_results, { force: true })
  const execution = spawnSync(command, args, { stdio: "inherit", env: coverage.env })
  if (execution.error) throw execution.error
  const exitCode = execution.status ?? 1
  let native = { testResults: [] }
  if (fs.existsSync(options.native_results)) {
    native = JSON.parse(fs.readFileSync(options.native_results, "utf8"))
  } else {
    fs.writeFileSync(options.native_results, JSON.stringify(native, null, 2))
  }
  const cases = []
  const identifiers = new Set()
  let duplicates = false
  for (const suite of (options.unit === "suite" ? [] : native.testResults || [])) {
    const file = relativeFile(suite.name || "unknown")
    for (const assertion of suite.assertionResults || []) {
      const fullName = assertion.fullName || [...(assertion.ancestorTitles || []), assertion.title].join(" ")
      const identifier = options.target + "::" + encodeURIComponent(JSON.stringify([file, fullName]))
      if (identifiers.has(identifier)) {
        duplicates = true
        continue
      }
      identifiers.add(identifier)
      const status = normalizedStatus(assertion.status)
      if (options.units.length && !options.units.includes(identifier)) {
        if (status !== "skipped") throw new Error("runner executed an unrequested test unit")
        continue
      }
      cases.push({
        id: identifier,
        name: assertion.title || fullName,
        suite: (assertion.ancestorTitles || []).join(" > ") || file,
        file,
        status,
        attempts: [attemptFor(status, assertion)],
        runner_metadata: { full_name: fullName, duration_ms: assertion.duration ?? null },
      })
    }
  }
  if (duplicates && units.length) {
    throw new Error("selected full test names are ambiguous; run the whole target without test-unit filters")
  }
  if (options.inventory) {
    if (exitCode !== 0 || duplicates || !cases.length || native.success === false) return exitCode || 2
    fs.writeFileSync(options.results, JSON.stringify({ schema: 'struo.test_inventory.v1', target: options.target, units: cases.map(item => item.id).sort() }))
    return 0
  }
  const suiteUnit = options.unit === "suite" || duplicates
  const metadata = {}
  if (suiteUnit) {
    cases.length = 0
    const nativeSummary = { total: 0, passed: 0, failed: 0, skipped: 0 }
    for (const suite of native.testResults || []) {
      for (const assertion of suite.assertionResults || []) {
        nativeSummary.total += 1
        nativeSummary[normalizedStatus(assertion.status)] += 1
      }
    }
    const status = exitCode === 0 && native.success !== false && nativeSummary.failed === 0 && !(native.testResults || []).some(suite => suite.status === "failed") ? "passed" : "failed"
    const attempt = { status, duration_ms: Math.round(performance.now() - started) }
    if (status === "failed") attempt.failure = { message: "Native test suite failed; see the native results and runner log" }
    metadata.unit = "suite"
    if (duplicates) metadata.selection_reason = "duplicate full test names cannot be filtered exactly"
    metadata.native_summary = nativeSummary
    cases.push({
      id: options.target + "::suite",
      name: options.target,
      suite: options.target,
      status,
      attempts: [attempt],
      runner_metadata: { unit: "suite" },
    })
  }
  const passed = cases.filter(item => item.status === "passed").length
  const failed = cases.filter(item => item.status === "failed").length
  const skipped = cases.filter(item => item.status === "skipped").length
  const status = suiteUnit ? cases[0].status : exitCode === 0 ? "passed" : "failed"
  const normalized = {
    schema: "struo.test_results.v1",
    target: options.target,
    runner: { type: options.runner_type, metadata },
    selection_supported: !suiteUnit,
    status,
    summary: { total: cases.length, passed, failed, skipped, flaky: 0 },
    cases,
    artifacts: {
      logs: [path.posix.join(path.posix.dirname(options.results), options.runner_type + ".log")],
      native_results: [options.native_results],
    },
  }
  attachCoverage(normalized, options, coverage)
  fs.writeFileSync(options.results, JSON.stringify(normalized, null, 2))
  return status === "passed" ? 0 : exitCode || 1
}

try {
  process.exitCode = await main()
} catch (error) {
  console.error(error instanceof Error ? error.stack : String(error))
  process.exitCode = 2
}
'''

def _javascript_test_info(ctx, runner_type, display_name, node, runner, adapter, results, log, native_results, test_dir):
    suite_unit = _javascript_attr(ctx, "unit", "case") == "suite"
    return {
        "schema": "struo.test_info.v1",
        "target": ctx["label"]["id"],
        "runner": {"type": runner_type, "display_name": display_name, "metadata": {}},
        "command": {"argv": [node, adapter], "env": _javascript_env(ctx, test_dir, node), "cwd": "."},
        "outputs": {"results": results, "logs": [log], "native_results": [native_results], "coverage": []},
        "listing": {"supported": not suite_unit, "strategy": "unsupported" if suite_unit else "normalized_results"},
        "filtering": {"case_filtering": "unsupported" if suite_unit else "runner_args"},
        "sharding": {"supported": not suite_unit, "granularity": "target" if suite_unit else _javascript_attr(ctx, "batching", "file")},
        "retries": {"supported": False, "default_attempts": 1},
        "execution": {"cacheable": not _javascript_attr(ctx, "coverage_per_test", False), "timeout_ms": _javascript_attr(ctx, "timeout_ms", None), "run_from_workspace_root": True},
        "labels": _javascript_attr(ctx, "labels", []),
        "metadata": {},
    }

def _javascript_test_impl(ctx, runner_type, display_name, default_runner, cwd = ".", env = {}, inputs = None, discovery_inputs = None):
    node = _javascript_node(ctx)
    runner, runner_via_node, runner_is_workspace_input = _javascript_runner(ctx, runner_type, default_runner)
    if discovery_inputs == None:
        discovery_inputs = _javascript_discovery_inputs(ctx)
    if inputs == None:
        inputs = _javascript_test_inputs(ctx, runner, runner_is_workspace_input, discovery_inputs)
    test_dir = _test_output_dir(ctx)
    results = test_dir + "/test_results.json"
    log = test_dir + "/" + runner_type + ".log"
    native_results = test_dir + "/" + runner_type + "-results.json"
    adapter = test_dir + "/struo_javascript_test_adapter.mjs"
    info = _javascript_test_info(ctx, runner_type, display_name, node, runner, adapter, results, log, native_results, test_dir)
    action_env = _javascript_env(ctx, test_dir, node)
    action_env.update(env)
    info["command"]["env"] = action_env
    provider = {"label_id": ctx["label"]["id"], "target_kind": runner_type + "_test", "affected_inputs": inputs, "test_discovery_inputs": discovery_inputs, "test_info": info}
    portable_env = _javascript_env(ctx, test_dir, "")
    runner_version_argv = ([node, runner] if runner_via_node else [runner]) + ["--version"]
    provider["portable_test_configuration"] = _portable_test_configuration(ctx, ["node", "runner"], [
        host_command([node, "--version"]).strip(),
        host_command(runner_version_argv, cwd = workspace_root()).strip() if runner_is_workspace_input else host_command(runner_version_argv).strip(),
    ], portable_env)
    measured = _javascript_attr(ctx, "coverage_per_test", False)
    coverage_sources = _file_globs(_javascript_attr(ctx, "coverage_sources", []))
    if measured:
        if _javascript_attr(ctx, "unit", "case") != "case":
            fail("coverage_per_test requires native case units")
        if not coverage_sources:
            fail("coverage_per_test requires nonempty coverage_sources")
        collector_version = " ".join(provider["portable_test_configuration"]["tools"])
        provider["affected_inputs"] = _unique(inputs + coverage_sources)
        provider["coverage_selection"] = {
            "inputs": inputs, "tool": "v8-istanbul", "tool_version": collector_version,
            "platform": host_command([node, "-p", "process.platform + '/' + process.arch"]).strip(),
            "configuration": {"instrumentation": coverage_sources, "runtime": provider["portable_test_configuration"]},
        }
    discovery = ctx["capability"] == "test-discovery"
    inventory = test_dir + "/inventory.json"
    provider["test_inventory"] = inventory
    if ctx["capability"] != "test" and not discovery:
        return provider
    write_path(adapter, _javascript_test_adapter())
    base = [node, adapter, "--struo-runner-type", runner_type, "--struo-runner", runner, "--struo-runner-via-node", "true" if runner_via_node else "false", "--struo-unit", _javascript_attr(ctx, "unit", "case"), "--struo-target", ctx["label"]["id"]]
    if cwd != ".":
        base.extend(["--struo-cwd", cwd])
    for source in glob(ctx["srcs"]):
        base.extend(["--struo-source", source])
    argv = base + ["--struo-results", inventory if discovery or measured else results, "--struo-native-results", native_results]
    filters = (ctx.get("test") or {}).get("filters") or []
    for unit in ([] if discovery or measured else filters):
        argv.extend(["--struo-test-unit", unit])
    if discovery or measured:
        argv.append("--struo-inventory")
    argv.append("--")
    argv.extend(_javascript_attr(ctx, "args", []))
    version = host_command([node, "--version"]).strip()
    runner_version_argv = ([node, runner] if runner_via_node else [runner]) + ["--version"]
    runner_version = host_command(runner_version_argv, cwd = workspace_root()).strip() if runner_is_workspace_input else host_command(runner_version_argv).strip()
    run_action(
        display_name = "Run " + runner_type + " tests · " + ctx["label"]["name"],
        presentation = _action_metadata(context = [_action_context("javascript.runner", runner_type)]),
        source_files = _action_source_files(_unique(inputs + [adapter])),
        argv = argv,
        inputs = _unique(inputs + [adapter]),
        outputs = [inventory, log, native_results] if discovery or measured else [test_dir, results, log, native_results],
        clean_paths = [inventory, log, native_results] if discovery or measured else [results, log, native_results],
        create_dirs = [test_dir, test_dir + "/home"],
        env = action_env,
        stdout = log,
        stderr = log,
        cacheable = not discovery and not measured,
        toolchain_identity = "struo." + runner_type + ".v2\x00" + node + "\x00" + version + "\x00" + runner + "\x00" + runner_version,
        identifier = ctx["label"]["id"] + ":" + runner_type,
    )
    if measured and not discovery:
        hook = test_dir + "/struo_javascript_coverage.cjs"
        write_path(hook, _javascript_coverage_hook())
        isolated = base + ["--struo-coverage-hook", hook, "--struo-collector-version", collector_version]
        for source in coverage_sources:
            isolated.extend(["--struo-coverage-source", source])
        expand_actions(
            implementation = "isolated_test_coverage_plan", inputs = [inventory], outputs = [results],
            args = {"target": ctx["label"]["id"], "inventory": inventory, "directory": test_dir,
                    "argv": isolated, "inputs": _unique(inputs + coverage_sources + [adapter, hook]),
                    "env": action_env, "runner_args": ["--"] + _javascript_attr(ctx, "args", []),
                    "filters": filters, "output": results, "success_exit_codes": [0, 1, 2],
                    "identity": "struo.javascript.coverage.v1\x00" + content_sha256(_json_encode(provider["coverage_selection"]))},
        )
    return provider

def _vitest_impl(ctx):
    return _javascript_test_impl(ctx, "vitest", "Vitest", "node_modules/vitest/vitest.mjs")

def _jest_impl(ctx):
    return _javascript_test_impl(ctx, "jest", "Jest", "node_modules/jest/bin/jest.js")

def _javascript_test_attrs(runner, display_name):
    return [
        attr("node", "string", default = "\"node\"", docs = "Node.js executable name, absolute path, or workspace-relative path.", configurable = False),
        attr("runner", "string", default = "\"" + runner + "\"", docs = "Package-relative " + display_name + " entry point or executable. When omitted, Struo uses the package entry point when present, then searches PATH for the runner.", configurable = False),
        attr("config", "list<string>", default = "[\"package.json\", \"package-lock.json\", \"pnpm-lock.yaml\", \"yarn.lock\"]", docs = "Configuration and dependency inputs.", configurable = False),
        attr("dependencies", "list<string>", default = "[\"node_modules/**/*\"]", docs = "Installed runner and package files required during execution.", configurable = False),
        attr("data", "list<string>", default = "[]", docs = "Runtime data, setup, transform, snapshot, and runner package inputs.", configurable = False),
        attr("args", "list<string>", default = "[]", docs = "Additional runner arguments.", configurable = False),
        attr("coverage_per_test", "bool", default = "false", docs = "Measure each current exact case independently using native V8 coverage and Istanbul conversion for approximate selection.", configurable = False),
        attr("coverage_sources", "list<string>", default = "[]", docs = "Product JavaScript or TypeScript source globs to instrument, required for coverage_per_test.", configurable = False),
        attr("env", "map<string,string>", default = "{}", docs = "Environment variables for test execution.", configurable = True),
        attr("env_inherit", "list<string>", default = "[]", docs = "Host environment variables inherited by name.", configurable = False),
        attr("unit", "string", default = "\"case\"", docs = "Result unit: `case` uses unique file/full-name identities, falling back to a whole-target suite when an unfiltered run contains duplicates; `suite` always retains the complete native report as one target-level unit without case listing, filtering, or sharding.", configurable = False, allowed_values = ["case", "suite"]),
        attr("batching", "string", default = "\"file\"", docs = "Automatic batch granularity: `file`, `case`, or `target`.", configurable = False),
        attr("labels", "list<string>", default = "[]", docs = "Labels exposed through test discovery."),
        attr("timeout_ms", "int", docs = "Optional test timeout in milliseconds.", configurable = False),
    ]

vitest_test = target_kind(
    docs = "Runs Vitest suites through Struo with exact file and case filtering, normalized results, and automatic batching.",
    attrs = _javascript_test_attrs("node_modules/vitest/vitest.mjs", "Vitest"),
    deps = [dep("deps", [], "Targets whose inputs should affect this test suite.")],
    providers = ["struo_test_info"],
    capabilities = [capability("test", ["default", "test_results", "logs"]), capability("test-discovery", ["test_inventory"])],
    tools = [_JAVASCRIPT_TOOL, _VITEST_TOOL],
    examples = [example("vitest-test-minimal", name = "Minimal Vitest suite", use_when = "You want JavaScript or TypeScript tests scheduled by file through Struo.")],
    impl = _vitest_impl,
)

jest_test = target_kind(
    docs = "Runs Jest suites through Struo with exact file and case filtering, normalized results, and automatic batching.",
    attrs = _javascript_test_attrs("node_modules/jest/bin/jest.js", "Jest"),
    deps = [dep("deps", [], "Targets whose inputs should affect this test suite.")],
    providers = ["struo_test_info"],
    capabilities = [capability("test", ["default", "test_results", "logs"]), capability("test-discovery", ["test_inventory"])],
    tools = [_JAVASCRIPT_TOOL, _JEST_TOOL],
    examples = [example("jest-test-minimal", name = "Minimal Jest suite", use_when = "You want JavaScript or TypeScript Jest tests scheduled by file through Struo.")],
    impl = _jest_impl,
)
