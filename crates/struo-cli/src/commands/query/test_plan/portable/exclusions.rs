use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{ensure, Result};
use struo_core::TestPlan;
use struo_frontend::GraphTarget;

use super::discovery_fingerprint;

pub(super) fn bind(
    workspace: &Path,
    graph: &[GraphTarget],
    plan: &TestPlan,
) -> Result<BTreeMap<String, String>> {
    targets(workspace, graph, plan)?
        .into_iter()
        .map(|target| {
            let fingerprint = discovery_fingerprint(workspace, graph, &target)?;
            Ok((target, fingerprint))
        })
        .collect()
}

pub(super) fn validate(
    workspace: &Path,
    graph: &[GraphTarget],
    plan: &TestPlan,
    bindings: &BTreeMap<String, String>,
) -> Result<()> {
    ensure!(
        bindings.keys().cloned().collect::<BTreeSet<_>>() == targets(workspace, graph, plan)?,
        "portable coverage exclusions do not cover the graph-selected envelope"
    );
    for (target, fingerprint) in bindings {
        ensure!(
            *fingerprint == discovery_fingerprint(workspace, graph, target)?,
            "portable test plan is stale for excluded target `{target}`; export a new plan"
        );
    }
    Ok(())
}

fn targets(workspace: &Path, graph: &[GraphTarget], plan: &TestPlan) -> Result<BTreeSet<String>> {
    if plan.selection.policy.evidence != "declared_graph_and_opt_in_coverage" {
        return Ok(BTreeSet::new());
    }
    ensure!(
        plan.selection.policy.safety == "approximate",
        "coverage-assisted plans must be approximate"
    );
    let envelope =
        super::super::selection::selection_report(workspace, graph, &plan.selection.changed_paths)?
            .tests
            .into_iter()
            .map(|test| test.id)
            .collect::<BTreeSet<_>>();
    let retained = plan
        .selection
        .tests
        .iter()
        .map(|test| test.id.clone())
        .collect::<BTreeSet<_>>();
    ensure!(
        retained.is_subset(&envelope),
        "coverage-assisted plan contains targets outside the graph-selected envelope"
    );
    Ok(envelope.difference(&retained).cloned().collect())
}
