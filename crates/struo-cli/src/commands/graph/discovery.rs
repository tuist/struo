use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{ensure, Context, Result};
use serde_json::Value;
use struo_core::{ResourceLimits, SandboxMode, WorkspacePath, Xdg};
use struo_frontend::analysis::AnalysisOptions;
use struo_frontend::GraphTarget;

use super::analysis::BuildSession;

pub(crate) async fn test_inventories(
    workspace: &Path,
    graph: &[GraphTarget],
    targets: &BTreeSet<String>,
    limits: ResourceLimits,
) -> Result<BTreeMap<String, BTreeSet<String>>> {
    let cache = crate::cache_provider::resolve(workspace, &Xdg::from_env())?;
    let session = BuildSession::new_with_options(
        workspace,
        &cache,
        graph.to_vec(),
        AnalysisOptions::default(),
        SandboxMode::Off,
    )
    .await?
    .with_resource_limits(limits);
    let targets = targets
        .iter()
        .map(|id| session.target(id))
        .collect::<Result<Vec<_>>>()?;
    let tasks = targets
        .into_iter()
        .filter(|target| {
            target
                .capabilities
                .iter()
                .any(|capability| capability.name == "test-discovery")
        })
        .map(|target| async {
            let id = &target.label.id;
            let outcome = session
                .run_with_analysis(target, "test-discovery")
                .await?
                .context("test discovery has no implementation")?;
            let path = WorkspacePath::try_from(
                outcome.provider["test_inventory"]
                    .as_str()
                    .context("test discovery omitted test_inventory")?,
            )?;
            let path = workspace.join(path.as_str());
            ensure!(
                path.canonicalize()?.starts_with(workspace.canonicalize()?),
                "test inventory escapes workspace"
            );
            ensure!(
                path.metadata()?.len() <= 1024 * 1024,
                "test inventory exceeds size limit"
            );
            let value: Value = serde_json::from_reader(std::fs::File::open(path)?)?;
            ensure!(
                value["schema"] == "struo.test_inventory.v1" && value["target"] == *id,
                "incompatible test inventory"
            );
            let values = value["units"]
                .as_array()
                .context("test inventory has no units")?;
            let units = values
                .iter()
                .map(|unit| {
                    unit.as_str()
                        .filter(|unit| !unit.is_empty())
                        .map(str::to_owned)
                        .context("invalid inventory unit")
                })
                .collect::<Result<BTreeSet<_>>>()?;
            ensure!(
                units.len() == values.len(),
                "test inventory contains duplicate units"
            );
            Ok((id.clone(), units))
        });
    Ok(futures::future::try_join_all(tasks)
        .await?
        .into_iter()
        .collect())
}
