use std::path::Path;
use std::time::Duration;

use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use struo_core::WorkspacePath;

use crate::commands::test_schedule::hosted::client::Host;

pub(super) async fn sync(workspace: &Path, host: &Host, commit: &str) -> Result<()> {
    ensure!(
        git(workspace, &["rev-parse", "--show-prefix"])
            .await?
            .is_empty(),
        "coverage history requires the repository root"
    );
    let repository = git(workspace, &["remote", "get-url", "origin"]).await?;
    let settings = host.get("git-history/settings").await?;
    let window = setting(&settings, "window_commits", 10_000)?;
    let days = setting(&settings, "window_days", 3650)?;
    let batch = setting(&settings, "upload_batch_size", 10_000)?;
    let limit = setting(&settings, "commit_file_limit", 1_000_000)?;
    let format = git(workspace, &["rev-parse", "--show-object-format"]).await?;
    let shallow = git(workspace, &["rev-parse", "--is-shallow-repository"]).await? == "true";
    let since = chrono::Utc::now() - chrono::Duration::days(i64::try_from(days)?);
    let log = git(
        workspace,
        &[
            "log",
            &format!("-n{window}"),
            &format!("--since={}", since.to_rfc3339()),
            "--topo-order",
            "--format=%H%x00%P%x00%cI",
            commit,
        ],
    )
    .await?;
    let mut commits = Vec::new();
    for line in log.lines().rev() {
        let fields = line.split('\0').collect::<Vec<_>>();
        ensure!(fields.len() == 3, "invalid Git history record");
        let mut parents = fields[1]
            .split_whitespace()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if shallow && parents.is_empty() {
            let object = git(workspace, &["cat-file", "-p", fields[0]]).await?;
            parents = object
                .lines()
                .take_while(|line| !line.is_empty())
                .filter_map(|line| line.strip_prefix("parent ").map(str::to_owned))
                .collect();
        }
        commits.push(json!({"sha": fields[0], "parents": parents, "committed_at": fields[2]}));
    }
    for chunk in commits.chunks(batch) {
        let shas = chunk.iter().map(|entry| &entry["sha"]).collect::<Vec<_>>();
        let missing = host
            .post(
                "git-history/commits/missing",
                &json!({"repository_url": repository, "shas": shas}),
            )
            .await?;
        let missing = missing["missing"]
            .as_array()
            .context("missing Git history response")?;
        let records = chunk
            .iter()
            .filter(|entry| missing.contains(&entry["sha"]))
            .collect::<Vec<_>>();
        if !records.is_empty() {
            host.post(
                "git-history/commits",
                &json!({"repository_url": repository, "object_format": format, "commits": records}),
            )
            .await?;
        }
    }
    sync_listing(workspace, host, &repository, commit, batch, limit).await
}

async fn sync_listing(
    workspace: &Path,
    host: &Host,
    repository: &str,
    commit: &str,
    batch: usize,
    limit: usize,
) -> Result<()> {
    let missing = host
        .post(
            "git-history/listings/missing",
            &json!({"repository_url": repository, "shas": [commit]}),
        )
        .await?;
    if !missing["missing"]
        .as_array()
        .context("missing listing response")?
        .contains(&json!(commit))
    {
        return Ok(());
    }
    let tree = git(workspace, &["ls-tree", "-r", "-z", commit]).await?;
    let mut files = Vec::new();
    for entry in tree.split('\0').filter(|entry| !entry.is_empty()) {
        let (header, path) = entry.split_once('\t').context("invalid Git tree entry")?;
        WorkspacePath::try_from(path)?;
        let fields = header.split_whitespace().collect::<Vec<_>>();
        ensure!(fields.len() == 3, "invalid Git tree header");
        files.push(json!({"path": path, "git_blob_id": fields[2], "mode": u32::from_str_radix(fields[0], 8)?}));
        ensure!(
            files.len() <= limit,
            "Git listing exceeds server limit; retaining graph selection"
        );
    }
    for (index, chunk) in files.chunks(batch.min(20_000)).enumerate() {
        let complete = (index + 1) * batch.min(20_000) >= files.len();
        host.post("git-history/listings", &json!({"repository_url": repository, "sha": commit, "files": chunk, "complete": complete, "truncated": false, "files_count": files.len()})).await?;
    }
    Ok(())
}

fn setting(value: &Value, field: &str, maximum: usize) -> Result<usize> {
    let number = value[field]
        .as_u64()
        .context("invalid Git history settings")?;
    let number = usize::try_from(number)?;
    ensure!(number > 0, "invalid Git history limit");
    Ok(number.min(maximum))
}

async fn git(workspace: &Path, args: &[&str]) -> Result<String> {
    let mut command = tokio::process::Command::new("git");
    command
        .current_dir(workspace)
        .arg("--no-replace-objects")
        .args(args)
        .kill_on_drop(true);
    let output = crate::commands::util::capture_tokio_command_output(
        &mut command,
        Some(Duration::from_secs(10)),
    )
    .await?;
    ensure!(
        output.status.success() && !output.stdout_truncated,
        "Git history unavailable or exceeds output limit"
    );
    Ok(String::from_utf8(output.stdout)?
        .trim_end_matches('\n')
        .to_string())
}
