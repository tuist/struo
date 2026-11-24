use std::path::Path;

use struo_cas::{ActionResult, CacheProvider};

use super::archive::create_input_archive_limited;
use super::PreparedCommand;
use crate::{ExecutionPermit, RemoteExecution, Result};

mod capture;
mod client;
mod control;
mod execution;
mod ownership;
#[cfg(test)]
mod tests;
mod transport;
mod work;

use client::{Client, Config};

const GUEST_ROOT: &str = "/workspace/struo";
const INPUT_ARCHIVE: &str = "/tmp/struo-inputs.tar";
const OUTPUT_ARCHIVE: &str = "/tmp/struo-outputs.tar";

pub(super) async fn execute_command(
    remote: &RemoteExecution,
    command: PreparedCommand<'_>,
    workspace_root: &Path,
    cache: &CacheProvider,
    stream_to_parent: bool,
) -> Result<ActionResult> {
    execute_with_client(
        remote,
        command,
        workspace_root,
        cache,
        stream_to_parent,
        Client::new(Config::from_env(remote)?)?,
    )
    .await
}

async fn execute_with_client(
    remote: &RemoteExecution,
    command: PreparedCommand<'_>,
    workspace_root: &Path,
    cache: &CacheProvider,
    stream_to_parent: bool,
    mut client: Client,
) -> Result<ActionResult> {
    client.limits = command
        .lease
        .as_ref()
        .map_or_else(crate::RemoteIoLimits::default, ExecutionPermit::io_limits);
    let mut deadline = super::phase::Deadline::new(Some(command.timeout_ms.unwrap_or(600_000)));
    let (archive, input_phase) = deadline
        .within(super::phase::run(
            "blaxel",
            "prepare",
            create_input_archive_limited(
                workspace_root,
                command.inputs,
                "blaxel",
                client.limits,
                command.lease.as_ref(),
            ),
        ))
        .await?;
    let capture_phase = deadline
        .within(async {
            match command.lease.as_ref() {
                Some(lease) => {
                    let capture = lease.capture_budget();
                    capture.reserve_upfront().await?;
                    Ok(Some(capture))
                }
                None => Ok(None),
            }
        })
        .await?;
    let journal = super::lifecycle::Journal::new(workspace_root);
    deadline
        .within(super::phase::run("blaxel", "reconcile", async {
            Ok(client.reconcile(&journal).await)
        }))
        .await?;
    let (sandbox, cleanup) = deadline
        .within(super::phase::run(
            "blaxel",
            "create",
            client.create_owned(
                journal,
                remote.environment.clone(),
                command.resources.clone(),
                command.timeout_ms,
                command.lease.clone(),
            ),
        ))
        .await?;
    let result = deadline
        .within(Box::pin(work::run(
            &client,
            sandbox,
            &command,
            workspace_root,
            cache,
            stream_to_parent,
            (archive, input_phase, capture_phase),
        )))
        .await;
    let cleanup_result = super::phase::run("blaxel", "cleanup", cleanup.run()).await;
    match (result, cleanup_result) {
        (Ok(result), Ok(())) => Ok(result),
        (Ok(_), Err(error)) | (Err(error), Ok(())) => Err(error),
        (Err(error), Err(cleanup_error)) => {
            tracing::warn!(provider = "blaxel", %cleanup_error, "failed to delete remote sandbox after execution failure");
            Err(error)
        }
    }
}
