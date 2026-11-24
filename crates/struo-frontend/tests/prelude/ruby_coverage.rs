#![cfg(unix)]

use std::process::Command;

use serde_json::{json, Value};

use super::{all_prelude_source, eval_prelude_string_function_in};

fn source(function: &str) -> String {
    eval_prelude_string_function_in(all_prelude_source(), function, "()").unwrap()
}

#[test]
fn ruby_coverage_measures_native_units_without_promoting_file_aggregates() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("out")).unwrap();
    std::fs::write(dir.path().join("hook.rb"), source("_ruby_coverage_hook")).unwrap();
    std::fs::write(
        dir.path().join("product.rb"),
        "module Product\n  def self.value\n    41\n  end\nend\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("unused.rb"),
        "module Unused\n  def self.value\n    17\n  end\nend\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("test_product.rb"), "require 'minitest/autorun'\nrequire_relative 'product'\nclass ProductTest < Minitest::Test\n  def test_value\n    assert_equal 41, Product.value\n  end\n  def test_second\n    assert_equal 82, Product.value * 2\n  end\nend\n").unwrap();
    std::fs::write(dir.path().join("product_spec.rb"), "require_relative 'product'\nRSpec.describe Product do\n  it('value') { expect(Product.value).to eq(41) }\nend\n").unwrap();
    for (kind, native_source, unit) in [
        ("minitest", "test_product.rb", "test_product.rb"),
        ("rspec", "product_spec.rb", "./product_spec.rb[1:1]"),
    ] {
        std::fs::write(
            dir.path().join("adapter.rb"),
            source(&format!("_ruby_{kind}_adapter")),
        )
        .unwrap();
        let mut command = Command::new("ruby");
        command.current_dir(dir.path()).args([
            "adapter.rb",
            "--struo-target",
            "tests",
            "--struo-source",
            native_source,
            "--struo-test-unit",
            &format!("tests::{unit}"),
            "--struo-results",
            "out/result.json",
            "--struo-native-results",
            "out/native.json",
            "--struo-coverage-hook",
            "hook.rb",
            "--struo-coverage-source",
            "product.rb",
            "--struo-coverage-source",
            "unused.rb",
        ]);
        if kind == "rspec" {
            command.args(["--struo-runner", "rspec"]);
        }
        let output = command.output().unwrap();
        assert!(output.status.success(), "{kind}: {output:?}");
        let report: Value =
            serde_json::from_slice(&std::fs::read(dir.path().join("out/result.json")).unwrap())
                .unwrap();
        assert_eq!(report["cases"].as_array().unwrap().len(), 1, "{report}");
        assert_eq!(report["cases"][0]["id"], format!("tests::{unit}"));
        let merged = struo_core::merge_test_coverage("tests", &json!({"schema":"struo.test_inventory.v1","target":"tests","units":[format!("tests::{unit}")]}), [Ok(report)], false).unwrap();
        assert_eq!(merged["coverage"]["partial"], false, "{kind}: {merged}");
        assert_eq!(merged["coverage"]["files"][1]["path"], "unused.rb");
        assert_eq!(merged["coverage"]["files"][1]["covered_lines"], 0);
        assert_eq!(merged["coverage_evidence"]["paths"], json!(["product.rb"]));
        assert_eq!(
            merged["coverage_evidence"]["scopes"][0]["name"],
            format!("tests::{unit}")
        );
        std::fs::write(dir.path().join("product.rb"), "module Product\n  def self.value\n    system(RbConfig.ruby, '-e', 'exit 0')\n    41\n  end\nend\n").unwrap();
        let output = command.output().unwrap();
        assert!(output.status.success(), "{kind}: {output:?}");
        let report: Value =
            serde_json::from_slice(&std::fs::read(dir.path().join("out/result.json")).unwrap())
                .unwrap();
        assert_eq!(report["coverage"]["partial"], true, "{kind}: {report}");
        std::fs::write(
            dir.path().join("product.rb"),
            "module Product\n  def self.value\n    41\n  end\nend\n",
        )
        .unwrap();
    }
}
