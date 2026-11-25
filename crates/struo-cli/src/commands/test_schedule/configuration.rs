use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use struo_core::RemoteExecution;

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Configuration {
    pub overrides: Vec<String>,
    pub execution: Option<RemoteExecution>,
    pub path_suffix: String,
}

impl Configuration {
    pub fn read(path: &Path) -> Result<Self> {
        serde_json::from_reader(std::fs::File::open(path)?)
            .context("reading internal test configuration")
    }

    pub fn snapshot(&self) -> Result<tempfile::NamedTempFile> {
        let file = tempfile::NamedTempFile::new()?;
        serde_json::to_writer(file.as_file(), self)?;
        Ok(file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_preserves_resolved_execution_and_configuration_identity() {
        let workspace = tempfile::tempdir().unwrap();
        std::fs::write(workspace.path().join("struo.toml"), "").unwrap();
        let overrides = vec!["os=linux".into(), "token=release".into()];
        let mut execution = RemoteExecution::provider("workers");
        execution.executor = Some("docker".into());
        execution.environment = Some(format!("sha256:{}", "a".repeat(64)));
        execution
            .properties
            .insert("version".into(), serde_json::json!(2));
        let parent =
            crate::commands::graph::resolve_invocation_configuration(workspace.path(), &overrides)
                .unwrap()
                .with_execution(execution.clone())
                .unwrap();
        let configuration = Configuration {
            overrides,
            execution: Some(execution),
            path_suffix: parent.path_suffix.clone(),
        };
        let snapshot = configuration.snapshot().unwrap();
        let child = Configuration::read(snapshot.path()).unwrap();
        assert_eq!(child.overrides, configuration.overrides);
        assert_eq!(child.execution, configuration.execution);
        let resolved = crate::commands::graph::resolve_invocation_configuration(
            workspace.path(),
            &child.overrides,
        )
        .unwrap()
        .with_execution(child.execution.unwrap())
        .unwrap();
        assert_eq!(resolved.digest, parent.digest);
        assert_eq!(resolved.path_suffix, child.path_suffix);
        assert_eq!(resolved.path_suffix, parent.path_suffix);
    }
}
