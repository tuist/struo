#![cfg(target_os = "linux")]

use std::process::Command;

use serde_json::{json, Value};

use super::{all_prelude_source, eval_prelude_string_function_in};

#[test]
#[ignore = "requires JACOCO_TOOLS containing jacocoagent.jar/jacococli.jar and Linux strace"]
#[allow(clippy::too_many_lines)]
fn jvm_coverage_measures_exact_units_and_rejects_spawned_work() {
    let tools = std::path::PathBuf::from(std::env::var_os("JACOCO_TOOLS").expect("JACOCO_TOOLS"));
    let dir = tempfile::tempdir().unwrap();
    for (file, function) in [
        ("StruoJvmTestRunner.java", "_jvm_test_runner_source"),
        ("adapter.py", "_jvm_coverage_adapter"),
    ] {
        std::fs::write(
            dir.path().join(file),
            eval_prelude_string_function_in(all_prelude_source(), function, "()").unwrap(),
        )
        .unwrap();
    }
    for (name, value) in [("First", 41), ("Second", 17), ("Unused", 29)] {
        std::fs::write(
            dir.path().join(format!("{name}.java")),
            format!("public class {name} {{ public static int value() {{ return {value}; }} }}\n"),
        )
        .unwrap();
    }
    std::fs::write(
        dir.path().join("Suite.java"),
        r#"
public class Suite {
  public void testFirst() throws Exception {
    if (System.getenv("SPAWN") != null) {
      if (new ProcessBuilder("/bin/true").start().waitFor() != 0) throw new AssertionError();
    }
    if (System.getenv("FAIL") != null || First.value() != 41) throw new AssertionError();
  }
  public void testSecond() { if (Second.value() != 17) throw new AssertionError(); }
}
"#,
    )
    .unwrap();
    let output = Command::new("javac")
        .current_dir(dir.path())
        .args([
            "-g",
            "-d",
            "classes",
            "StruoJvmTestRunner.java",
            "First.java",
            "Second.java",
            "Unused.java",
            "Suite.java",
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let config = json!({"argv":["java","-cp","classes","StruoJvmTestRunner","classes"],
        "target":"Tests","runner":"kotlin_jvm","filters":[],"trace":"strace",
        "agent":tools.join("jacocoagent.jar"),"cli":tools.join("jacococli.jar"),"version":"fixture",
        "sources":["First.java","Second.java","Unused.java"],"classfiles":["classes"]});
    std::fs::write(
        dir.path().join("config.json"),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    let base = [
        "adapter.py",
        "--struo-config",
        "config.json",
        "--struo-results",
        "out/results.json",
        "--struo-native-results",
        "out/native.txt",
    ];
    let output = Command::new("python3")
        .current_dir(dir.path())
        .args(base)
        .arg("--struo-inventory")
        .env("FAIL", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "discovery ran test bodies: {output:?}"
    );
    let read = || -> Value {
        serde_json::from_slice(&std::fs::read(dir.path().join("out/results.json")).unwrap())
            .unwrap()
    };
    let inventory = read();
    assert_eq!(
        inventory["units"],
        json!(["Tests::Suite.testFirst", "Tests::Suite.testSecond"])
    );
    let mut observations = Vec::new();
    for unit in ["Tests::Suite.testFirst", "Tests::Suite.testSecond"] {
        let mut command = Command::new("python3");
        command
            .current_dir(dir.path())
            .args(base)
            .args(["--struo-test-unit", unit]);
        let output = command.output().unwrap();
        assert!(output.status.success(), "{output:?}");
        let measured = read();
        assert_eq!(
            measured["coverage"]["partial"], false,
            "{measured} {output:?}"
        );
        observations.push(Ok(measured));
        if unit.ends_with("testFirst") {
            let output = command.env("SPAWN", "1").output().unwrap();
            assert!(output.status.success(), "{output:?}");
            assert_eq!(read()["coverage"]["partial"], true);
            let output = command
                .env_remove("SPAWN")
                .env("FAIL", "1")
                .output()
                .unwrap();
            assert!(!output.status.success());
            assert_eq!(read()["status"], "failed");
            assert_eq!(read()["coverage"]["partial"], true);
        }
    }
    let merged = struo_core::merge_test_coverage("Tests", &inventory, observations, false).unwrap();
    assert_eq!(merged["coverage"]["partial"], false, "{merged}");
    assert_eq!(merged["coverage"]["files"][2]["covered_lines"], 0);
    assert_eq!(
        merged["coverage_evidence"]["paths"],
        json!(["First.java", "Second.java"])
    );
    assert_eq!(
        merged["coverage_evidence"]["scopes"][0]["files"],
        json!([0])
    );
    assert_eq!(
        merged["coverage_evidence"]["scopes"][1]["files"],
        json!([1])
    );
}
