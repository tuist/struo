def _javascript_coverage_hook():
    return '''const fs = require('node:fs')
const path = require('node:path')
const { fileURLToPath } = require('node:url')
const children = require('node:child_process')
const threads = require('node:worker_threads')
const marker = process.env.STRUO_COVERAGE_CHILD_MARKER
if (marker) {
  const mark = reason => fs.appendFileSync(marker, reason + '\\n')
  for (const name of ['spawn', 'spawnSync', 'exec', 'execSync', 'execFile', 'execFileSync', 'fork']) {
    const original = children[name]
    children[name] = function (...args) { mark(name); return Reflect.apply(original, this, args) }
  }
  const OriginalWorker = threads.Worker
  threads.Worker = class extends OriginalWorker {
    constructor(filename, options) {
      const entry = filename instanceof URL ? fileURLToPath(filename) : path.resolve(filename)
      if (!threads.isMainThread || entry !== process.env.STRUO_COVERAGE_NATIVE_WORKER) mark('worker')
      super(filename, options)
    }
  }
  require('node:module').syncBuiltinESMExports()
}
'''

def _javascript_coverage_adapter():
    return '''function coverageConfiguration(options, runnerArgs) {
  if (!options.coverage_sources.length) return { env: process.env }
  const directory = path.dirname(options.results) + '/coverage'
  fs.rmSync(directory, { recursive: true, force: true })
  fs.mkdirSync(directory, { recursive: true })
  const marker = directory + '/children'
  if (options.runner_type === 'vitest') {
    runnerArgs.push('--coverage.enabled', '--coverage.provider=v8', '--coverage.reporter=json', '--coverage.reportsDirectory=' + directory, '--pool=threads', '--maxWorkers=1')
    for (const source of options.coverage_sources) runnerArgs.push('--coverage.include=' + source)
  } else {
    runnerArgs.push('--coverage', '--coverageProvider=v8', '--coverageReporters=json', '--coverageDirectory=' + directory, '--watchman=false')
    for (const source of options.coverage_sources) runnerArgs.push('--collectCoverageFrom=' + source)
  }
  return { directory, marker, env: { ...process.env,
    NODE_OPTIONS: (process.env.NODE_OPTIONS || '') + ' --require ' + JSON.stringify(path.resolve(options.coverage_hook)),
    STRUO_COVERAGE_CHILD_MARKER: path.resolve(marker),
    STRUO_COVERAGE_NATIVE_WORKER: options.runner_type === 'vitest' ? path.join(path.dirname(fs.realpathSync(options.runner)), 'dist/workers/threads.js') : '',
  } }
}

function attachCoverage(normalized, options, configuration) {
  if (!configuration.directory) return
  let complete = normalized.status === 'passed' && normalized.cases.length === 1 && !fs.existsSync(configuration.marker)
  const files = []
  try {
    const native = JSON.parse(fs.readFileSync(configuration.directory + '/coverage-final.json', 'utf8'))
    const indexed = new Map(Object.entries(native).map(([name, file]) => [fs.realpathSync(name), file]))
    const root = fs.realpathSync(workspaceRoot)
    for (const source of options.coverage_sources) {
      const absolute = fs.realpathSync(source)
      if (!absolute.startsWith(root + path.sep)) throw new Error('coverage path escapes workspace')
      const file = indexed.get(absolute)
      if (!file) throw new Error('source absent from instrumented universe')
      const lines = new Map()
      for (const [id, statement] of Object.entries(file.statementMap)) {
        const count = file.s[id]
        const line = statement.start.line
        if (!Number.isSafeInteger(count) || count < 0 || !Number.isSafeInteger(line) || line < 1) throw new Error('invalid coverage counters')
        lines.set(line, Math.max(lines.get(line) || 0, count))
      }
      if (!lines.size) throw new Error('empty instrumented universe')
      const numbers = [...lines.keys()].sort((a, b) => a - b)
      files.push({ path: path.relative(root, absolute).split(path.sep).join('/'), line_numbers: numbers, execution_counts: numbers.map(line => lines.get(line)) })
    }
  } catch (error) {
    console.error('incomplete coverage: ' + String(error))
    complete = false
  }
  normalized.coverage = { tool: 'v8-istanbul', tool_version: options.collector_version, platform: process.platform + '/' + process.arch, partial: !complete, files }
  normalized.artifacts.logs = [path.posix.dirname(options.results) + '/runner.log']
}
'''

def _javascript_task_coverage(ctx, command, cwd, inputs, env, kind):
    python = _resolve_host_executable("python3")
    if not python:
        fail("JavaScript task coverage requires Python 3 for native argv resolution")
    analysis_env = {key:value.replace(execution_path(""), workspace_root()) for key,value in env.items()}
    parsed = json_decode(host_command([python, "-c", '''import json, os, pathlib, shlex, shutil, sys
argv = shlex.split(sys.argv[1])
if not argv or any(any(char in word for char in '$`;&|<>\\n') for word in argv):
    raise SystemExit('coverage requires a single native Jest or Vitest command without shell expansion')
executable = shutil.which(argv[0])
if not executable:
    raise SystemExit('native task executable was not found')
if pathlib.Path(argv[0]).name in ['node', 'nodejs']:
    if len(argv) < 2:
        raise SystemExit('missing native test runner')
    if os.path.realpath(executable) != os.path.realpath(sys.argv[2]):
        raise SystemExit('native command uses a different Node.js toolchain')
    runner, args = os.path.realpath(argv[1]), argv[2:]
else:
    runner, args = os.path.realpath(executable), argv[1:]
name = pathlib.Path(runner).name
if name in ['jest', 'jest.js']:
    framework = 'jest'
elif name in ['vitest', 'vitest.mjs']:
    framework = 'vitest'
    if args and args[0] == 'run':
        args = args[1:]
else:
    raise SystemExit('coverage requires a native Jest or Vitest task command')
print(json.dumps({'runner':runner,'args':args,'framework':framework}))
''', command, _javascript_node(ctx)], cwd = workspace_root() + "/" + cwd, env = analysis_env))
    sources = _file_globs(ctx["attr"].get("coverage_sources") or [])
    mapped = dict(ctx)
    attrs = dict(ctx["attr"])
    attrs.update({"runner":parsed["runner"],"args":parsed["args"],"unit":"case","batching":"target",
        "data":[],"config":[],"dependencies":[],"env":{key:value for key,value in env.items() if key != "PATH"},"env_inherit":[]})
    mapped.update({"attr":attrs,"srcs":[]})
    test_inputs = [path for path in inputs if path not in sources]
    provider = _javascript_test_impl(mapped, parsed["framework"], parsed["framework"], parsed["runner"], cwd = cwd, env = env, inputs = inputs, discovery_inputs = test_inputs)
    provider["coverage_selection"]["inputs"] = test_inputs
    provider["target_kind"] = kind
    provider["affected_inputs"] = inputs
    return provider
