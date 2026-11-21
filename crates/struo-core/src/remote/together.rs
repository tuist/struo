use std::collections::BTreeMap;
use std::path::Path;

use struo_cas::{ActionResult, CacheProvider};

use super::archive::create_input_archive_limited;
use super::PreparedCommand;
use crate::{ExecutionPermit, RemoteExecution, Result};

mod client;
mod exec;
mod lifecycle;
#[cfg(test)]
mod tests;
mod transport;

use client::Client;

const ROOT: &str = "/tmp/struo-workspace";
const INPUT: &str = "/tmp/struo-inputs.tar";
const OUTPUT: &str = "/tmp/struo-outputs.tar";

pub(super) async fn execute_command(
    remote: &RemoteExecution,
    command: PreparedCommand<'_>,
    workspace: &Path,
    cache: &CacheProvider,
    stream: bool,
) -> Result<ActionResult> {
    super::phase::run(
        "together",
        "action",
        Box::pin(execute_with_client(
            remote,
            command,
            workspace,
            cache,
            stream,
            Client::from_env()?,
        )),
    )
    .await
}

async fn execute_with_client(
    remote: &RemoteExecution,
    command: PreparedCommand<'_>,
    workspace: &Path,
    cache: &CacheProvider,
    stream: bool,
    mut client: Client,
) -> Result<ActionResult> {
    let snapshot = remote
        .environment
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            client::config_error("declare an explicit Together snapshot ID in environment")
        })?;
    client.limits = command
        .lease
        .as_ref()
        .map_or_else(crate::RemoteIoLimits::default, ExecutionPermit::io_limits);
    if client.limits.command_idle_timeout.is_some() {
        return Err(crate::Executor::Together.unsupported("command output idle timeout"));
    }
    let mut deadline = super::phase::Deadline::new(command.timeout_ms);
    let (archive, input_phase, capture) = deadline
        .within(async {
            let (archive, phase) = create_input_archive_limited(
                workspace,
                command.inputs,
                "together",
                client.limits,
                command.lease.as_ref(),
            )
            .await?;
            let capture = command.lease.as_ref().map(ExecutionPermit::capture_budget);
            if let Some(capture) = &capture {
                capture.reserve_upfront().await?;
            }
            Ok((archive, phase, capture))
        })
        .await?;
    let journal = super::lifecycle::Journal::new(workspace);
    deadline
        .within(async {
            client.reconcile(&journal).await;
            Ok(())
        })
        .await?;
    let (sandbox, cleanup) = deadline
        .within(client.create_owned(
            journal,
            snapshot.to_string(),
            command.timeout_ms,
            command.lease.clone(),
        ))
        .await?;
    let result = deadline
        .within(Box::pin(async {
            let sandbox = client.wait_running(sandbox).await?;
            client
                .upload(&sandbox, archive.path(), archive.len(), INPUT)
                .await?;
            drop(archive);
            drop(input_phase);
            client
                .setup(&sandbox, "rm", &["-rf".into(), "--".into(), ROOT.into()])
                .await?;
            client
                .setup(
                    &sandbox,
                    "mkdir",
                    &[
                        "-p".into(),
                        command.cwd.map_or_else(
                            || ROOT.into(),
                            |cwd| super::join_path(ROOT, cwd.as_str()),
                        ),
                    ],
                )
                .await?;
            client
                .setup(
                    &sandbox,
                    "tar",
                    &["-xf".into(), INPUT.into(), "-C".into(), ROOT.into()],
                )
                .await?;
            let result = client
                .capture(&sandbox, &command, workspace, cache, stream, capture)
                .await?;
            if command.accepts_exit_code(result.exit_code) && !command.outputs.is_empty() {
                client
                    .restore_outputs(&sandbox, &command, workspace)
                    .await?;
            }
            Ok(result)
        }))
        .await;
    let deleted = cleanup.run().await;
    match (result, deleted) {
        (Ok(result), Ok(())) => Ok(result),
        (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
        (Err(error), Err(cleanup_error)) => {
            tracing::warn!(provider = "together", %cleanup_error, "failed to terminate sandbox after execution failure");
            Err(error)
        }
    }
}

impl Client {
    async fn setup(&self, sandbox: &client::Sandbox, program: &str, args: &[String]) -> Result<()> {
        let code = self
            .exec(sandbox, program, args, "/tmp", &BTreeMap::new())
            .await?;
        if code != 0 {
            return Err(client::api_error(format!(
                "remote setup failed with exit code {code}"
            )));
        }
        Ok(())
    }
}
