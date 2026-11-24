#![cfg(target_os = "linux")]

use std::process::Command;

use serde_json::{json, Value};

use super::{all_prelude_source, eval_prelude_string_function_in};

#[test]
#[ignore = "requires Clang/LLVM 18, CMake and Linux strace"]
#[allow(clippy::too_many_lines)]
fn ctest_coverage_preserves_invocation_identity_and_rejects_child_work() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/test_selection/cmake_coverage");
    for file in ["CMakeLists.txt", "tests.c", "product.c", "unused.c"] {
        std::fs::copy(fixture.join(file), dir.path().join(file)).unwrap();
    }
    let output = Command::new("cmake")
        .current_dir(dir.path())
        .args(["-S", ".", "-B", "build", "-DCMAKE_C_COMPILER=clang"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let output = Command::new("cmake")
        .current_dir(dir.path())
        .args(["--build", "build"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    std::fs::write(
        dir.path().join("adapter.py"),
        eval_prelude_string_function_in(all_prelude_source(), "_cmake_coverage_adapter", "()")
            .unwrap(),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("config.json"),
        serde_json::to_vec(
            &json!({"ctest":"ctest","build":"build","build_type":"Debug",
            "target":"Tests","sources":["product.c","unused.c"],"version":"fixture",
            "cov":"llvm-cov-18","profdata":"llvm-profdata-18","trace":"strace"}),
        )
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
        "out/native.xml",
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
    assert_eq!(
        inventory["units"],
        json!(["Tests::alpha.one", "Tests::beta+two"])
    );
    let mut observations = Vec::new();
    for (unit, counts) in [
        ("Tests::alpha.one", json!([1, 0])),
        ("Tests::beta+two", json!([0, 1])),
    ] {
        let output = Command::new("python3")
            .current_dir(dir.path())
            .args(base)
            .args(["--struo-test-unit", unit])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let report = read();
        assert_eq!(report["coverage"]["partial"], false, "{report}: {output:?}");
        assert_eq!(report["coverage"]["files"][0]["execution_counts"], counts);
        assert_eq!(
            report["coverage"]["files"][1]["execution_counts"],
            json!([0])
        );
        observations.push(Ok(report));
    }
    let merged = struo_core::merge_test_coverage("Tests", &inventory, observations, false).unwrap();
    assert_eq!(
        merged["coverage_evidence"]["scopes"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    for (variable, success) in [("STRUO_FIXTURE_CHILD", true), ("STRUO_FIXTURE_FAIL", false)] {
        let output = Command::new("python3")
            .current_dir(dir.path())
            .args(base)
            .args(["--struo-test-unit", "Tests::alpha.one"])
            .env(variable, "1")
            .output()
            .unwrap();
        assert_eq!(output.status.success(), success, "{output:?}");
        let report = read();
        assert_eq!(report["coverage"]["partial"], true, "{report}");
        let merged = struo_core::merge_test_coverage(
            "Tests", &json!({"schema":"struo.test_inventory.v1","target":"Tests","units":["Tests::alpha.one"]}),
            [Ok(report)], false,
        ).unwrap();
        assert_ne!(merged["coverage_evidence"]["status"], "collected");
    }
}
