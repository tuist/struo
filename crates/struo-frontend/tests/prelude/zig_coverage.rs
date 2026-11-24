#![cfg(target_os = "linux")]

use std::process::Command;

use serde_json::{json, Value};

use super::{all_prelude_source, eval_prelude_string_function_in};

#[test]
#[ignore = "requires Zig 0.16 and kcov 43 with ptrace access"]
#[allow(clippy::too_many_lines)]
fn zig_coverage_preserves_atomic_boundary_and_rejects_incomplete_execution() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/test_selection/zig_coverage");
    for file in ["tests.zig", "product.zig", "unused.zig"] {
        std::fs::copy(fixture.join(file), dir.path().join(file)).unwrap();
    }
    let output = Command::new("zig")
        .current_dir(dir.path())
        .args([
            "test",
            "tests.zig",
            "--test-no-exec",
            "-fllvm",
            "-femit-bin=tests",
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    std::fs::write(
        dir.path().join("adapter.py"),
        eval_prelude_string_function_in(all_prelude_source(), "_zig_coverage_adapter", "()")
            .unwrap(),
    )
    .unwrap();
    let version = Command::new("kcov").arg("--version").output().unwrap();
    assert!(version.status.success(), "{version:?}");
    let version = String::from_utf8(version.stdout).unwrap();
    std::fs::write(
        dir.path().join("config.json"),
        serde_json::to_vec(&json!({"binary":"tests","target":"Tests","name":"Tests",
            "sources":["product.zig","unused.zig"],"kcov":"kcov","version":version.trim(),"platform":"native"}))
        .unwrap(),
    )
    .unwrap();
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
        "discovery executed bodies: {output:?}"
    );
    let inventory = read();
    assert_eq!(inventory["units"], json!(["Tests::suite"]));
    let output = Command::new("python3")
        .current_dir(dir.path())
        .args(base)
        .args(["--struo-test-unit", "Tests::suite"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let report = read();
    assert_eq!(report["coverage"]["partial"], false, "{report}: {output:?}");
    assert_eq!(
        report["coverage"]["files"][0]["execution_counts"],
        json!([1, 1, 1, 1])
    );
    assert_eq!(
        report["coverage"]["files"][1]["execution_counts"],
        json!([0, 0])
    );
    let merged = struo_core::merge_test_coverage("Tests", &inventory, [Ok(report)], false).unwrap();
    assert_eq!(
        merged["coverage_evidence"]["scopes"][0]["name"],
        "Tests::suite"
    );
    assert_eq!(merged["coverage_evidence"]["paths"], json!(["product.zig"]));
    for (variable, success) in [
        ("STRUO_FIXTURE_UNUSED", true),
        ("STRUO_FIXTURE_CHILD", true),
        ("STRUO_FIXTURE_SKIP", true),
        ("STRUO_FIXTURE_FAIL", false),
    ] {
        let output = Command::new("python3")
            .current_dir(dir.path())
            .args(base)
            .args(["--struo-test-unit", "Tests::suite"])
            .env(variable, "1")
            .output()
            .unwrap();
        assert_eq!(output.status.success(), success, "{variable}: {output:?}");
        let report = read();
        assert_eq!(
            report["coverage"]["partial"],
            variable != "STRUO_FIXTURE_UNUSED",
            "{variable}: {report}: {output:?}"
        );
        if variable == "STRUO_FIXTURE_UNUSED" {
            assert_eq!(
                report["coverage"]["files"][1]["execution_counts"],
                json!([1, 1])
            );
        }
    }
}
