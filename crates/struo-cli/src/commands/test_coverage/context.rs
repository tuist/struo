use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::Path;

use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use struo_core::{InputDigestBuilder, WorkspacePath};
use struo_frontend::{analysis::AnalysisEngine, GraphTarget};

use crate::commands::test_schedule::hosted::metadata;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Configuration {
    digest: String,
    pub platform: String,
    tool: String,
    version: String,
}

pub(super) fn scope(
    target: &str,
    configuration: &Configuration,
    units: &BTreeSet<String>,
) -> Result<String> {
    ensure!(!units.is_empty(), "coverage inventory is empty");
    let mut digest = InputDigestBuilder::new(b"struo.coverage-scope.v1");
    digest.push_bytes(target.as_bytes());
    digest.push_bytes(configuration.digest.as_bytes());
    digest.push_bytes(&serde_json::to_vec(units)?);
    Ok(format!("struo.coverage.v1.{}", digest.finish()))
}

pub(super) fn configuration(
    workspace: &Path,
    graph: &[GraphTarget],
) -> Result<BTreeMap<String, Configuration>> {
    let engine = AnalysisEngine::for_workspace(workspace)?;
    let mut configurations = BTreeMap::new();
    for target in graph.iter().filter(|target| {
        target
            .capabilities
            .iter()
            .any(|capability| capability.name == "test")
    }) {
        let provider = engine
            .analyze_target_capability(target, workspace, &[], "metadata")?
            .provider;
        let Some(contract) = provider.get("coverage_selection") else {
            continue;
        };
        let mut digest = InputDigestBuilder::new(b"struo.coverage-configuration.v1");
        digest.push_bytes(engine.implementation_fingerprint().as_bytes());
        digest.push_bytes(&serde_json::to_vec(
            &json!({"graph": graph, "contract": contract}),
        )?);
        for input in contract["inputs"]
            .as_array()
            .context("coverage_selection inputs must be an array")?
        {
            let input = WorkspacePath::try_from(
                input
                    .as_str()
                    .context("coverage selection input must be a path")?,
            )?;
            digest.push_bytes(input.as_str().as_bytes());
            let mut file = std::fs::File::open(workspace.join(input.as_str()))?;
            let mut buffer = [0; 16 * 1024];
            loop {
                let count = file.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                digest.push_bytes(&buffer[..count]);
            }
        }
        configurations.insert(
            target.label.id.clone(),
            Configuration {
                digest: digest.finish().to_string(),
                platform: required(contract, "platform")?,
                tool: required(contract, "tool")?,
                version: required(contract, "tool_version")?,
            },
        );
    }
    Ok(configurations)
}

fn required(value: &Value, field: &str) -> Result<String> {
    let text = value[field]
        .as_str()
        .filter(|text| !text.is_empty())
        .with_context(|| format!("coverage selection missing {field}"))?;
    Ok(text.to_string())
}

pub(super) fn observation_scope(
    results: &Value,
    target: &str,
    configuration: &Configuration,
) -> Result<String> {
    struo_core::validate_test_results(results, target)?;
    ensure!(
        results["status"] == "passed"
            && results["coverage"]["partial"] == false
            && results["coverage_evidence"]["status"] == "collected",
        "coverage observation is incomplete or unsuccessful"
    );
    let coverage = &results["coverage"];
    ensure!(
        coverage["tool"] == configuration.tool
            && coverage["tool_version"] == configuration.version
            && coverage["platform"] == configuration.platform,
        "coverage collector is incompatible"
    );
    let units = results["cases"]
        .as_array()
        .context("missing cases")?
        .iter()
        .map(|case| {
            ensure!(case["status"] == "passed", "test did not pass");
            required(case, "id")
        })
        .collect::<Result<BTreeSet<_>>>()?;
    ensure!(!units.is_empty(), "coverage inventory is empty");
    let paths = results["coverage_evidence"]["paths"]
        .as_array()
        .context("missing evidence paths")?;
    for path in paths {
        WorkspacePath::try_from(path.as_str().context("invalid evidence path")?)?;
    }
    let scopes = results["coverage_evidence"]["scopes"]
        .as_array()
        .context("missing evidence scopes")?;
    let mut attributed = BTreeSet::new();
    for scope in scopes {
        ensure!(
            scope["kind"] == "test" && scope["module"] == target && scope["suite"] == "",
            "coverage must be isolated per-test evidence"
        );
        let name = required(scope, "name")?;
        ensure!(
            units.contains(&name) && attributed.insert(name),
            "coverage identity does not match inventory"
        );
        for file in scope["files"].as_array().context("missing scope files")? {
            ensure!(
                file.as_u64()
                    .is_some_and(|index| index < paths.len() as u64),
                "coverage path index out of range"
            );
        }
    }
    ensure!(
        units == attributed,
        "not every test has attributed coverage"
    );
    let files = coverage["files"]
        .as_array()
        .context("missing instrumented file universe")?
        .iter()
        .map(|file| {
            let path = required(file, "path")?;
            WorkspacePath::try_from(path.as_str())?;
            Ok(path)
        })
        .collect::<Result<BTreeSet<_>>>()?;
    ensure!(
        !files.is_empty()
            && paths
                .iter()
                .all(|path| path.as_str().is_some_and(|path| files.contains(path))),
        "coverage universe is incomplete"
    );
    scope(target, configuration, &units)
}

pub(super) async fn clean_commit(workspace: &Path) -> Option<String> {
    let dirty = metadata::git_output(
        workspace,
        &["status", "--porcelain", "--untracked-files=normal"],
    )
    .await?;
    if !dirty.is_empty() {
        return None;
    }
    metadata::git(workspace, &["rev-parse", "HEAD"]).await
}
