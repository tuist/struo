use std::collections::{BTreeMap, BTreeSet};

use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};

use super::{validate_test_results, TEST_RESULTS_SCHEMA};

mod counters;

pub fn merge_test_coverage(
    target: &str,
    inventory: &Value,
    observations: impl IntoIterator<Item = Result<Value>>,
    partial: bool,
) -> Result<Value> {
    ensure!(
        inventory["schema"] == "struo.test_inventory.v1" && inventory["target"] == target,
        "incompatible coverage inventory"
    );
    let listed = inventory["units"]
        .as_array()
        .context("missing inventory units")?;
    let units = listed
        .iter()
        .map(|unit| {
            unit.as_str()
                .filter(|unit| !unit.is_empty())
                .context("invalid inventory unit")
        })
        .collect::<Result<BTreeSet<_>>>()?;
    ensure!(
        units.len() == listed.len(),
        "duplicate coverage inventory units"
    );
    let mut cases = BTreeMap::new();
    let mut counters = counters::Counters::default();
    let mut artifacts = BTreeMap::<String, BTreeSet<String>>::new();
    let mut runner = json!({"type":"isolated", "metadata":{}});
    let mut complete = !partial && !units.is_empty();
    let mut passed = true;
    for observation in observations {
        let observation = observation?;
        validate_test_results(&observation, target)?;
        let observed = observation["cases"]
            .as_array()
            .context("missing observed cases")?;
        ensure!(
            observed.len() == 1,
            "coverage requires one exact unit per observation"
        );
        let case = &observed[0];
        let unit = case["id"].as_str().context("missing observed unit")?;
        ensure!(units.contains(unit), "coverage observed an unknown unit");
        ensure!(
            cases.insert(unit.to_owned(), case.clone()).is_none(),
            "duplicate coverage observation"
        );
        passed &= observation["status"] == "passed"
            && matches!(case["status"].as_str(), Some("passed" | "skipped"));
        complete &= observation["status"] == "passed" && case["status"] == "passed";
        if cases.len() == 1 {
            runner = observation["runner"].clone();
        }
        for kind in ["logs", "native_results", "coverage"] {
            if let Some(values) = observation["artifacts"][kind].as_array() {
                for path in values {
                    artifacts
                        .entry(kind.into())
                        .or_default()
                        .insert(path.as_str().context("invalid artifact")?.into());
                }
            }
        }
        complete &= counters.observe(target, unit, &observation["coverage"])?;
    }
    complete &= units.iter().all(|unit| cases.contains_key(*unit));
    passed &= units.iter().all(|unit| cases.contains_key(*unit));
    let passed_cases = cases
        .values()
        .filter(|case| case["status"] == "passed")
        .count();
    let skipped = cases
        .values()
        .filter(|case| case["status"] == "skipped")
        .count();
    let total = cases.len();
    let mut result = json!({"schema":TEST_RESULTS_SCHEMA, "target":target, "runner":runner,
        "status":if passed {"passed"} else {"failed"},
        "summary":{"total":total,"passed":passed_cases,"failed":total-passed_cases-skipped,"skipped":skipped,"flaky":0},
        "cases":cases.into_values().collect::<Vec<_>>(),
        "artifacts":{"logs":artifacts.remove("logs").unwrap_or_default(),"native_results":artifacts.remove("native_results").unwrap_or_default(),"coverage":artifacts.remove("coverage").unwrap_or_default()},
    });
    counters.finish(target, complete, &mut result);
    validate_test_results(&result, target)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observation(unit: &str, counts: [u64; 2]) -> Value {
        json!({"schema":TEST_RESULTS_SCHEMA,"target":"suite","runner":{"type":"native","metadata":{}},
            "status":"passed","summary":{"total":1,"passed":1,"failed":0,"skipped":0,"flaky":0},
            "cases":[{"id":unit,"name":unit,"suite":"native-suite","status":"passed","attempts":[{"status":"passed"}],"runner_metadata":{}}],
            "artifacts":{"logs":[],"native_results":[]},
            "coverage":{"tool":"collector","tool_version":"4","platform":"host","partial":false,
                "files":[{"path":"product.txt","line_numbers":[3,9],"execution_counts":counts},
                         {"path":"zero.txt","line_numbers":[7],"execution_counts":[0]}]}})
    }

    #[test]
    fn attributes_only_isolated_positive_hits_and_preserves_zero_hit_universe() {
        let inventory =
            json!({"schema":"struo.test_inventory.v1","target":"suite","units":["a","b"]});
        let result = merge_test_coverage(
            "suite",
            &inventory,
            [Ok(observation("b", [0, 0])), Ok(observation("a", [2, 5]))],
            false,
        )
        .unwrap();
        assert_eq!(result["coverage"]["partial"], false);
        assert_eq!(
            result["coverage"]["files"][0]["execution_counts"],
            json!([2, 5])
        );
        assert_eq!(result["coverage"]["files"][1]["covered_lines"], 0);
        assert_eq!(result["coverage_evidence"]["paths"], json!(["product.txt"]));
        assert_eq!(result["coverage_evidence"]["scopes"][0]["files"], json!([]));
        assert_eq!(result["coverage_evidence"]["scopes"][1]["name"], "a");
    }

    #[test]
    fn incomplete_inventory_instrumentation_or_execution_cannot_establish_a_checkpoint() {
        let inventory =
            json!({"schema":"struo.test_inventory.v1","target":"suite","units":["a","b"]});
        for mode in ["missing", "instrumentation", "failed", "partial"] {
            let mut second = observation("b", [1, 0]);
            if mode == "instrumentation" {
                second["coverage"]["files"].as_array_mut().unwrap().pop();
            }
            if mode == "failed" {
                second["status"] = json!("failed");
                second["cases"][0]["status"] = json!("failed");
            }
            let mut observations = vec![Ok(observation("a", [0, 3]))];
            if mode != "missing" {
                observations.push(Ok(second));
            }
            let result =
                merge_test_coverage("suite", &inventory, observations, mode == "partial").unwrap();
            assert_eq!(result["coverage"]["partial"], true, "{mode}");
            assert_eq!(result["coverage_evidence"]["scopes"], json!([]), "{mode}");
        }
        let duplicate = merge_test_coverage(
            "suite",
            &inventory,
            [Ok(observation("a", [1, 0])), Ok(observation("a", [0, 1]))],
            false,
        );
        assert!(duplicate.is_err());
        let mut aggregate = observation("a", [1, 0]);
        aggregate["cases"]
            .as_array_mut()
            .unwrap()
            .push(observation("b", [0, 1])["cases"][0].clone());
        assert!(merge_test_coverage("suite", &inventory, [Ok(aggregate)], false).is_err());
    }
}
