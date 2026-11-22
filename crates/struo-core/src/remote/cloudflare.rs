use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;
use serde_json::json;
use struo_cas::{ActionResult, CacheProvider};

use super::archive::{
    create_input_archive_limited, install_output_archive_limited, output_archive_file,
};
use super::{lifecycle, phase, PreparedCommand};
use crate::executor::staging::CapturePermit;
use crate::stream::{self, Destination};
use crate::{
    resolve_execution_argv, resolve_execution_env, Error, ExecutionPermit, RemoteExecution, Result,
};

mod client;
use client::{Client, Sandbox, LIFETIME, PROVIDER};

pub(super) async fn execute_command(
    remote: &RemoteExecution,
    command: PreparedCommand<'_>,
    workspace: &Path,
    cache: &CacheProvider,
    stream_to_parent: bool,
) -> Result<ActionResult> {
    let limits = command
        .lease
        .as_ref()
        .map_or_else(crate::RemoteIoLimits::default, ExecutionPermit::io_limits);
    let client = Client::from_env(limits)?;
    Box::pin(execute(
        remote,
        command,
        workspace,
        cache,
        stream_to_parent,
        client,
    ))
    .await
}

async fn execute(
    remote: &RemoteExecution,
    command: PreparedCommand<'_>,
    workspace: &Path,
    cache: &CacheProvider,
    stream_to_parent: bool,
    client: Client,
) -> Result<ActionResult> {
    if command
        .timeout_ms
        .is_some_and(|ms| u128::from(ms) >= LIFETIME.as_millis())
    {
        return Err(Error::RemoteProviderConfig { provider: PROVIDER.into(), message: "Cloudflare bridge actions must finish provisioning, transfers and execution within one hour".into() });
    }
    let mut deadline = phase::Deadline::new(Some(command.timeout_ms.unwrap_or(3_600_000)));
    let (archive, input_phase) = deadline
        .within(phase::run(
            PROVIDER,
            "prepare",
            create_input_archive_limited(
                workspace,
                command.inputs,
                PROVIDER,
                client.limits,
                command.lease.as_ref(),
            ),
        ))
        .await?;
    let capture = match command.lease.as_ref() {
        Some(lease) => {
            let capture = lease.capture_budget();
            deadline.within(capture.reserve_upfront()).await?;
            Some(capture)
        }
        None => None,
    };
    let journal = lifecycle::Journal::new(workspace);
    deadline
        .within(async {
            lifecycle::reconcile(&journal, &client, lifecycle::Budget::default()).await;
            Ok(())
        })
        .await?;
    let environment = remote
        .environment
        .clone()
        .unwrap_or_else(|| "cloudflare/debian-trixie".into());
    let resources = command.resources.clone();
    let limits = client.limits;
    let (sandbox, cleanup) = deadline.within(phase::run(PROVIDER, "create", lifecycle::create_owned(
        client.clone(), Some(journal), LIFETIME + std::time::Duration::from_mins(10), command.lease.clone(),
        move |client, marker| async move {
            let marker = marker.expect("Cloudflare creates always use the ownership journal").nonce;
            let sandbox: Sandbox = client.json("sandboxes", &json!({
                "marker": marker, "image": environment,
                "cpu": resources.cpu_slots.max(1), "memoryBytes": resources.memory_bytes,
                "maxArchiveBytes": limits.max_archive_bytes, "maxCaptureBytes": limits.max_capture_bytes,
            })).await?;
            client::validate_id(&sandbox.id)?;
            if sandbox.id != marker {
                return Err(Error::RemoteProviderApi { provider: PROVIDER.into(), message: "bridge returned a sandbox with a different ownership marker".into() });
            }
            Ok(sandbox)
        }
    ))).await?;
    let result = deadline
        .within(Box::pin(phase::run(PROVIDER, "execute", async {
            client
                .upload(&sandbox.id, archive.path(), archive.len())
                .await?;
            drop(archive);
            drop(input_phase);
            run(
                &client,
                &sandbox,
                &command,
                workspace,
                cache,
                stream_to_parent,
                capture.as_ref(),
            )
            .await
        })))
        .await;
    let deleted = phase::run(PROVIDER, "cleanup", cleanup.run()).await;
    match (result, deleted) {
        (Ok(result), Ok(())) => Ok(result),
        (Ok(_), Err(error)) | (Err(error), Ok(())) => Err(error),
        (Err(error), Err(cleanup_error)) => {
            tracing::warn!(provider = PROVIDER, %cleanup_error, "sandbox cleanup failed; ownership journal retained");
            Err(error)
        }
    }
}

async fn run(
    client: &Client,
    sandbox: &Sandbox,
    command: &PreparedCommand<'_>,
    workspace: &Path,
    cache: &CacheProvider,
    stream_to_parent: bool,
    capture: Option<&CapturePermit>,
) -> Result<ActionResult> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Completed {
        exit_code: i32,
    }
    let root = Path::new("/workspace");
    let completed: Completed = client.json(&format!("sandboxes/{}/exec", sandbox.id), &json!({
        "argv": resolve_execution_argv(command.argv, root),
        "env": resolve_execution_env(command.env, root),
        "cwd": command.cwd.map_or_else(|| "/workspace".into(), |cwd| super::join_path("/workspace", cwd.as_str())),
    })).await?;
    let mut digests = Vec::new();
    let mut remaining = client.limits.max_capture_bytes;
    for (name, destination) in [
        ("stdout", Destination::Stdout),
        ("stderr", Destination::Stderr),
    ] {
        let file = output_archive_file(workspace)?;
        let permit = client
            .download(
                &format!("sandboxes/{}/{name}", sandbox.id),
                file.path(),
                // Capture capacity was reserved before provisioning, independently of transfers.
                None,
                1,
                remaining,
            )
            .await?;
        let input =
            tokio::fs::File::open(file.path())
                .await
                .map_err(|source| Error::FileAction {
                    action: "read_remote_capture",
                    path: ".struo/tmp".into(),
                    source,
                })?;
        let size = input
            .metadata()
            .await
            .map_err(|source| Error::FileAction {
                action: "measure_remote_capture",
                path: ".struo/tmp".into(),
                source,
            })?
            .len();
        if let Some(capture) = capture {
            capture.reserve(size)?;
        }
        remaining = remaining.saturating_sub(size);
        digests.push(stream::to_cache(input, destination, cache, stream_to_parent).await?);
        drop(permit);
    }
    if command.accepts_exit_code(completed.exit_code) && !command.outputs.is_empty() {
        let _: serde_json::Value = client
            .json(
                &format!("sandboxes/{}/outputs", sandbox.id),
                &json!({ "paths": command.outputs.iter().map(crate::WorkspacePath::as_str).collect::<Vec<_>>() }),
            )
            .await?;
        let file = output_archive_file(workspace)?;
        let permit = client
            .download(
                &format!("sandboxes/{}/outputs", sandbox.id),
                file.path(),
                command.lease.as_ref(),
                2,
                client.limits.max_archive_bytes,
            )
            .await?;
        install_output_archive_limited(
            file.path(),
            workspace,
            command.outputs,
            PROVIDER,
            client.limits,
            permit,
        )
        .await?;
    }
    Ok(ActionResult {
        exit_code: completed.exit_code,
        stdout: Some(digests[0]),
        stderr: Some(digests[1]),
        outputs: BTreeMap::new(),
    })
}

#[cfg(all(test, any(target_os = "linux", target_vendor = "apple")))]
mod tests;
