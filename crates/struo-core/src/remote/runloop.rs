use std::path::Path;

use struo_cas::{ActionResult, CacheProvider};

use super::archive::create_input_archive_limited;
use super::PreparedCommand;
use crate::{ExecutionPermit, RemoteExecution, Result};

mod capture;
mod cleanup;
mod client;
mod command;
mod execution;
mod ownership;
#[cfg(all(test, any(target_os = "linux", target_vendor = "apple")))]
mod tests;
mod transport;

use client::{Client, Config};
use command::{command_line, pack_outputs_command, shell_word, workdir};
const GUEST_ROOT: &str = "/home/user/struo-workspace";
const INPUT_ARCHIVE: &str = "/home/user/struo-inputs.tar";
const OUTPUT_ARCHIVE: &str = "/home/user/struo-outputs.tar";

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
        Client::new(Config::from_env()?)?,
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
    if client.limits.command_idle_timeout.is_some() {
        return Err(crate::Executor::Runloop.unsupported("command output idle timeout"));
    }
    let mut deadline = super::phase::Deadline::new(command.timeout_ms.or(Some(600_000)));
    let (archive, input_phase) = deadline
        .within(create_input_archive_limited(
            workspace_root,
            command.inputs,
            "runloop",
            client.limits,
            command.lease.as_ref(),
        ))
        .await?;
    let capture_phase = if let Some(lease) = command.lease.as_ref() {
        let capture = lease.capture_budget();
        deadline.within(capture.reserve_upfront()).await?;
        Some(capture)
    } else {
        None
    };
    let journal = super::lifecycle::Journal::new(workspace_root);
    deadline
        .within(async {
            client.reconcile(&journal).await;
            Ok(())
        })
        .await?;
    let (sandbox, cleanup) = deadline
        .within(Box::pin(client.create_owned(
            journal,
            remote.environment.clone(),
            command.timeout_ms,
            command.lease.clone(),
        )))
        .await?;
    let result = deadline
        .within(Box::pin(execution::run(
            &client,
            sandbox,
            &command,
            workspace_root,
            cache,
            stream_to_parent,
            archive,
            input_phase,
            capture_phase,
        )))
        .await;
    let cleaned = super::phase::run("runloop", "cleanup", cleanup.run()).await;
    match (result, cleaned) {
        (Ok(result), Ok(())) => Ok(result),
        (Ok(_), Err(error)) | (Err(error), Ok(())) => Err(error),
        (Err(error), Err(cleanup_error)) => {
            tracing::warn!(provider = "runloop", %cleanup_error, "failed to shut down devbox after execution failure");
            Err(error)
        }
    }
}

fn ensure_success(response: &transport::ExecuteResponse, operation: &str) -> Result<()> {
    let exit_code = response.exit_code()?;
    if exit_code == 0 {
        return Ok(());
    }
    Err(client::api_error(format!(
        "{operation} failed with exit code {exit_code}"
    )))
}
