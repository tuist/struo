use std::io::{Read, Write};
use std::path::Path;

use anyhow::{ensure, Context, Result};
use serde_json::Value;

use crate::WorkspacePath;

pub(crate) fn merge_files(
    root: &Path,
    target: &str,
    inventory: &WorkspacePath,
    results: &[WorkspacePath],
    output: &WorkspacePath,
    partial: bool,
) -> Result<bool> {
    let root = root.canonicalize()?;
    let mut remaining = 16 * 1024 * 1024;
    let inventory = read(&root, inventory, &mut remaining)?;
    let observations = results.iter().map(|path| read(&root, path, &mut remaining));
    let result = super::merge_test_coverage(target, &inventory, observations, partial)?;
    let output = output.resolve(&root);
    let parent = output.parent().context("coverage output has no parent")?;
    let ancestor = parent
        .ancestors()
        .find(|path| path.exists())
        .context("coverage output has no existing ancestor")?;
    ensure!(
        ancestor.canonicalize()?.starts_with(&root),
        "coverage output escapes workspace"
    );
    std::fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer(temporary.as_file_mut(), &result)?;
    temporary.flush()?;
    temporary.persist(output)?;
    Ok(result["status"] == "passed")
}

fn read(root: &Path, path: &WorkspacePath, remaining: &mut usize) -> Result<Value> {
    let path = path.resolve(root).canonicalize()?;
    ensure!(
        path.starts_with(root),
        "coverage observation escapes workspace"
    );
    let file = std::fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take(u64::try_from(*remaining)? + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= *remaining,
        "coverage observations exceed 16 MiB limit"
    );
    *remaining -= bytes.len();
    Ok(serde_json::from_slice(&bytes)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{run_uncached, Action};
    use serde_json::json;

    #[tokio::test]
    async fn assembly_action_publishes_current_verdict_and_never_reuses_complete_evidence() {
        let root = tempfile::tempdir().unwrap();
        let cache_dir = tempfile::tempdir().unwrap();
        let cache = struo_cas::CacheProvider::open_local(cache_dir.path());
        std::fs::write(
            root.path().join("inventory.json"),
            json!({"schema":"struo.test_inventory.v1","target":"tests","units":["unit"]})
                .to_string(),
        )
        .unwrap();
        let mut observation = json!({"schema":"struo.test_results.v1","target":"tests","runner":{"type":"native","metadata":{}},
            "status":"passed","summary":{"total":1,"passed":1,"failed":0,"skipped":0,"flaky":0},
            "cases":[{"id":"unit","name":"unit","suite":"suite","status":"passed","attempts":[{"status":"passed"}],"runner_metadata":{}}],
            "artifacts":{"logs":[],"native_results":[]},"coverage":{"tool":"collector","tool_version":"1","platform":"host","partial":false,
            "files":[{"path":"source.txt","line_numbers":[2],"execution_counts":[1]}]}});
        let action = Action::MergeTestCoverage {
            target: "tests".into(),
            inventory: WorkspacePath::try_from("inventory.json").unwrap(),
            results: vec![WorkspacePath::try_from("observation.json").unwrap()],
            output: WorkspacePath::try_from("out/results.json").unwrap(),
            partial: false,
            input_digest: None,
        };
        for failed in [false, true] {
            if failed {
                observation["status"] = json!("failed");
                observation["cases"][0]["status"] = json!("failed");
            }
            std::fs::write(
                root.path().join("observation.json"),
                observation.to_string(),
            )
            .unwrap();
            let result = run_uncached(&action, root.path(), &cache, false)
                .await
                .unwrap();
            assert_eq!(result.exit_code, i32::from(failed));
            assert!(result.outputs.contains_key("out/results.json"));
            let report: Value = serde_json::from_slice(
                &std::fs::read(root.path().join("out/results.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(report["coverage"]["partial"], failed);
            assert_eq!(
                report["coverage_evidence"]["status"],
                if failed { "failed" } else { "collected" }
            );
        }
    }
}
