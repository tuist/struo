#![cfg(unix)]

use std::path::Path;
use std::process::Command;

fn jest_entry_point() -> String {
    let output = Command::new("mise")
        .args(["which", "jest"])
        .output()
        .expect("mise is required to locate the pinned Jest");
    assert!(output.status.success(), "{output:?}");
    let shim = String::from_utf8(output.stdout).unwrap();
    std::fs::canonicalize(shim.trim())
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

fn write(root: &Path, path: &str, contents: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

fn struo(root: &Path, home: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_struo"))
        .arg("-C")
        .arg(root)
        .args(["--memory-limit", "512MiB"])
        .args(args)
        .env("HOME", home)
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_STATE_HOME", home.join("state"))
        .env("STRUO_CACHE_PROVIDER", "local")
        .env("STRUO_CHANGE_TRACKER", "0")
        .output()
        .unwrap()
}

// A lint target owns `src`, so before Struo read a test kind's declared data a
// change there selected no test at all. Declared data adds its test target and
// leaves the nearest-package fallback for undeclared paths in place.
#[test]
fn affected_tests_select_jest_targets_through_their_declared_data() {
    let workspace = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let root = workspace.path();
    write(
        root,
        "struo.toml",
        &format!(
            "[[target]]\nname = \"Tests\"\nkind = \"jest_test\"\nsrcs = [\"tests/*.test.js\"]\n\n[target.attrs]\nrunner = {0:?}\ndata = [\"src/**/*.js\"]\nconfig = [\"jest.config.js\"]\ndependencies = []\n\n[[target]]\nname = \"Other\"\nkind = \"jest_test\"\nsrcs = [\"other/*.test.js\"]\n\n[target.attrs]\nrunner = {0:?}\nconfig = []\ndependencies = []\n\n[[target]]\nname = \"Lint\"\nkind = \"eslint_lint\"\nsrcs = [\"src/**/*.js\"]\n",
            jest_entry_point()
        ),
    );
    for path in [
        "tests/app.test.js",
        "src/app.js",
        "jest.config.js",
        "other/other.test.js",
        "README.md",
    ] {
        write(root, path, "\n");
    }
    let affected = |path: &str| {
        let output = struo(
            root,
            home.path(),
            &[
                "--format",
                "json",
                "query",
                "affected-tests",
                "--changed-path",
                path,
            ],
        );
        assert!(output.status.success(), "{output:?}");
        let tests: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let mut ids = tests
            .as_array()
            .unwrap()
            .iter()
            .map(|test| test["id"].as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        ids.sort();
        ids
    };
    assert_eq!(affected("src/app.js"), ["Tests"]);
    assert_eq!(affected("jest.config.js"), ["Other", "Tests"]);
    assert_eq!(affected("other/other.test.js"), ["Other"]);
    assert_eq!(affected("README.md"), ["Other", "Tests"]);
}

#[test]
fn jest_ignores_package_copies_in_struo_outputs() {
    let workspace = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let root = workspace.path();
    write(
        root,
        "struo.toml",
        &format!(
            "[[target]]\nname = \"Tests\"\nkind = \"jest_test\"\nsrcs = [\"tests/*.test.js\"]\n\n[target.attrs]\nrunner = {:?}\ndata = [\"shared/**/*\"]\nconfig = [\"package.json\"]\ndependencies = []\n",
            jest_entry_point()
        ),
    );
    write(
        root,
        "package.json",
        r#"{"name":"app","jest":{"modulePathIgnorePatterns":["<rootDir>/ignored/"]}}"#,
    );
    write(
        root,
        "tests/shared.test.js",
        "test('resolves the workspace package', () => { expect(require('shared')).toBe('workspace') })\ntest('keeps configured ignores', () => { expect(() => require('other')).toThrow() })\n",
    );
    for (directory, value) in [
        ("shared", "workspace"),
        (".struo/out/Bundle/project/shared", "staged copy"),
        ("ignored/other", "ignored"),
    ] {
        let name = if directory.ends_with("other") {
            "other"
        } else {
            "shared"
        };
        write(
            root,
            &format!("{directory}/package.json"),
            &format!(r#"{{"name":"{name}","main":"index.js"}}"#),
        );
        write(
            root,
            &format!("{directory}/index.js"),
            &format!("module.exports = {value:?}\n"),
        );
    }
    let output = Command::new(env!("CARGO_BIN_EXE_struo"))
        .arg("-C")
        .arg(root)
        .args(["--memory-limit", "512MiB", "test", "Tests"])
        .env("HOME", home.path())
        .env("XDG_CACHE_HOME", home.path().join("cache"))
        .env("XDG_CONFIG_HOME", home.path().join("config"))
        .env("XDG_DATA_HOME", home.path().join("data"))
        .env("XDG_STATE_HOME", home.path().join("state"))
        .env("STRUO_CACHE_PROVIDER", "local")
        .env("STRUO_CHANGE_TRACKER", "0")
        .output()
        .unwrap();
    let log =
        std::fs::read_to_string(root.join(".struo/out/Tests/test/jest.log")).unwrap_or_default();
    assert!(output.status.success(), "{output:?}\n{log}");
}

// `config` and `dependencies` default to the manifests, lockfiles and
// installed packages, so an upgraded package reruns a cached suite and a
// manifest change selects it.
#[test]
fn jest_defaults_track_installed_packages_and_manifests() {
    let workspace = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let root = workspace.path();
    write(
        root,
        "struo.toml",
        &format!(
            "[[target]]\nname = \"Tests\"\nkind = \"jest_test\"\nsrcs = [\"tests/*.test.js\"]\n\n[target.attrs]\nrunner = {:?}\n\n[[target]]\nname = \"Lint\"\nkind = \"eslint_lint\"\nsrcs = [\"package.json\"]\n",
            jest_entry_point()
        ),
    );
    write(root, "package.json", r#"{"name":"app"}"#);
    write(
        root,
        "tests/dep.test.js",
        "test('installed value', () => { expect(require('dep')).toBe(1) })\n",
    );
    write(root, "node_modules/dep/package.json", r#"{"name":"dep"}"#);
    write(root, "node_modules/dep/index.js", "module.exports = 1\n");
    write(root, "node_modules/.modules.yaml", "prunedAt: first\n");
    write(
        root,
        "node_modules/.bin/dep",
        "#!/bin/sh\nexport NODE_PATH=/checkout\n",
    );
    let first = struo(root, home.path(), &["test", "Tests"]);
    assert!(first.status.success(), "{first:?}");
    // pnpm rewrites its install state and `.bin` shims with the install time
    // and absolute paths, and no test reads them.
    write(root, "node_modules/.modules.yaml", "prunedAt: second\n");
    write(
        root,
        "node_modules/.bin/dep",
        "#!/bin/sh\nexport NODE_PATH=/elsewhere\n",
    );
    let reinstalled = struo(root, home.path(), &["test", "Tests"]);
    assert!(
        String::from_utf8_lossy(&reinstalled.stdout).contains("cache hit"),
        "{reinstalled:?}"
    );
    write(root, "node_modules/dep/index.js", "module.exports = 2\n");
    let upgraded = struo(root, home.path(), &["test", "Tests"]);
    assert!(
        !upgraded.status.success(),
        "a changed installed package must rerun the suite: {upgraded:?}"
    );
    let selected = struo(
        root,
        home.path(),
        &[
            "--format",
            "json",
            "query",
            "affected-tests",
            "--changed-path",
            "package.json",
        ],
    );
    let tests: serde_json::Value = serde_json::from_slice(&selected.stdout).unwrap();
    assert_eq!(tests[0]["id"], "Tests", "{tests}");
}
