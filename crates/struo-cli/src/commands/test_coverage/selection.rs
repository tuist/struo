use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use struo_core::{ResourceLimits, TestPlan};
use struo_frontend::GraphTarget;

use super::{context, history};
use crate::commands::{
    graph::test_inventories,
    test_schedule::hosted::{client::Host, metadata},
};

pub(crate) async fn narrow(
    workspace: &Path,
    graph: &[GraphTarget],
    plan: TestPlan,
    limits: ResourceLimits,
) -> Result<TestPlan> {
    let mut selection = plan.selection.clone();
    selection.policy.safety = "approximate".into();
    selection.policy.evidence = "declared_graph_and_opt_in_coverage".into();
    let excluded = exclusions(workspace, graph, &plan, limits).await;
    match excluded {
        Ok(excluded) => selection
            .tests
            .retain(|target| !excluded.contains(&target.id)),
        Err(error) => {
            tracing::warn!(%error, "coverage selection unavailable; retaining graph-selected tests");
            for target in &mut selection.tests {
                target.reasons.push(format!("coverage fallback: {error:#}"));
            }
        }
    }
    let retained = selection
        .tests
        .iter()
        .map(|test| test.id.as_str())
        .collect::<BTreeSet<_>>();
    let batches = plan
        .batches
        .into_iter()
        .filter(|batch| retained.contains(batch.target.as_str()))
        .collect();
    Ok(TestPlan::new(selection, batches)?)
}

async fn exclusions(
    workspace: &Path,
    graph: &[GraphTarget],
    plan: &TestPlan,
    limits: ResourceLimits,
) -> Result<BTreeSet<String>> {
    ensure!(
        !plan.selection.changed_paths.is_empty() && plan.selection.unmatched_paths.is_empty(),
        "coverage selection requires owned changed paths"
    );
    let commit = context::clean_commit(workspace)
        .await
        .context("coverage selection requires a clean Git checkout")?;
    let configurations = context::configuration(workspace, graph)?;
    let targets = plan
        .selection
        .tests
        .iter()
        .filter(|target| configurations.contains_key(&target.id))
        .map(|target| target.id.clone())
        .collect();
    let inventories = test_inventories(workspace, graph, &targets, limits).await?;
    let host = Host::resolve(workspace)?;
    history::sync(workspace, &host, &commit).await?;
    let parents = metadata::git(workspace, &["show", "-s", "--format=%P", "HEAD"])
        .await
        .unwrap_or_default()
        .split_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mut excluded = BTreeSet::new();
    for target in &plan.selection.tests {
        let Some(configuration) = configurations.get(&target.id) else {
            continue;
        };
        let Some(units) = inventories.get(&target.id) else {
            continue;
        };
        let Ok(scope) = context::scope(&target.id, configuration, units) else {
            continue;
        };
        let mut request = json!({"build_system": "once", "commit_sha": commit, "parent_shas": parents,
            "scope": scope, "platform": configuration.platform});
        let baseline = host
            .post("selective-testing/coverage/baseline", &request)
            .await?;
        let Some(baseline) = baseline["baseline_sha"].as_str() else {
            continue;
        };
        let ancestor = metadata::git(workspace, &["merge-base", baseline, &commit])
            .await
            .context("coverage baseline ancestry unavailable")?;
        ensure!(ancestor == baseline, "coverage baseline is not an ancestor");
        let changed = changes(workspace, baseline, &commit, &target.id).await?;
        let actual = changed
            .iter()
            .filter_map(|file| file["path"].as_str())
            .collect::<BTreeSet<_>>();
        ensure!(
            actual
                == plan
                    .selection
                    .changed_paths
                    .iter()
                    .map(String::as_str)
                    .collect(),
            "changed paths do not match the baseline diff"
        );
        request["head_sha"] = commit.clone().into();
        request["coverage_policy"] = "instrumented".into();
        request["baseline_sha"] = baseline.into();
        request["modules"] = json!([{"name": target.id, "skipped": false}]);
        request["changed_files_count"] = changed.len().into();
        request["changed_files"] = changed.into();
        let decision = host
            .post("selective-testing/coverage/runs", &request)
            .await?;
        if excludes_target(&decision, &target.id, units)? {
            excluded.insert(target.id.clone());
        }
    }
    ensure!(
        context::clean_commit(workspace).await.as_deref() == Some(commit.as_str()),
        "checkout changed during coverage selection"
    );
    Ok(excluded)
}

fn excludes_target(decision: &Value, target: &str, units: &BTreeSet<String>) -> Result<bool> {
    if decision["outcome"] != "selected" {
        return Ok(false);
    }
    let entries = decision["skipped_tests"]
        .as_array()
        .context("coverage decision has no exclusions")?;
    let mut skipped = BTreeSet::new();
    let mut whole_target = false;
    for entry in entries {
        ensure!(
            entry["module"] == target,
            "coverage decision contains an unexpected target"
        );
        match (entry.get("suite"), entry.get("name")) {
            (None, None) => whole_target = true,
            (Some(suite), Some(name)) if suite == "" => {
                let name = name.as_str().context("invalid excluded unit")?;
                ensure!(
                    units.contains(name),
                    "coverage decision contains an unknown unit"
                );
                skipped.insert(name.to_string());
            }
            _ => anyhow::bail!("coverage decision contains an unsupported exclusion"),
        }
    }
    Ok(!units.is_empty() && (whole_target || skipped == *units))
}

async fn changes(
    workspace: &Path,
    baseline: &str,
    commit: &str,
    target: &str,
) -> Result<Vec<Value>> {
    let diff = metadata::git_output(
        workspace,
        &[
            "diff",
            "--name-status",
            "--no-renames",
            "-z",
            baseline,
            commit,
        ],
    )
    .await
    .context("Git diff unavailable")?;
    let mut fields = diff.split('\0').filter(|field| !field.is_empty());
    let mut changes = Vec::new();
    while let Some(status) = fields.next() {
        let path = fields.next().context("incomplete Git diff")?;
        struo_core::WorkspacePath::try_from(path)?;
        ensure!(
            matches!(status, "M"),
            "new, deleted, renamed or unknown inputs retain graph-selected work"
        );
        let blob = metadata::git(workspace, &["rev-parse", &format!("{commit}:{path}")])
            .await
            .context("changed input blob unavailable")?;
        changes.push(
            json!({"path": path, "status": status, "blob": blob, "affected_modules": [target]}),
        );
        ensure!(changes.len() <= 5000, "coverage diff exceeds limit");
    }
    Ok(changes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_complete_opaque_inventory_exclusions_remove_a_target() {
        let units = BTreeSet::from(["opaque-a".into(), "opaque-b".into()]);
        let decision = |entries| json!({"outcome": "selected", "skipped_tests": entries});
        assert!(!excludes_target(
            &decision(json!([{"module":"t", "suite":"", "name":"opaque-a"}])),
            "t",
            &units
        )
        .unwrap());
        assert!(excludes_target(&decision(json!([{"module":"t", "suite":"", "name":"opaque-a"}, {"module":"t", "suite":"", "name":"opaque-b"}])), "t", &units).unwrap());
        assert!(excludes_target(&decision(json!([{"module":"t"}])), "t", &units).unwrap());
        assert!(excludes_target(
            &decision(json!([{"module":"t"}, {"module":"other"}])),
            "t",
            &units
        )
        .is_err());
        assert!(excludes_target(
            &decision(json!([{"module":"t", "suite":"", "name":"display-name"}])),
            "t",
            &units
        )
        .is_err());
        assert!(!excludes_target(&json!({"outcome":"fallback"}), "t", &units).unwrap());
    }
}
