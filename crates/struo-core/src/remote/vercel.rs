use std::collections::BTreeMap;
use std::path::Path;

use struo_cas::{ActionResult, CacheProvider};

use super::archive::create_input_archive_limited;
use super::PreparedCommand;
use crate::{ExecutionPermit, RemoteExecution, Result};

mod capture;
mod client;
mod lifecycle;
mod outputs;
#[cfg(test)]
mod tests;
mod transport;

use client::{Client, Config};

const ROOT: &str = "/vercel/sandbox/struo";

pub(super) async fn execute_command(
    remote: &RemoteExecution,
    command: PreparedCommand<'_>,
    workspace: &Path,
    cache: &CacheProvider,
    stream: bool,
) -> Result<ActionResult> {
    let client = Client::new(Config::from_env(remote)?)?;
    execute(client, remote, command, workspace, cache, stream).await
}

async fn execute(
    mut client: Client,
    remote: &RemoteExecution,
    command: PreparedCommand<'_>,
    workspace: &Path,
    cache: &CacheProvider,
    stream: bool,
) -> Result<ActionResult> {
    client.limits = command
        .lease
        .as_ref()
        .map_or_else(crate::RemoteIoLimits::default, ExecutionPermit::io_limits);
    if client.limits.command_idle_timeout.is_some() {
        return Err(crate::Executor::Vercel.unsupported("command output-idle timeout"));
    }
    let timeout = command.timeout_ms.unwrap_or(600_000);
    let lifetime = timeout
        .checked_add(120_000)
        .filter(|value| *value <= 86_400_000)
        .ok_or_else(|| {
            client::api_error("session lifetime exceeds 24 hours including cleanup grace")
        })?;
    let mut deadline = super::phase::Deadline::new(Some(timeout));
    let (archive, input_phase) = deadline
        .within(create_input_archive_limited(
            workspace,
            command.inputs,
            "vercel",
            client.limits,
            command.lease.as_ref(),
        ))
        .await?;
    let capture_phase = command.lease.as_ref().map(ExecutionPermit::capture_budget);
    if let Some(capture) = &capture_phase {
        deadline.within(capture.reserve_upfront()).await?;
    }
    let journal = super::lifecycle::Journal::new(workspace);
    deadline
        .within(async {
            super::lifecycle::reconcile(&journal, &client, super::lifecycle::Budget::default())
                .await;
            Ok(())
        })
        .await?;
    let (sandbox, cleanup) = deadline
        .within(
            client.create_owned(
                journal,
                remote
                    .environment
                    .clone()
                    .unwrap_or_else(|| "node24".into()),
                command.resources.clone(),
                lifetime,
                command.lease.clone(),
            ),
        )
        .await?;
    let result = deadline
        .within(Box::pin(async {
            client.ready(&sandbox.session.id).await?;
            ensure_success(
                client
                    .command(
                        &sandbox.session.id,
                        &["mkdir".into(), "-p".into(), ROOT.into()],
                        "/vercel/sandbox",
                        &BTreeMap::new(),
                    )
                    .await?,
                "creating workspace",
            )?;
            client
                .upload(&sandbox.session.id, archive, input_phase)
                .await?;
            let result = Box::pin(capture::capture(
                &client,
                &sandbox.session.id,
                &command,
                workspace,
                cache,
                stream,
            ))
            .await?;
            if command.accepts_exit_code(result.exit_code) && !command.outputs.is_empty() {
                Box::pin(outputs::restore(
                    &client,
                    &sandbox.session.id,
                    &command,
                    workspace,
                ))
                .await?;
            }
            Ok(result)
        }))
        .await;
    drop(capture_phase);
    let cleaned = cleanup.run().await;
    match (result, cleaned) {
        (Ok(result), Ok(())) => Ok(result),
        (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
        (Err(error), Err(cleanup_error)) => {
            tracing::warn!(provider = "vercel", %cleanup_error, "sandbox deletion failed after action failure");
            Err(error)
        }
    }
}

fn ensure_success(code: i32, operation: &str) -> Result<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(client::api_error(format!(
            "{operation} failed with exit code {code}"
        )))
    }
}
