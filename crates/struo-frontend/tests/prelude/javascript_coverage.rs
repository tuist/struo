use std::process::Command;

use serde_json::{json, Value};

use super::{all_prelude_source, eval_prelude_string_function_in};

#[test]
#[allow(clippy::too_many_lines)]
fn javascript_coverage_keeps_zero_hits_and_rejects_child_or_missing_observations() {
    let dir = tempfile::tempdir().unwrap();
    for (file, function) in [
        ("adapter.mjs", "_javascript_test_adapter"),
        ("hook.cjs", "_javascript_coverage_hook"),
    ] {
        std::fs::write(
            dir.path().join(file),
            eval_prelude_string_function_in(all_prelude_source(), function, "()").unwrap(),
        )
        .unwrap();
    }
    std::fs::create_dir(dir.path().join("out")).unwrap();
    for file in ["product.cjs", "unused.cjs"] {
        std::fs::write(dir.path().join(file), "exports.value = () => 41\n").unwrap();
    }
    std::fs::write(dir.path().join("runner.cjs"), r"
const fs = require('node:fs'), path = require('node:path')
const args = process.argv.slice(2)
const directory = args.find(arg => arg.startsWith('--coverageDirectory=')).split('=')[1]
fs.mkdirSync(directory, {recursive: true})
const coverage = {}
for (const [file, count] of [['product.cjs', 3], ['unused.cjs', 0]]) {
  if (process.env.MISSING_SOURCE && file === 'unused.cjs') continue
  coverage[path.resolve(file)] = {statementMap: {'0': {start: {line: 1}}}, s: {'0': count}}
}
fs.writeFileSync(directory + '/coverage-final.json', JSON.stringify(coverage))
if (process.env.CHILD_WORK) require('node:child_process').spawnSync(process.execPath, ['-e', 'process.exit(0)'])
fs.writeFileSync(args.find(arg => arg.startsWith('--outputFile=')).split('=')[1], JSON.stringify({success:true, testResults:[{
  name: path.resolve('product.test.cjs'), assertionResults:[{title:'value', fullName:'value', status:'passed'}]
}]}))
").unwrap();
    for (cwd, unit, product) in [
        (
            ".",
            "tests::%5B%22product.test.cjs%22%2C%22value%22%5D",
            "product.cjs",
        ),
        (
            "package",
            "tests::%5B%22package%2Fproduct.test.cjs%22%2C%22value%22%5D",
            "package/product.cjs",
        ),
    ] {
        if cwd != "." {
            std::fs::create_dir(dir.path().join(cwd)).unwrap();
            for file in ["product.cjs", "unused.cjs"] {
                std::fs::copy(dir.path().join(file), dir.path().join(cwd).join(file)).unwrap();
            }
        }
        let runner = dir.path().join("runner.cjs");
        let mut command = Command::new("node");
        command.current_dir(dir.path()).args([
            "adapter.mjs",
            "--struo-runner-type",
            "jest",
            "--struo-runner",
            runner.to_str().unwrap(),
            "--struo-runner-via-node",
            "true",
            "--struo-unit",
            "case",
            "--struo-target",
            "tests",
            "--struo-test-unit",
            unit,
            "--struo-results",
            "out/results.json",
            "--struo-native-results",
            "out/native.json",
            "--struo-coverage-hook",
            "hook.cjs",
            "--struo-collector-version",
            "fixture-1",
            "--struo-coverage-source",
            product,
            "--struo-coverage-source",
            if cwd == "." {
                "unused.cjs"
            } else {
                "package/unused.cjs"
            },
        ]);
        if cwd != "." {
            command.args(["--struo-cwd", cwd]);
        }
        for incomplete in [None, Some("CHILD_WORK"), Some("MISSING_SOURCE")] {
            command
                .env_remove("CHILD_WORK")
                .env_remove("MISSING_SOURCE");
            if let Some(variable) = incomplete {
                command.env(variable, "1");
            }
            let output = command.output().unwrap();
            assert!(output.status.success(), "{output:?}");
            let report: Value = serde_json::from_slice(
                &std::fs::read(dir.path().join("out/results.json")).unwrap(),
            )
            .unwrap();
            let merged = struo_core::merge_test_coverage(
                "tests",
                &json!({"schema":"struo.test_inventory.v1", "target":"tests", "units":[unit]}),
                [Ok(report)],
                false,
            )
            .unwrap();
            if incomplete.is_none() {
                assert_eq!(merged["coverage"]["partial"], false, "{merged}");
                assert_eq!(merged["coverage"]["files"][1]["covered_lines"], 0);
                assert_eq!(merged["coverage_evidence"]["paths"], json!([product]));
            } else {
                assert!(merged.get("coverage").is_none(), "{merged}");
                assert_ne!(merged["coverage_evidence"]["status"], "collected");
            }
        }
    }
}
