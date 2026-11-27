use std::path::Path;
use std::process::{Command, Output};

use serde_json::json;

fn write_files(root: &Path, files: &[(&str, &str)]) {
    for (path, contents) in files {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }
}

fn task(id: &str, directory: &str, command: &str, outputs: &[&str]) -> serde_json::Value {
    let (package, name) = id.rsplit_once('#').unwrap();
    json!({
        "taskId": id, "package": package, "task": name, "directory": directory,
        "command": command, "dependencies": [],
        "resolvedTaskDefinition": {"outputs": outputs, "cache": true, "persistent": false},
        "environmentVariables": {"specified": {"env": [], "passThroughEnv": null}, "configured": [], "inferred": [], "passthrough": null},
    })
}

/// A pnpm-style workspace with one package whose test, like Vitest, writes its
/// own cache under the package's `node_modules` and a declared output.
fn workspace() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    write_workspace(root.path());
    root
}

fn write_workspace(root: &Path) {
    let graph = json!({
        "packages": [
            {"name": "app", "path": "app", "allDependencies": {"items": [{"name": "lib"}]}},
            {"name": "lib", "path": "lib", "allDependencies": {"items": []}},
        ],
        "dry": {"tasks": [
            task("app#test", "app", "node test.js", &["coverage.json"]),
            task("lib#test", "lib", "node -e 0", &[]),
        ]},
    });
    write_files(
        root,
        &[
            (
                "struo.toml",
                "[[target]]\nname = 'turbo'\nkind = 'turbo_workspace'\n[target.attrs]\ngraph_file = 'graph.json'\n",
            ),
            ("graph.json", &graph.to_string()),
            ("package.json", "{\"name\": \"root\", \"private\": true}\n"),
            ("tsconfig.json", "{}\n"),
            ("README.md", "readme\n"),
            ("app/package.json", "{\"name\": \"app\"}\n"),
            ("app/src/value.js", "module.exports = 42;\n"),
            ("lib/package.json", "{\"name\": \"lib\"}\n"),
            ("lib/src/value.js", "module.exports = 1;\n"),
            (
                "app/test.js",
                "const fs = require('node:fs');\n\
                 if (require('dep') + require('./src/value.js') !== 84) process.exit(1);\n\
                 for (const dir of ['node_modules/.vite/vitest/abc', 'node_modules/.vite-temp', '../node_modules/.cache/babel', '../node_modules/.vitest-cache']) fs.mkdirSync(dir, { recursive: true });\n\
                 fs.writeFileSync('node_modules/.vite/vitest/abc/results.json', String(process.hrtime.bigint()));\n\
                 fs.writeFileSync('../node_modules/.vitest-cache/_metadata.json', String(process.hrtime.bigint()));\n\
                 fs.writeFileSync('../node_modules/.cache/babel/entry.json', String(process.hrtime.bigint()));\n\
                 fs.writeFileSync('coverage.json', String(process.hrtime.bigint()));\n",
            ),
            ("app/node_modules/.keep", ""),
            ("node_modules/dep/package.json", "{\"name\": \"dep\"}\n"),
            ("node_modules/dep/index.js", "module.exports = 42;\n"),
            ("node_modules/.modules.yaml", "prunedAt: Mon, 01 Jan 2026 00:00:00 GMT\n"),
            (
                "node_modules/.pnpm-workspace-state.json",
                "{\"lastValidatedTimestamp\": 1}\n",
            ),
        ],
    );
}

fn struo(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_struo"))
        .current_dir(root)
        .args(["--format", "json", "--cpu-limit", "1"])
        .args(args)
        .env("STRUO_CACHE_PROVIDER", "local")
        // Action keys do not depend on the checkout path, so a shared cache
        // would answer from other runs of the same fixture.
        .env("XDG_CACHE_HOME", root.join(".struo/xdg-cache"))
        .env("STRUO_CHANGE_TRACKER", "0")
        .output()
        .unwrap()
}

fn success(output: &Output) {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn test_cache_state(root: &Path) -> String {
    let output = struo(root, &["test", "app__test"]);
    success(&output);
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find(|record| record["target"] == "app__test")
        .and_then(|record| record["cache"].as_str().map(str::to_string))
        .unwrap_or_else(|| panic!("no record: {}", String::from_utf8_lossy(&output.stdout)))
}

// Turbo hashes only a package's tracked files and the lockfile, so caches
// tools keep in `node_modules`, such as Vite's, Vitest 5's and Babel's, and the package manager's install bookkeeping
// never rerun a task. They are not inputs here either, while installed
// package files still are.
#[test]
fn tool_caches_and_install_state_in_node_modules_do_not_rerun_tests() {
    let root = workspace();
    assert_eq!(test_cache_state(root.path()), "miss");
    assert_eq!(test_cache_state(root.path()), "hit");

    // What a no-op `pnpm install` rewrites.
    write_files(
        root.path(),
        &[
            (
                "node_modules/.modules.yaml",
                "prunedAt: Tue, 02 Jan 2026 00:00:00 GMT\n",
            ),
            (
                "node_modules/.pnpm-workspace-state.json",
                "{\"lastValidatedTimestamp\": 2}\n",
            ),
        ],
    );
    assert_eq!(test_cache_state(root.path()), "hit");

    // Control: an installed package's code is still an input.
    write_files(
        root.path(),
        &[("node_modules/dep/index.js", "module.exports = 42; // v2\n")],
    );
    assert_eq!(test_cache_state(root.path()), "miss");
}

// Turbo hashes a package's tracked files and its untracked files git does not
// ignore, so what a task leaves in the package outside its declared outputs,
// such as `tsc --build`'s `tsconfig.tsbuildinfo`, does not rerun it when git
// ignores it.
#[test]
fn files_git_ignores_are_not_task_inputs() {
    let root = workspace();
    let mut graph: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.path().join("graph.json")).unwrap()).unwrap();
    graph["dry"]["tasks"][0]["resolvedTaskDefinition"]["inputs"] =
        json!(["$TURBO_DEFAULT$", ".env*"]);
    write_files(
        root.path(),
        &[
            ("graph.json", &graph.to_string()),
            (
                ".gitignore",
                "node_modules\n.struo\n**/tsconfig.tsbuildinfo\ncache*/\n.env*\ngenerated.js\n",
            ),
            ("app/.env.local", "A=1\n"),
            ("app/src/generated.js", "module.exports = 1;\n"),
            (
                "app/test.js",
                "const fs = require('node:fs');\n\
                 if (require('dep') + require('./src/value.js') !== 84) process.exit(1);\n\
                 fs.writeFileSync('tsconfig.tsbuildinfo', String(process.hrtime.bigint()));\n\
                 fs.mkdirSync('cache[*]', { recursive: true });\n\
                 fs.writeFileSync('cache[*]/entry', String(process.hrtime.bigint()));\n",
            ),
        ],
    );
    for args in [
        &["init", "-q"][..],
        &["add", "-A"],
        &["add", "-f", "app/src/generated.js"],
    ] {
        let status = Command::new("git")
            .current_dir(root.path())
            .args(args)
            .status()
            .unwrap();
        assert!(status.success());
    }
    assert_eq!(test_cache_state(root.path()), "miss");
    assert_eq!(test_cache_state(root.path()), "hit");

    // Controls, as turbo hashes them: an untracked file git does not ignore,
    // an ignored file the task's `inputs` name, and a tracked file an ignore
    // rule matches are all still inputs.
    for (path, contents) in [
        ("app/src/extra.js", "module.exports = 0;\n"),
        ("app/.env.local", "A=2\n"),
        ("app/src/generated.js", "module.exports = 2;\n"),
    ] {
        write_files(root.path(), &[(path, contents)]);
        assert_eq!(test_cache_state(root.path()), "miss", "{path}");
    }
}

// A Turborepo root inside a larger git repository, as turbo supports, still
// hashes a tracked file that an ignore rule inside the workspace matches.
#[test]
fn nested_workspaces_keep_tracked_but_ignored_files_as_inputs() {
    let repository = tempfile::tempdir().unwrap();
    let root = repository.path().join("web");
    write_workspace(&root);
    write_files(
        &root,
        &[
            (".gitignore", "node_modules\n.struo\ngenerated.js\n"),
            ("app/src/generated.js", "module.exports = 1;\n"),
        ],
    );
    for args in [
        &["init", "-q"][..],
        &["add", "-A"],
        &["add", "-f", "web/app/src/generated.js"],
    ] {
        let status = Command::new("git")
            .current_dir(repository.path())
            .args(args)
            .status()
            .unwrap();
        assert!(status.success());
    }
    assert_eq!(test_cache_state(&root), "miss");
    assert_eq!(test_cache_state(&root), "hit");
    write_files(&root, &[("app/src/generated.js", "module.exports = 2;\n")]);
    assert_eq!(test_cache_state(&root), "miss");
}

// A checkout whose `.git` git refuses to read, as with dubious ownership in a
// container, still loads, and an ignored file stays an input because the
// tracked set is unknown.
#[test]
fn checkouts_git_cannot_read_keep_ignored_files_as_inputs() {
    let root = workspace();
    write_files(
        root.path(),
        &[
            (".git", "gitdir: /nonexistent/struo-test\n"),
            (".gitignore", "node_modules\n.struo\ngenerated.js\n"),
            ("app/src/generated.js", "module.exports = 1;\n"),
        ],
    );
    assert_eq!(test_cache_state(root.path()), "miss");
    assert_eq!(test_cache_state(root.path()), "hit");
    write_files(
        root.path(),
        &[("app/src/generated.js", "module.exports = 2;\n")],
    );
    assert_eq!(test_cache_state(root.path()), "miss");
}

// Turbo reports a task's `inputs` relative to its package, with `$TURBO_ROOT$`
// already resolved, as in `../.env`, and a literal directory such as `gen`
// stands for every file in it. Turbo hashes both even when git ignores them.
#[test]
fn package_relative_and_directory_inputs_stay_inputs() {
    let root = workspace();
    let mut graph: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.path().join("graph.json")).unwrap()).unwrap();
    graph["dry"]["tasks"][0]["resolvedTaskDefinition"]["inputs"] =
        json!(["!README.md", "../.env", "./gen", "../../outside"]);
    write_files(
        root.path(),
        &[
            ("graph.json", &graph.to_string()),
            (".gitignore", "node_modules\n.struo\n.env\ngen/\n"),
            (".env", "A=1\n"),
            ("app/gen/a.js", "module.exports = 1;\n"),
        ],
    );
    assert_eq!(test_cache_state(root.path()), "miss");
    assert_eq!(test_cache_state(root.path()), "hit");
    for (path, contents) in [(".env", "A=2\n"), ("app/gen/a.js", "module.exports = 2;\n")] {
        write_files(root.path(), &[(path, contents)]);
        assert_eq!(test_cache_state(root.path()), "miss", "{path}");
    }
}

// A test's discovery binding holds only while what it reads is unchanged, so
// what the run itself writes to its outputs and tool caches must not count.
#[test]
fn test_runs_keep_their_discovery_binding() {
    let root = workspace();
    test_cache_state(root.path());
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(
            root.path()
                .join(".struo/test-manifests/app__test/manifest.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(
        manifest["discovery_fingerprint"].is_string(),
        "no discovery fingerprint: {manifest}"
    );
}

// pnpm's `.bin` shims point `NODE_PATH` at pnpm's hoisted dependencies,
// which code outside every package's `node_modules` chain, such as a bundle a
// test writes to a temporary directory, resolves through. The variable names the
// checkout, yet two checkouts of one commit still share cache keys.
#[test]
fn pnpm_scripts_resolve_hoisted_dependencies_with_checkout_independent_keys() {
    let checkouts = [workspace(), workspace()];
    let digests = checkouts.each_ref().map(|root| {
        write_files(
            root.path(),
            &[
                (
                    "node_modules/.pnpm/node_modules/hoisted/index.js",
                    "module.exports = 84;\n",
                ),
                (
                    "app/test.js",
                    "if (require('hoisted') !== 84) process.exit(1);\n",
                ),
            ],
        );
        let output = struo(root.path(), &["test", "app__test"]);
        success(&output);
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .find(|record| record["target"] == "app__test")
            .and_then(|record| record["action_digest"].as_str().map(str::to_string))
            .unwrap()
    });
    assert_ne!(checkouts[0].path(), checkouts[1].path());
    assert_eq!(digests[0], digests[1]);
}

// Two checkouts of one commit have the same graph fingerprint whatever their
// installs and earlier runs left in package directories.
#[test]
fn graph_fingerprint_leaves_out_what_installs_and_tasks_write() {
    let root = workspace();
    let fingerprint = || {
        let output = struo(root.path(), &["query", "graph-fingerprint"]);
        success(&output);
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        value["digest"].as_str().unwrap().to_string()
    };
    let before = fingerprint();
    write_files(
        root.path(),
        &[
            ("app/node_modules/.vite/vitest/abc/results.json", "{}"),
            ("app/node_modules/.bin/tool", "#!/bin/sh\n# /checkout/a\n"),
            ("app/coverage.json", "{}"),
        ],
    );
    assert_eq!(fingerprint(), before);
    // Control: a package source still changes it.
    write_files(
        root.path(),
        &[("app/src/value.js", "module.exports = 43;\n")],
    );
    assert_ne!(fingerprint(), before);
}

// Turbo selects a package's tasks for changes to the package and to the
// workspace packages it depends on, and maps a file outside every package to
// the root package `//`, whose tasks are the only ones that file affects.
#[test]
fn affected_tests_follow_turbo_package_ownership() {
    let root = workspace();
    let affected = |path: &str| {
        let output = struo(
            root.path(),
            &["query", "affected-tests", "--changed-path", path],
        );
        success(&output);
        let tests: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        tests
            .as_array()
            .unwrap()
            .iter()
            .map(|test| test["id"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(affected("README.md"), Vec::<String>::new());
    assert_eq!(affected(".github/workflows/ci.yml"), Vec::<String>::new());
    // Controls: package sources and shared root configuration still select.
    assert_eq!(affected("app/src/value.js"), ["app__test"]);
    // `app` depends on `lib` as a package, though no task edge joins them.
    assert_eq!(affected("lib/src/value.js"), ["app__test", "lib__test"]);
    assert_eq!(affected("tsconfig.json"), ["app__test", "lib__test"]);
}

// Turbo runs a task as `<package manager> run <task>`, so the package
// manager's `pre` and `post` scripts run around it wherever that manager runs
// them: npm, Bun, Yarn 1 and pnpm 9 or later, but not Yarn 2 or later, and
// each manager's own settings override its default. Like `npm run`, each
// script gets its own shell, so a trailing comment ends only its own script.
fn append(text: &str) -> String {
    format!("node -e \"require('fs').appendFileSync('order.txt', '{text}')\"")
}

/// A one-package workspace whose `test` script has `pre` and `post` scripts
/// that record the order they ran in.
fn lifecycle_workspace(package_manager: &str, npmrc: &str, pretest: &str) -> tempfile::TempDir {
    let root = workspace();
    write_files(
        root.path(),
        &[
            (
                "package.json",
                &json!({"name": "root", "private": true, "packageManager": package_manager})
                    .to_string(),
            ),
            (".npmrc", npmrc),
            (
                "app/package.json",
                &json!({"name": "app", "scripts": {
                    "pretest": pretest,
                    "test": append("test "),
                    "posttest": append("post"),
                }})
                .to_string(),
            ),
            (
                "graph.json",
                &json!({
                    "packages": [{"name": "app", "path": "app", "allDependencies": {"items": []}}],
                    "dry": {"tasks": [task("app#test", "app", &append("test "), &[])]},
                })
                .to_string(),
            ),
        ],
    );
    root
}

#[test]
fn tasks_run_their_package_managers_lifecycle_scripts() {
    let setup = lifecycle_workspace;
    let hooks_with = |package_manager: &str, files: &[(&str, &str)]| {
        let root = setup(package_manager, "", "true");
        write_files(root.path(), files);
        let output = struo(
            root.path(),
            &[
                "query",
                "MATCH (t:Target {id: 'app__test'}) RETURN t.attrs.pre_command",
            ],
        );
        success(&output);
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        value["rows"][0][0] == "true"
    };
    let runs_hooks =
        |package_manager: &str, npmrc: &str| hooks_with(package_manager, &[(".npmrc", npmrc)]);
    assert!(runs_hooks("bun@1.4.3", ""));
    assert!(runs_hooks("pnpm@9.15.0", ""));
    assert!(runs_hooks("yarn@1.22.22", ""));
    assert!(!runs_hooks("yarn@4.18.1", ""));
    assert!(!runs_hooks("pnpm@8.15.0", ""));
    assert!(runs_hooks("pnpm@8.15.0", "enable-pre-post-scripts=true\n"));
    assert!(!runs_hooks("npm@11.0.0", "ignore-scripts=true\n"));
    assert!(!runs_hooks("npm@11.0.0", "ignore-scripts\n"));
    assert!(runs_hooks(
        "npm@11.0.0",
        "# ignore-scripts=true\nignore-scripts = \"false\"\n"
    ));
    // Only npm skips them under `ignore-scripts`.
    assert!(runs_hooks("pnpm@9.15.0", "ignore-scripts=true\n"));
    assert!(runs_hooks("yarn@1.22.22", "ignore-scripts=true\n"));
    assert!(!hooks_with(
        "pnpm@10.10.0",
        &[(
            "pnpm-workspace.yaml",
            "packages: [app]\nenablePrePostScripts: false # opt out\n"
        )],
    ));
    // pnpm leaves out a hook the script already runs itself.
    let calls_its_hook = json!({"name": "app", "scripts": {"pretest": "true", "test": "pnpm run pretest && vitest"}});
    assert!(!hooks_with(
        "pnpm@10.10.0",
        &[("app/package.json", &calls_its_hook.to_string())]
    ));
    assert!(hooks_with(
        "npm@11.0.0",
        &[("app/package.json", &calls_its_hook.to_string())]
    ));
}

// Like `npm run`, each script gets its own shell, so a trailing comment ends
// only its own script, and the first failing script stops the rest.
#[test]
fn lifecycle_scripts_run_in_order_and_stop_at_a_failure() {
    let setup = lifecycle_workspace;
    let pre = format!("{} # warm up", append("pre "));
    let root = setup("npm@11.0.0", "", &pre);
    success(&struo(root.path(), &["test", "app__test"]));
    assert_eq!(
        std::fs::read_to_string(root.path().join("app/order.txt")).unwrap(),
        "pre test post"
    );
    // A failing `pre` script stops the task.
    let root = setup("npm@11.0.0", "", "node -e \"process.exit(3)\"");
    assert!(!struo(root.path(), &["test", "app__test"]).status.success());
    assert!(!root.path().join("app/order.txt").exists());
    // So does a failing `post` script, after the task ran.
    let root = setup("npm@11.0.0", "", &append("pre "));
    let failing_post = json!({"name": "app", "scripts": {
        "pretest": append("pre "), "test": append("test "), "posttest": "node -e \"process.exit(4)\"",
    }});
    write_files(
        root.path(),
        &[("app/package.json", &failing_post.to_string())],
    );
    assert!(!struo(root.path(), &["test", "app__test"]).status.success());
    assert_eq!(
        std::fs::read_to_string(root.path().join("app/order.txt")).unwrap(),
        "pre test "
    );
}

// Turbo outputs such as medusa's `["*/**", "!src/**"]` name everything in the
// package but its sources. Leaving the negation out would drop the sources
// from every task's inputs, so editing them would replay a stale result.
#[test]
fn sources_negated_out_of_outputs_stay_inputs() {
    let root = workspace();
    let graph = json!({
        "packages": [
            {"name": "app", "path": "app", "allDependencies": {"items": []}},
            {"name": "lib", "path": "lib", "allDependencies": {"items": []}},
        ],
        "dry": {"tasks": [
            task("app#build", "app", "node -e 0", &["*/**", "!src/**"]),
            task("lib#build", "lib", "node -e 0", &[".next/**", "!.next/cache/**"]),
            task("lib#test", "lib", "node -e 0", &[]),
            task("app#test", "app", "node -e \"process.exit(require('./src/value.js') === 42 ? 0 : 1)\"", &[]),
        ]},
    });
    write_files(root.path(), &[("graph.json", &graph.to_string())]);
    assert_eq!(test_cache_state(root.path()), "miss");
    assert_eq!(test_cache_state(root.path()), "hit");
    let output = struo(
        root.path(),
        &[
            "query",
            "affected-tests",
            "--changed-path",
            "app/src/value.js",
        ],
    );
    success(&output);
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("app__test"),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    // Control: an output under its own directory is still excluded whole.
    let output = struo(
        root.path(),
        &[
            "query",
            "affected-tests",
            "--changed-path",
            "lib/.next/BUILD_ID",
        ],
    );
    success(&output);
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "[]");
    write_files(
        root.path(),
        &[("app/src/value.js", "module.exports = 41;\n")],
    );
    let output = struo(root.path(), &["test", "app__test"]);
    assert!(!output.status.success(), "a stale result was replayed");
}

// A dependency's build leaves git-ignored files, such as `tsc --build`'s
// `tsconfig.tsbuildinfo`, in a package its dependents read. The first run's
// key must already leave them out, or the second run misses once.
#[test]
fn first_runs_leave_out_what_dependencies_leave_ignored() {
    let root = workspace();
    let mut build = task(
        "lib#build",
        "lib",
        "node -e \"require('fs').writeFileSync('tsconfig.tsbuildinfo', String(Date.now()))\"",
        &[],
    );
    build["resolvedTaskDefinition"]["outputs"] = json!(["dist/**"]);
    let mut test = task("app#test", "app", "node -e 0", &[]);
    test["dependencies"] = json!(["lib#build"]);
    let graph = json!({
        "packages": [
            {"name": "app", "path": "app", "allDependencies": {"items": [{"name": "lib"}]}},
            {"name": "lib", "path": "lib", "allDependencies": {"items": []}},
        ],
        "dry": {"tasks": [build, test]},
    });
    write_files(
        root.path(),
        &[
            ("graph.json", &graph.to_string()),
            (
                ".gitignore",
                "node_modules\n.struo\n**/tsconfig.tsbuildinfo\n",
            ),
        ],
    );
    let status = Command::new("git")
        .current_dir(root.path())
        .args(["init", "-q"])
        .status()
        .unwrap();
    assert!(status.success());
    let digest = || {
        let output = struo(root.path(), &["test", "app__test"]);
        success(&output);
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .find(|record| record["target"] == "app__test")
            .map(|record| (record["action_digest"].clone(), record["cache"].clone()))
            .unwrap()
    };
    let (first, _) = digest();
    assert_eq!(digest(), (first, json!("hit")));
}
