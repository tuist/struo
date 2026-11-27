#shellcheck shell=bash
# End-to-end specs for the graph Struo derives from an Nx workspace without a
# manifest of its own. Nx's project graph API provides the graph that Struo
# translates into task targets so it can schedule and cache them.

Describe 'nx native project'
  BeforeEach 'setup_workspace'
  AfterEach 'cleanup_workspace'

  nx_toolchain_unavailable() {
    command -v node >/dev/null 2>&1 || return 0
    command -v npm >/dev/null 2>&1 || return 0
    return 1
  }

  copy_fixture() {
    cp -R "$REPO_ROOT/fixtures/$1/." "$WORKSPACE/"
  }

  install_fixture() {
    (cd "$WORKSPACE" && npm install --no-audit --no-fund --loglevel=error >/dev/null 2>&1)
  }

  target_ids() {
    struo --format json query targets | jq -r '.[] | "\(.kind) \(.id)"' | sort
  }

  rebuild_without_output() {
    struo --format json build probe || return
    rm "$WORKSPACE/packages/app/result.txt"
    struo --format json build probe
  }

  optional_outputs() {
    result=$(struo --format json build probe) || return
    printf '%s\n' "$result" | jq -er '.cache == "miss"' >/dev/null || return
    test ! -e "$WORKSPACE/packages/app/coverage" || return
    result=$(struo --format json build probe) || return
    printf '%s\n' "$result" | jq -er '.cache == "hit"' >/dev/null || return
    printf present > "$WORKSPACE/packages/app/mode"
    result=$(struo --format json build probe) || return
    printf '%s\n' "$result" | jq -er '.cache == "miss"' >/dev/null || return
    test -f "$WORKSPACE/packages/app/coverage/result.txt" || return
    rm -rf "$WORKSPACE/packages/app/coverage"
    result=$(struo --format json build probe) || return
    printf '%s\n' "$result" | jq -er '.cache == "hit"' >/dev/null || return
    printf fail > "$WORKSPACE/packages/app/mode"
    if struo --format json build probe > "$WORKSPACE/failure.stdout" 2> "$WORKSPACE/failure.stderr"; then
      return 1
    fi
    grep -q 'exit code 13' "$WORKSPACE/failure.stderr" || return
    printf 'miss hit miss hit; failure propagated\n'
  }

  It 'derives Nx tasks with no struo.toml present'
    nx_toolchain_unavailable && Skip 'node and npm are required'
    copy_fixture nx_hello
    install_fixture || Skip 'npm install failed; skipping (offline?)'

    When call target_ids
    The status should be success
    The stdout should include 'nx_workspace nx'
    The stdout should include 'nx_task hello_world__build'
    The stdout should include 'nx_test hello_world__test'
    The stdout should include 'nx_task greeter__build'
    The path "$WORKSPACE/struo.toml" should not be exist
  End

  It 'runs package-local executables and restores their declared outputs from cache'
    command -v node >/dev/null 2>&1 || Skip 'node is required'
    mkdir -p "$WORKSPACE/packages/app/node_modules/.bin" "$WORKSPACE/node_modules/.bin"
    printf '#!/bin/sh\nprintf workspace\n' > "$WORKSPACE/node_modules/.bin/nx-probe"
    printf '#!/bin/sh\nprintf package > result.txt\n' > "$WORKSPACE/packages/app/node_modules/.bin/nx-probe"
    chmod +x "$WORKSPACE/node_modules/.bin/nx-probe" "$WORKSPACE/packages/app/node_modules/.bin/nx-probe"
    cat > "$WORKSPACE/struo.toml" <<'TOML'
[[target]]
name = "probe"
kind = "nx_task"
[target.attrs]
project = "app"
project_root = "packages/app"
task = "build"
runnable = true
executor = "nx:run-commands"
commands = ["nx-probe"]
command_cwd = "packages/app"
outputs = ["packages/app/result.txt"]
TOML

    When call rebuild_without_output
    The status should be success
    The stderr should be blank
    The stdout should include '"cache":"miss"'
    The stdout should include '"cache":"hit"'
    The contents of file "$WORKSPACE/packages/app/result.txt" should equal 'package'
  End

  It 'caches absent Nx outputs and restores existing output trees without hiding failures'
    command -v node >/dev/null 2>&1 || Skip 'node is required'
    mkdir -p "$WORKSPACE/packages/app"
    printf absent > "$WORKSPACE/packages/app/mode"
    cat > "$WORKSPACE/packages/app/probe.js" <<'JS'
const fs = require('node:fs');
const mode = fs.readFileSync('mode', 'utf8').trim();
if (mode === 'fail') process.exit(13);
fs.mkdirSync('node_modules/.vite/vitest', { recursive: true });
fs.writeFileSync('node_modules/.vite/vitest/results.json', String(Date.now()));
if (mode === 'present') {
  fs.mkdirSync('coverage', { recursive: true });
  fs.writeFileSync('coverage/result.txt', 'actual coverage');
}
JS
    cat > "$WORKSPACE/struo.toml" <<'TOML'
[[target]]
name = "probe"
kind = "nx_task"
[target.attrs]
project = "app"
project_root = "packages/app"
task = "test"
runnable = true
executor = "nx:run-commands"
commands = ["node probe.js"]
command_cwd = "packages/app"
outputs = ["packages/app/coverage"]
TOML

    When call optional_outputs
    The status should be success
    The stderr should be blank
    The stdout should equal 'miss hit miss hit; failure propagated'
    The contents of file "$WORKSPACE/packages/app/coverage/result.txt" should equal 'actual coverage'
  End

  path_changes() {
    struo --format json build probe | jq -r .cache || return
    mkdir -p "$WORKSPACE/../extra-bin" "$WORKSPACE/../other-home"
    HOME="$WORKSPACE/../other-home" PATH="$WORKSPACE/../extra-bin:$PATH" struo --format json build probe | jq -r .cache
  }

  It "keeps the caller's PATH and HOME out of task cache keys and still finds the package manager"
    command -v node >/dev/null 2>&1 || Skip 'node is required'
    command -v npm >/dev/null 2>&1 || Skip 'npm is required'
    printf '%s\n' '{"name":"probe","private":true,"scripts":{"build":"node build.js"}}' > "$WORKSPACE/package.json"
    printf '%s\n' "require('node:fs').writeFileSync('result.txt', 'built')" > "$WORKSPACE/build.js"
    cat > "$WORKSPACE/struo.toml" <<'TOML'
[[target]]
name = "probe"
kind = "nx_task"
[target.attrs]
project = "probe"
task = "build"
runnable = true
executor = "nx:run-script"
commands = ["npm run build"]
outputs = ["result.txt"]
TOML

    When call path_changes
    The status should be success
    The stdout should equal 'miss
hit'
    The contents of file "$WORKSPACE/result.txt" should equal 'built'
  End

  generated_dependency_changes() {
    SEED=first struo --format json build consumer >/dev/null || return
    test "$(cat "$WORKSPACE/result.txt")" = first || return
    SEED=second struo --format json build consumer >/dev/null || return
    test "$(cat "$WORKSPACE/result.txt")" = second || return
    rm "$WORKSPACE/generated.txt" "$WORKSPACE/result.txt"
    SEED=second struo --format json build consumer >/dev/null || return
    test "$(cat "$WORKSPACE/generated.txt")" = second || return
    cat "$WORKSPACE/result.txt"
  }

  It 'invalidates consumers when generated dependency contents change without source changes'
    command -v node >/dev/null 2>&1 || Skip 'node is required'
    cat > "$WORKSPACE/struo.toml" <<'TOML'
[[target]]
name = "generate"
kind = "nx_task"
[target.attrs]
project = "app"
task = "generate"
runnable = true
executor = "nx:run-commands"
commands = ["printf %s \"$SEED\" > generated.txt"]
env_inherit = ["SEED"]
outputs = ["generated.txt"]
workspace_outputs = ["generated.txt", "result.txt"]

[[target]]
name = "consumer"
kind = "nx_task"
deps = ["generate"]
[target.attrs]
project = "app"
task = "build"
runnable = true
executor = "nx:run-commands"
commands = ["cp generated.txt result.txt"]
outputs = ["result.txt"]
workspace_outputs = ["generated.txt", "result.txt"]
TOML

    When call generated_dependency_changes
    The status should be success
    The stderr should be blank
    The stdout should equal 'second'
  End

  It 'does not skip failing test prerequisites of builds'
    command -v node >/dev/null 2>&1 || Skip 'node is required'
    cat > "$WORKSPACE/struo.toml" <<'TOML'
[[target]]
name = "check"
kind = "nx_test"
[target.attrs]
project = "app"
task = "test"
runnable = true
executor = "nx:run-commands"
commands = ["exit 7"]

[[target]]
name = "consumer"
kind = "nx_task"
deps = ["check"]
[target.attrs]
project = "app"
task = "build"
runnable = true
executor = "nx:run-commands"
commands = ["printf built > result.txt"]
outputs = ["result.txt"]
TOML

    When call struo build consumer
    The status should be failure
    The stderr should include 'exit code 7'
    The path "$WORKSPACE/result.txt" should not be exist
  End

  It 'runs plugin executors with native JSONC workspace configuration'
    nx_toolchain_unavailable && Skip 'node and npm are required'
    printf '%s\n' '{"private":true,"devDependencies":{"nx":"^23.0.0","@nx/devkit":"^23.0.0"}}' > "$WORKSPACE/package.json"
    install_fixture || Skip 'npm install failed; skipping (offline?)'
    printf '%s\n' '{/* native Nx accepts comments */ "parallel":1,}' > "$WORKSPACE/nx.json"
    cat > "$WORKSPACE/struo.toml" <<'TOML'
[[target]]
name = "probe"
kind = "nx_task"
[target.attrs]
project = "app"
task = "build"
runnable = true
executor = "nx:noop"
executor_graph = '{"nodes":{"app":{"name":"app","type":"app","data":{"root":".","targets":{"build":{"executor":"nx:noop"}}}}},"dependencies":{}}'
TOML

    When call struo --format json build probe
    The status should be success
    The stderr should be blank
    The stdout should include '"status":"completed"'
  End

  It 'loads the graph from the binary Nx declares when node_modules/.bin holds shell shims'
    command -v node >/dev/null 2>&1 || Skip 'node is required'
    nx="$WORKSPACE/node_modules/nx"
    mkdir -p "$nx/dist/bin" "$nx/dist/src/project-graph" "$nx/dist/src/utils" "$WORKSPACE/node_modules/.bin"
    printf '%s\n' 'exports.loadRootEnvFiles = () => {};' > "$nx/dist/src/utils/dotenv.js"
    printf '%s\n' '{}' > "$WORKSPACE/nx.json"
    printf '%s\n' '{"name":"nx","version":"23.0.0","bin":{"nx":"./dist/bin/nx.js"}}' > "$nx/package.json"
    printf '%s\n' '' > "$nx/dist/bin/nx.js"
    cat > "$nx/dist/src/project-graph/project-graph.js" <<'JS'
exports.createProjectGraphAsync = async () => ({
  nodes: { app: { name: 'app', type: 'app', data: { root: 'app', targets: { build: { executor: 'nx:run-commands', options: { command: 'true' } } } } } },
  dependencies: {},
});
JS
    # pnpm links binaries with a shell script instead of a symlink.
    printf '#!/bin/sh\nexec node "$(dirname "$0")/../nx/dist/bin/nx.js" "$@"\n' > "$WORKSPACE/node_modules/.bin/nx"
    chmod +x "$WORKSPACE/node_modules/.bin/nx"

    When call target_ids
    The status should be success
    The stdout should include 'nx_task app__build'
  End

  It 'loads many plugin tasks over a large external package graph within a bounded memory budget'
    Skip if 'ulimit -v is enforced on Linux' [ "$(uname -s)" != Linux ]
    Skip if 'node is required' ! command -v node >/dev/null 2>&1
    node -e '
      const projects = {};
      for (let p = 0; p < 10; p++) {
        const targets = {};
        for (const t of ["build", "test", "lint", "build-package"]) targets[t] = { executor: "@probe/plugin:" + t };
        projects["lib" + p] = { type: "lib", data: { root: "libs/lib" + p, targets } };
      }
      const externalNodes = {};
      for (let i = 0; i < 4000; i++) {
        externalNodes["npm:pkg-" + i] = { type: "npm", name: "npm:pkg-" + i, data: { version: "1.0." + i, packageName: "pkg-" + i, hash: "sha512-" + "x".repeat(160) } };
      }
      require("node:fs").writeFileSync(process.argv[1], JSON.stringify({ graph: { nodes: projects, externalNodes, dependencies: {} } }));
    ' "$WORKSPACE/graph.json"
    cat > "$WORKSPACE/struo.toml" <<'TOML'
[[target]]
name = "nx"
kind = "nx_workspace"
[target.attrs]
graph_file = "graph.json"
TOML

    # Each task encoding the graph again needed about 5 GiB of address space.
    # Two malloc arenas keep the reservation independent of the CPU count.
    bounded_query() { (ulimit -v 2097152 && MALLOC_ARENA_MAX=2 struo --format json query targets) | jq -r '.[].id' | grep -c '__'; }
    When call bounded_query
    The status should be success
    The stdout should equal 30
  End

  affected_by() {
    struo --format json query affected-tests --changed-path "$1" | jq -r '.[].id' | sort | tr '\n' ' '
  }

  It 'selects the tests of projects that depend on a changed project, like nx affected'
    mkdir -p "$WORKSPACE/apps/app" "$WORKSPACE/libs/core" "$WORKSPACE/libs/ui" "$WORKSPACE/libs/other"
    for root in apps/app libs/core libs/ui libs/other; do printf x > "$WORKSPACE/$root/index.ts"; done
    cat > "$WORKSPACE/graph.json" <<'JSON'
{"graph": {"nodes": {
  "app": {"type": "app", "data": {"root": "apps/app", "targets": {"test": {"executor": "nx:run-commands", "options": {"command": "true"}}}}},
  "ui": {"type": "lib", "data": {"root": "libs/ui", "targets": {"test": {"executor": "nx:run-commands", "options": {"command": "true"}}}}},
  "core": {"type": "lib", "data": {"root": "libs/core", "targets": {"test": {"executor": "nx:run-commands", "options": {"command": "true"}}}}},
  "other": {"type": "lib", "data": {"root": "libs/other", "targets": {"test": {"executor": "nx:run-commands", "options": {"command": "true"}}}}},
  "tools": {"type": "lib", "data": {"root": ".", "targets": {"test": {"executor": "nx:run-commands", "options": {"command": "true"}}}}}
}, "dependencies": {
  "app": [{"source": "app", "target": "ui", "type": "static"}, {"source": "app", "target": "npm:react", "type": "static"}],
  "ui": [{"source": "ui", "target": "core", "type": "implicit"}, {"source": "ui", "target": "tools", "type": "static"}],
  "core": [], "other": [], "tools": [{"source": "tools", "target": "other", "type": "static"}]
}}}
JSON
    cat > "$WORKSPACE/struo.toml" <<'TOML'
[[target]]
name = "nx"
kind = "nx_workspace"
[target.attrs]
graph_file = "graph.json"
TOML

    selections() {
      affected_by libs/core/index.ts && echo
      affected_by libs/other/index.ts && echo
      affected_by README.md
    }
    When call selections
    The status should be success
    The line 1 of stdout should equal 'app__test core__test tools__test ui__test '
    The line 2 of stdout should equal 'app__test other__test tools__test ui__test '
    The line 3 of stdout should equal 'tools__test '
  End

  build_cache() { struo --format json build "$1" | jq -r .cache; }

  It 'keeps outputs declared with a trailing slash out of other tasks inputs'
    command -v node >/dev/null 2>&1 || Skip 'node is required'
    mkdir -p "$WORKSPACE/app"
    printf x > "$WORKSPACE/app/index.ts"
    cat > "$WORKSPACE/graph.json" <<'JSON'
{"graph": {"nodes": {"app": {"type": "app", "data": {"root": "app", "targets": {
  "build": {"executor": "nx:run-commands", "outputs": ["{options.outputPath}"],
    "options": {"outputPath": "staging/../dist-app/", "command": "mkdir -p dist-app && date +%s%N > dist-app/out.txt"}},
  "lint": {"executor": "nx:run-commands", "options": {"command": "true"}}
}}}}, "dependencies": {}}}
JSON
    cat > "$WORKSPACE/struo.toml" <<'TOML'
[[target]]
name = "nx"
kind = "nx_workspace"
[target.attrs]
graph_file = "graph.json"
TOML

    lint_around_build() {
      build_cache app__lint && build_cache app__build >/dev/null && build_cache app__lint
    }
    When call lint_around_build
    The status should be success
    The stdout should equal 'miss
hit'
  End

  It 'keeps gitignored and nxignored files out of task inputs, as Nx does'
    command -v node >/dev/null 2>&1 || Skip 'node is required'
    mkdir -p "$WORKSPACE/app/fixtures" "$WORKSPACE/app/notes"
    printf x > "$WORKSPACE/app/index.ts"
    printf '%s\n' 'fixtures/out/*' '!fixtures/out/keep.json' > "$WORKSPACE/app/.gitignore"
    printf '%s\n' 'notes/' > "$WORKSPACE/app/.nxignore"
    cat > "$WORKSPACE/graph.json" <<'JSON'
{"graph": {"nodes": {"app": {"type": "app", "data": {"root": "app", "targets": {
  "lint": {"executor": "nx:run-commands", "options": {"command": "true"}}
}}}}, "dependencies": {}}}
JSON
    cat > "$WORKSPACE/struo.toml" <<'TOML'
[[target]]
name = "nx"
kind = "nx_workspace"
[target.attrs]
graph_file = "graph.json"
TOML

    ignored_writes() {
      build_cache app__lint || return
      # A test byproduct and a note the ignore files exclude.
      mkdir -p "$WORKSPACE/app/fixtures/out"
      printf generated > "$WORKSPACE/app/fixtures/out/en.json"
      printf note > "$WORKSPACE/app/notes/todo.md"
      build_cache app__lint || return
      # A file the ignore rules re-include, then a source file.
      printf kept > "$WORKSPACE/app/fixtures/out/keep.json"
      build_cache app__lint || return
      printf y > "$WORKSPACE/app/index.ts"
      build_cache app__lint || return
      # Editing an ignore file changes what the task reads.
      printf '%s\n' 'fixtures/other' > "$WORKSPACE/app/.gitignore"
      build_cache app__lint
    }
    When call ignored_writes
    The status should be success
    The stdout should equal 'miss
hit
miss
miss
miss'
  End

  It 'accepts plugin executors that return their result instead of yielding it, like ng-packagr'
    command -v node >/dev/null 2>&1 || Skip 'node is required'
    mkdir -p "$WORKSPACE/node_modules/@nx/devkit"
    printf '%s\n' '{"name":"@nx/devkit","main":"index.js"}' > "$WORKSPACE/node_modules/@nx/devkit/package.json"
    cat > "$WORKSPACE/node_modules/@nx/devkit/index.js" <<'JS'
exports.readJsonFile = () => ({});
// @nx/angular:package returns its result from an async generator.
exports.runExecutor = async function* () {
  require('node:fs').writeFileSync('result.txt', 'returned');
  return { success: true };
};
JS
    cat > "$WORKSPACE/struo.toml" <<'TOML'
[[target]]
name = "probe"
kind = "nx_task"
[target.attrs]
project = "app"
task = "build"
runnable = true
executor = "probe:return"
outputs = ["result.txt"]
executor_graph = '{"nodes":{"app":{"name":"app","type":"app","data":{"root":".","targets":{"build":{"executor":"probe:return"}}}}},"dependencies":{}}'
TOML

    When call struo --format json build probe
    The status should be success
    The stderr should be blank
    The stdout should include '"status":"completed"'
    The contents of file "$WORKSPACE/result.txt" should equal 'returned'
  End

  It 'lets plugin executors fork Node.js workers in an ES module workspace'
    command -v node >/dev/null 2>&1 || Skip 'node is required'
    mkdir -p "$WORKSPACE/node_modules/@nx/devkit"
    printf '%s\n' '{"type":"module"}' > "$WORKSPACE/package.json"
    printf '%s\n' '{"name":"@nx/devkit","main":"index.js"}' > "$WORKSPACE/node_modules/@nx/devkit/package.json"
    cat > "$WORKSPACE/node_modules/@nx/devkit/index.js" <<'JS'
const { fork } = require('node:child_process');
exports.readJsonFile = () => ({});
exports.runExecutor = async function* () {
  // Like jest-worker, pass a copy of the parent's Node.js flags to the worker.
  const execArgv = process.execArgv.filter((flag) => !flag.startsWith('--inspect'));
  const code = await new Promise((resolve) => fork(require.resolve('./worker.js'), [], { execArgv }).on('exit', resolve));
  yield { success: code === 0 };
};
JS
    printf '%s\n' "require('node:fs').writeFileSync('result.txt', 'forked')" > "$WORKSPACE/node_modules/@nx/devkit/worker.js"
    cat > "$WORKSPACE/struo.toml" <<'TOML'
[[target]]
name = "probe"
kind = "nx_task"
[target.attrs]
project = "app"
task = "build"
runnable = true
executor = "probe:fork"
outputs = ["result.txt"]
executor_graph = '{"nodes":{"app":{"name":"app","type":"app","data":{"root":".","targets":{"build":{"executor":"probe:fork"}}}}},"dependencies":{}}'
TOML

    When call struo --format json build probe
    The status should be success
    The stderr should be blank
    The stdout should include '"status":"completed"'
    The contents of file "$WORKSPACE/result.txt" should equal 'forked'
  End

  compiler_dependency_outputs() {
    result=$(struo --format json build app__build) || return
    printf '%s\n' "$result" | jq -er '.cache == "miss"' >/dev/null || return
    rm -rf "$WORKSPACE/dist"
    result=$(struo --format json build app__build) || return
    printf '%s\n' "$result" | jq -er '.cache == "hit"' >/dev/null || return
    (cd "$WORKSPACE" && node -e 'console.log(require("./dist/app/index.js").answer, require("./dist/app/package.json").dependencies.tslib)')
  }

  It 'retains external package dependencies for Nx 17 compiler actions and cache restoration'
    nx_toolchain_unavailable && Skip 'node and npm are required'
    cat > "$WORKSPACE/package.json" <<'JSON'
{"name":"nx17-probe","private":true,"dependencies":{"tslib":"2.4.1"},"devDependencies":{"nx":"17.1.3","@nx/js":"17.1.3","typescript":"5.2.2"}}
JSON
    install_fixture || Skip 'npm install failed; skipping (offline?)'
    printf '%s\n' '{"plugins":["./dotenv-plugin.cjs"]}' > "$WORKSPACE/nx.json"
    printf '%s\n' 'STRUO_NX_GRAPH_BASE=loaded' 'STRUO_NX_GRAPH_VALUE=${STRUO_NX_GRAPH_BASE}-local' > "$WORKSPACE/.local.env"
    printf '%s\n' 'STRUO_NX_GRAPH_VALUE=wrong' > "$WORKSPACE/.env.local"
    cat > "$WORKSPACE/dotenv-plugin.cjs" <<'JS'
module.exports = {
  name: 'dotenv-probe',
  createNodes: ['**/project.json', () => {
    if (process.env.STRUO_NX_GRAPH_VALUE !== 'loaded-local') {
      throw new Error('root dotenv precedence and expansion were not preserved');
    }
    return { projects: {} };
  }],
};
JS
    printf '%s\n' '{"compilerOptions":{"importHelpers":true,"declaration":true,"module":"commonjs","target":"es2018","types":[]}}' > "$WORKSPACE/tsconfig.base.json"
    mkdir -p "$WORKSPACE/packages/app/src"
    printf '%s\n' 'export const answer: number = 42;' > "$WORKSPACE/packages/app/src/index.ts"
    printf '%s\n' '{"extends":"../../tsconfig.base.json","include":["src/**/*.ts"]}' > "$WORKSPACE/packages/app/tsconfig.lib.json"
    cat > "$WORKSPACE/packages/app/project.json" <<'JSON'
{"name":"app","sourceRoot":"packages/app/src","projectType":"library","targets":{"build":{"executor":"@nx/js:tsc","outputs":["{options.outputPath}"],"options":{"outputPath":"dist/app","main":"packages/app/src/index.ts","rootDir":"packages/app/src","tsConfig":"packages/app/tsconfig.lib.json","assets":[],"updateBuildableProjectDepsInPackageJson":true}}}}
JSON

    When call compiler_dependency_outputs
    The status should be success
    The stderr should be blank
    The stdout should equal '42 2.4.1'
  End
End
