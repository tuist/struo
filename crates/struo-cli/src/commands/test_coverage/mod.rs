mod context;
mod history;
mod selection;

pub(crate) use selection::narrow;

use std::collections::BTreeMap;
use std::path::Path;
use std::process::ExitCode;

use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use struo_core::{ResourceLimits, SandboxMode, TestPlan};
use struo_frontend::GraphTarget;

use super::query::test_plan::portable::PortableTestPlan;
use super::test_schedule::hosted::{client::Host, metadata, report};

#[allow(clippy::too_many_arguments)]
pub(crate) async fn run(
    workspace: &Path,
    graph: Vec<GraphTarget>,
    output: crate::cli::Output,
    plan: TestPlan,
    jobs: Option<usize>,
    sandbox: SandboxMode,
    limits: ResourceLimits,
) -> Result<ExitCode> {
    let portable = PortableTestPlan::export(workspace, &graph, plan.clone())?;
    let before = context::configuration(workspace, &graph);
    let commit = context::clean_commit(workspace).await;
    let metadata = metadata::collect(workspace).await;
    let execution =
        super::test_schedule::execute(workspace, Some(graph.clone()), plan, jobs, sandbox, limits)
            .await?;
    let succeeded = execution.runs.iter().all(|run| run["success"] == true);
    let value = json!({"schema": "struo.portable_test_execution.v1", "plan_id": portable.id,
        "selected_batch_ids": portable.plan.batches.iter().map(|batch| &batch.id).collect::<Vec<_>>(),
        "complete": true, "success": succeeded, "execution": execution});
    let submitted = match before {
        Ok(before) => {
            publish(
                workspace,
                &graph,
                &portable,
                &value,
                &before,
                commit.as_deref(),
                metadata,
            )
            .await
        }
        Err(error) => Err(error),
    };
    if let Err(error) = &submitted {
        tracing::warn!(%error, "coverage report unavailable; tests retain their execution verdict");
    }
    let result = json!({"execution": value, "coverage_reported": submitted.is_ok(),
        "coverage_error": submitted.err().map(|error| format!("{error:#}"))});
    super::query::write_body(
        output,
        || {
            format!(
                "Tests {}; coverage report {}\n",
                if succeeded { "passed" } else { "failed" },
                if result["coverage_reported"] == true {
                    "submitted"
                } else {
                    "unavailable"
                }
            )
        },
        &result,
    )
    .await?;
    Ok(if succeeded {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

#[allow(clippy::too_many_arguments)]
async fn publish(
    workspace: &Path,
    graph: &[GraphTarget],
    portable: &PortableTestPlan,
    value: &Value,
    before: &BTreeMap<String, context::Configuration>,
    commit: Option<&str>,
    metadata: Value,
) -> Result<()> {
    let commit = commit.context("coverage baselines require a clean Git checkout")?;
    ensure!(
        value["success"] == true,
        "failed executions cannot publish coverage checkpoints"
    );
    ensure!(
        context::clean_commit(workspace).await.as_deref() == Some(commit),
        "checkout changed during coverage collection"
    );
    ensure!(
        *before == context::configuration(workspace, graph)?,
        "test configuration changed during coverage collection"
    );
    portable.validate(workspace, graph)?;
    let assigned = portable
        .plan
        .batches
        .iter()
        .map(|batch| batch.id.clone())
        .collect::<Vec<_>>();
    let (modules, _) = report::project(value, portable, &assigned)?;
    ensure!(
        !modules.is_empty(),
        "no complete coverage targets to report"
    );
    let host = Host::resolve(workspace)?;
    history::sync(workspace, &host, commit).await?;
    for module in modules {
        let target = module["name"].as_str().context("missing reported target")?;
        let runs = value["execution"]["runs"]
            .as_array()
            .context("missing executions")?;
        let matching = runs
            .iter()
            .filter(|run| run["target"] == target)
            .collect::<Vec<_>>();
        ensure!(
            matching.len() == 1
                && matching[0]["record"]["cache"] == "bypass"
                && portable
                    .plan
                    .batches
                    .iter()
                    .any(|batch| batch.target == target && batch.test_filters.is_empty()),
            "coverage reporting requires a freshly measured complete whole-target observation"
        );
        let results = &matching[0]["results"];
        let configuration = before
            .get(target)
            .context("target has no coverage selection contract")?;
        let scope = context::observation_scope(results, target, configuration)?;
        let mut coverage = results["coverage"].clone();
        for file in coverage["files"]
            .as_array_mut()
            .context("missing coverage files")?
        {
            let path = file["path"].as_str().context("missing coverage path")?;
            let blob = metadata::git(workspace, &["rev-parse", &format!("{commit}:{path}")])
                .await
                .context("coverage source is not tracked at the measured commit")?;
            file["git_blob_id"] = blob.into();
        }
        let mut payload = metadata.clone();
        payload.as_object_mut().unwrap().extend(
            json!({
                "id": uuid::Uuid::now_v7().to_string(), "build_system": "once", "scheme": scope,
                "ran_at": chrono::Utc::now().to_rfc3339(), "duration": module["duration"],
                "status": module["status"], "test_modules": [module], "git_dirty": false,
                "coverage": coverage, "coverage_evidence": results["coverage_evidence"],
            })
            .as_object()
            .unwrap()
            .clone(),
        );
        host.post("", &payload).await?;
    }
    Ok(())
}
