use std::path::Path;
use std::process::{Command, Output};

fn struo(workspace: &Path, home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_struo"))
        .arg("-C")
        .arg(workspace)
        .args(["--memory-limit", "512MiB", "--cpu-limit", "1"])
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

/// Source sets outside `srcs` still select the tests that compile them, and
/// test targets keep their runner contract.
fn assert_graph_queries(workspace: &Path, home: &Path) {
    let affected = struo(
        workspace,
        home,
        &[
            "--format",
            "json",
            "query",
            "affected-tests",
            "--changed-path",
            "src/commonMain/Platform.kt",
        ],
    );
    assert!(affected.status.success(), "{affected:?}");
    let affected: serde_json::Value = serde_json::from_slice(&affected.stdout).unwrap();
    let selected = affected
        .as_array()
        .unwrap()
        .iter()
        .map(|record| record["id"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(selected, ["LibraryTests"], "{affected}");

    let tests = struo(workspace, home, &["--format", "json", "query", "tests"]);
    let tests: serde_json::Value = serde_json::from_slice(&tests.stdout).unwrap();
    assert!(
        tests
            .as_array()
            .unwrap()
            .iter()
            .all(|record| record["runner"] == "kotlin_jvm"),
        "{tests}"
    );
}

#[test]
fn kotlin_source_sets_match_expect_and_actual_across_refined_fragments() {
    let workspace = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    for (file, source) in [
        (
            "src/commonMain/Platform.kt",
            "expect fun platformName(): String\nexpect fun separator(): String\nfun describe() = platformName() + separator()\n",
        ),
        ("src/nonJsMain/Separator.kt", "actual fun separator() = \":\"\n"),
        ("src/jvmMain/Platform.jvm.kt", "actual fun platformName() = \"jvm\" + separator()\n"),
        (
            "src/commonTest/DescribeTest.kt",
            "expect fun platformUnderTest(): String\nclass DescribeTest {\n    fun testDescribe() {\n        check(describe() == platformUnderTest()) { describe() }\n    }\n}\n",
        ),
        ("src/jvmTest/TestPlatform.kt", "actual fun platformUnderTest() = \"jvm::\"\n"),
        ("other/OtherTest.kt", "class OtherTest { fun testOther() {} }\n"),
    ] {
        let path = workspace.path().join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, source).unwrap();
    }
    std::fs::write(
        workspace.path().join("struo.toml"),
        r#"[[target]]
name = "Library"
kind = "kotlin_jvm_library"
srcs = ["src/jvmMain/**/*.kt"]

[target.attrs]
source_sets = { commonMain = ["src/commonMain/**/*.kt"], nonJsMain = ["src/nonJsMain/**/*.kt"] }
source_set_depends_on = { nonJsMain = ["commonMain"] }

[[target]]
name = "LibraryTests"
kind = "kotlin_jvm_test"
srcs = ["src/jvmTest/**/*.kt"]
deps = ["./Library"]

[target.attrs]
source_sets = { commonTest = ["src/commonTest/**/*.kt"] }

[[target]]
name = "OtherTests"
kind = "kotlin_jvm_test"
srcs = ["other/**/*.kt"]
"#,
    )
    .unwrap();

    assert_graph_queries(workspace.path(), home.path());

    let build = struo(workspace.path(), home.path(), &["build", "Library"]);
    assert!(build.status.success(), "{build:?}");
    assert!(workspace
        .path()
        .join(".struo/out/Library/Library.jar")
        .is_file());

    let test = struo(workspace.path(), home.path(), &["test", "LibraryTests"]);
    assert!(test.status.success(), "{test:?}");
    let results = struo(
        workspace.path(),
        home.path(),
        &[
            "--format",
            "json",
            "query",
            "test-results",
            "LibraryTests",
            "--summary-only",
        ],
    );
    assert!(results.status.success(), "{results:?}");
    let summary: serde_json::Value = serde_json::from_slice(&results.stdout).unwrap();
    assert_eq!(summary["summary"]["total"], 1, "{summary}");
    assert_eq!(summary["summary"]["passed"], 1, "{summary}");
}
