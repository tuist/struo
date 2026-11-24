#![cfg(unix)]

use std::process::Command;

use serde_json::{json, Value};

use super::{all_prelude_source, eval_prelude_string_function_in};

#[test]
#[allow(clippy::too_many_lines)]
fn exunit_coverage_isolates_duplicate_names_and_rejects_external_work() {
    let dir = tempfile::tempdir().unwrap();
    let workspace_alias = tempfile::tempdir().unwrap();
    let root = workspace_alias.path().join("workspace");
    std::os::unix::fs::symlink(dir.path(), &root).unwrap();
    let compilation = tempfile::tempdir().unwrap();
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/test_selection/elixir_coverage");
    std::fs::create_dir(dir.path().join("lib")).unwrap();
    std::fs::create_dir(dir.path().join("test")).unwrap();
    for file in [
        "lib/alpha.ex",
        "lib/beta.ex",
        "lib/unused.ex",
        "test/product_test.exs",
    ] {
        std::fs::copy(fixture.join(file), dir.path().join(file)).unwrap();
    }
    std::fs::create_dir(compilation.path().join("lib")).unwrap();
    for source in ["lib/alpha.ex", "lib/beta.ex", "lib/unused.ex"] {
        std::fs::copy(fixture.join(source), compilation.path().join(source)).unwrap();
    }
    let output = Command::new("elixirc")
        .current_dir(compilation.path())
        .arg("-o")
        .arg(dir.path().join("ebin"))
        .args(["lib/alpha.ex", "lib/beta.ex", "lib/unused.ex"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let output = Command::new("elixir")
        .current_dir(compilation.path())
        .args(["-e", "File.write!(hd(System.argv()), \"source_root\\t\" <> Base.encode64(:erlang.term_to_binary(File.cwd!())) <> \"\\n\")"])
        .arg(dir.path().join("compile-metadata.tsv"))
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    for (file, function) in [
        ("coverage.exs", "_elixir_coverage_hook"),
        ("adapter.py", "_elixir_coverage_adapter"),
    ] {
        std::fs::write(
            dir.path().join(file),
            eval_prelude_string_function_in(all_prelude_source(), function, "()").unwrap(),
        )
        .unwrap();
    }
    std::fs::write(
        dir.path().join("config.json"),
        serde_json::to_vec(&json!({
        "argv":["elixir", "-pa", "ebin", "coverage.exs"], "cwd":".", "root":root,
        "target":"Tests", "sources":["lib/alpha.ex", root.join("lib/beta.ex"), "lib/unused.ex"],
        "ebin":["ebin"], "tests":["test/product_test.exs"], "mix":false,
        "compile_metadata":{"ebin":"compile-metadata.tsv"},
        "native_defaults":false,"timeout_ms":null,"version":"fixture","platform":"native"}))
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
        "discovery executed tests: {output:?}: {}",
        std::fs::read_to_string(dir.path().join("out/native.log")).unwrap()
    );
    let inventory = read();
    assert_eq!(
        inventory["units"],
        json!([
            "Tests::AlphaTest/test same name",
            "Tests::BetaTest/test same name"
        ])
    );
    let mut observations = Vec::new();
    for (unit, covered) in [
        ("Tests::AlphaTest/test same name", 0),
        ("Tests::BetaTest/test same name", 1),
    ] {
        let output = Command::new("python3")
            .current_dir(dir.path())
            .args(base)
            .args(["--struo-test-unit", unit])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{output:?}: {}",
            std::fs::read_to_string(dir.path().join("out/native.log")).unwrap()
        );
        let report = read();
        assert_eq!(report["coverage"]["partial"], false, "{report}");
        assert_eq!(report["cases"][0]["file"], "test/product_test.exs");
        for index in 0..3 {
            assert_eq!(
                report["coverage"]["files"][index]["execution_counts"],
                json!([u64::from(index == covered)])
            );
        }
        observations.push(Ok(report));
    }
    let merged = struo_core::merge_test_coverage("Tests", &inventory, observations, false).unwrap();
    assert_eq!(
        merged["coverage_evidence"]["paths"],
        json!(["lib/alpha.ex", "lib/beta.ex"])
    );
    for (variable, success) in [("STRUO_FIXTURE_CHILD", true), ("STRUO_FIXTURE_FAIL", false)] {
        let output = Command::new("python3")
            .current_dir(dir.path())
            .args(base)
            .args(["--struo-test-unit", "Tests::AlphaTest/test same name"])
            .env(variable, "1")
            .output()
            .unwrap();
        assert_eq!(output.status.success(), success, "{output:?}");
        let report = read();
        assert_eq!(report["coverage"]["partial"], true, "{variable}: {report}");
    }
    let config_path = dir.path().join("config.json");
    let original_config = std::fs::read(&config_path).unwrap();
    let mut config: Value = serde_json::from_slice(&original_config).unwrap();
    config["timeout_ms"] = json!(1);
    std::fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();
    let output = Command::new("python3")
        .current_dir(dir.path())
        .args(base)
        .args(["--struo-test-unit", "Tests::AlphaTest/test same name"])
        .env("STRUO_FIXTURE_SLOW", "1")
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "native timeout was ignored: {output:?}"
    );
    let report = read();
    assert_eq!(report["cases"][0]["status"], "failed", "{report}");
    assert_eq!(report["coverage"]["partial"], true, "{report}");
    std::fs::write(&config_path, original_config).unwrap();
    std::fs::write(
        dir.path().join("test/product_test.exs"),
        "invalid syntax ! %\n",
    )
    .unwrap();
    let output = Command::new("python3")
        .current_dir(dir.path())
        .args(base)
        .arg("--struo-inventory")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!dir.path().join("out/results.json").exists());

    std::fs::copy(
        fixture.join("test/product_test.exs"),
        dir.path().join("test/product.check.exs"),
    )
    .unwrap();
    std::fs::write(dir.path().join("test/test_helper.exs"), "ExUnit.start()\n").unwrap();
    std::fs::write(dir.path().join("mix.exs"), "defmodule Demo.MixProject do\n  use Mix.Project\n  def project, do: [app: :demo, version: \"0.1.0\", test_load_filters: [~r/check.exs$/]]\nend\n").unwrap();
    let config_path = dir.path().join("config.json");
    let mut config: Value = serde_json::from_slice(&std::fs::read(&config_path).unwrap()).unwrap();
    config["mix"] = json!(true);
    config["native_defaults"] = json!(true);
    config["no_start"] = json!(true);
    std::fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();
    let output = Command::new("python3")
        .current_dir(dir.path())
        .args(base)
        .arg("--struo-inventory")
        .env("STRUO_FIXTURE_FAIL", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "Mix discovery did not honor native load filters: {output:?}: {}",
        std::fs::read_to_string(dir.path().join("out/native.log")).unwrap()
    );
    assert_eq!(read(), inventory);
    let output = Command::new("python3")
        .current_dir(dir.path())
        .args(base)
        .args(["--struo-test-unit", "Tests::BetaTest/test same name"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{output:?}: {}",
        std::fs::read_to_string(dir.path().join("out/native.log")).unwrap()
    );
    assert_eq!(read()["coverage"]["partial"], false);
}
