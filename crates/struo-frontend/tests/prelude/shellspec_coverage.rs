#![cfg(target_os = "linux")]

use std::process::Command;

use serde_json::{json, Value};

use super::{all_prelude_source, eval_prelude_string_function_in};

#[test]
#[ignore = "requires ShellSpec 0.28, kcov 43, Bash and strace"]
#[allow(clippy::too_many_lines)]
fn shellspec_coverage_includes_lifecycle_and_rejects_incomplete_invocations() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/test_selection/shellspec_coverage");
    for file in [
        ".shellspec",
        "product.sh",
        "unused.sh",
        "lifecycle.sh",
        "product_spec.sh",
    ] {
        std::fs::copy(fixture.join(file), dir.path().join(file)).unwrap();
    }
    std::fs::write(
        dir.path().join("adapter.py"),
        eval_prelude_string_function_in(all_prelude_source(), "_shellspec_coverage_adapter", "()")
            .unwrap(),
    )
    .unwrap();
    let hook =
        eval_prelude_string_function_in(all_prelude_source(), "_shellspec_coverage_hook", "()")
            .unwrap();
    std::fs::write(dir.path().join("config.json"), serde_json::to_vec(&json!({
        "shellspec":"shellspec", "bash":"bash", "kcov":"kcov", "strace":"strace", "root":dir.path(), "target":"Tests",
        "specs":["product_spec.sh"], "args":[], "hook":hook, "version":"fixture", "platform":"linux",
        "sources":[dir.path().join("product.sh"), dir.path().join("unused.sh"), dir.path().join("lifecycle.sh")]
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
    let listing = Command::new("python3")
        .current_dir(dir.path())
        .args(base)
        .arg("--struo-inventory")
        .env("STRUO_FIXTURE_FAIL", "1")
        .output()
        .unwrap();
    assert!(
        listing.status.success(),
        "discovery executed cases: {listing:?}"
    );
    assert_eq!(read()["units"], json!(["Tests::invocation"]));
    let inventory = read();
    for (variable, success, partial) in [
        ("NONE", true, false),
        ("STRUO_FIXTURE_CHILD", true, true),
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
        assert_eq!(
            output.status.success(),
            success,
            "{variable}: {output:?}: {}",
            std::fs::read_to_string(dir.path().join("out/native.log")).unwrap()
        );
        let report = read();
        assert_eq!(
            report["coverage"]["partial"],
            partial,
            "{variable}: {report}: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        if !partial {
            let files = report["coverage"]["files"].as_array().unwrap();
            assert!(files[0]["execution_counts"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value.as_u64().unwrap() > 0));
            assert!(files[1]["execution_counts"]
                .as_array()
                .unwrap()
                .iter()
                .all(|value| value == 0));
            let lifecycle = &files[2];
            for expected in [2, 6] {
                let index = lifecycle["line_numbers"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .position(|line| line == expected)
                    .unwrap();
                assert!(
                    lifecycle["execution_counts"][index].as_u64().unwrap() > 0,
                    "lifecycle line {expected} was not collected: {lifecycle}"
                );
            }
            let merged =
                struo_core::merge_test_coverage("Tests", &inventory, [Ok(report)], false).unwrap();
            assert_eq!(
                merged["coverage_evidence"]["scopes"][0]["name"],
                "Tests::invocation"
            );
        }
    }
    std::fs::remove_file(dir.path().join("product_spec.sh")).unwrap();
    let listing = Command::new("python3")
        .current_dir(dir.path())
        .args(base)
        .arg("--struo-inventory")
        .output()
        .unwrap();
    assert!(!listing.status.success());
    assert!(!dir.path().join("out/results.json").exists());
}
