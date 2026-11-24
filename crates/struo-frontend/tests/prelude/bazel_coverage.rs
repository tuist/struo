#![cfg(unix)]

use std::process::Command;

use serde_json::{json, Value};

use super::{all_prelude_source, eval_prelude_string_function_in};

#[test]
#[ignore = "requires Bazel 7.6.1 with native C++ coverage support"]
#[allow(clippy::too_many_lines)]
fn bazel_coverage_preserves_native_invocation_and_rejects_incomplete_runs() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/test_selection/bazel_coverage");
    for file in [
        ".bazelversion",
        ".bazelrc",
        "WORKSPACE",
        "BUILD",
        "product.cc",
        "unused.cc",
        "tests.cc",
    ] {
        std::fs::copy(fixture.join(file), dir.path().join(file)).unwrap();
    }
    std::fs::write(
        dir.path().join("adapter.py"),
        eval_prelude_string_function_in(all_prelude_source(), "_bazel_coverage_adapter", "()")
            .unwrap(),
    )
    .unwrap();
    std::fs::write(dir.path().join("config.json"), serde_json::to_vec(&json!({
        "bazel":"bazelisk", "root":dir.path(), "cwd":dir.path(), "target":"Tests", "label":"//:tests",
        "sources":[dir.path().join("product.cc"), dir.path().join("unused.cc")], "version":"fixture", "platform":"host",
        "test_env":{"STRUO_FIXTURE_FAIL":"", "STRUO_FIXTURE_SKIP":""}
    })).unwrap()).unwrap();
    let base = [
        "adapter.py",
        "--struo-config",
        "config.json",
        "--struo-results",
        "out/results.json",
        "--struo-native-results",
        "out/native.log",
    ];
    let read = || -> Value {
        serde_json::from_slice(&std::fs::read(dir.path().join("out/results.json")).unwrap())
            .unwrap()
    };
    let output = Command::new("python3")
        .current_dir(dir.path())
        .args(base)
        .arg("--struo-inventory")
        .env("STRUO_FIXTURE_FAIL", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "discovery executed tests: {output:?}"
    );
    assert_eq!(read()["units"], json!(["Tests::invocation"]));
    let inventory = read();
    for (variable, success, partial) in [
        ("NONE", true, false),
        ("STRUO_FIXTURE_SKIP", true, true),
        ("STRUO_FIXTURE_FAIL", false, true),
    ] {
        let output = Command::new("python3")
            .current_dir(dir.path())
            .args(base)
            .args(["--struo-test-unit", "Tests::invocation"])
            .env(variable, "1")
            .output()
            .unwrap();
        assert_eq!(output.status.success(), success, "{variable}: {output:?}");
        let report = read();
        assert_eq!(
            report["coverage"]["partial"],
            partial,
            "{variable}: {report}: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert_eq!(report["cases"][0]["id"], "Tests::invocation");
        if !partial {
            assert_eq!(
                report["coverage"]["files"][0]["execution_counts"],
                json!([1])
            );
            assert_eq!(
                report["coverage"]["files"][1]["execution_counts"],
                json!([0])
            );
            let merged =
                struo_core::merge_test_coverage("Tests", &inventory, [Ok(report)], false).unwrap();
            assert_eq!(
                merged["coverage_evidence"]["scopes"][0]["name"],
                "Tests::invocation"
            );
        }
    }
    std::fs::write(dir.path().join("BUILD"), "broken syntax ! %").unwrap();
    let output = Command::new("python3")
        .current_dir(dir.path())
        .args(base)
        .arg("--struo-inventory")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        !dir.path().join("out/results.json").exists(),
        "failed discovery reused prior inventory"
    );
}
