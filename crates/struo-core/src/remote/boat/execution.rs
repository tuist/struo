use std::path::Path;

use struo_cas::{ActionResult, CacheProvider};

use super::super::archive::{
    create_input_archive_limited, install_output_archive_limited, output_archive_file,
};
use super::super::lifecycle::{self, Budget, Journal};
use super::super::phase;
use super::super::PreparedCommand;
use super::client::{self, Client};
use super::{capture, pack_outputs_command, shell_word, GUEST_ROOT, INPUT_ARCHIVE, OUTPUT_ARCHIVE};
use crate::{ExecutionPermit, RemoteExecution, Result};

pub(super) async fn execute_with_client(
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
        return Err(crate::Executor::Boat.unsupported("command output idle timeout"));
    }
    let ttl = client::lifetime(command.timeout_ms)?;
    let mut deadline = phase::Deadline::new(command.timeout_ms);
    let capture_budget = command.lease.as_ref().map(ExecutionPermit::capture_budget);
    if let Some(budget) = &capture_budget {
        deadline.within(budget.reserve_upfront()).await?;
    }
    let (archive, input_phase) = deadline
        .within(phase::run(
            "boat",
            "prepare",
            create_input_archive_limited(
                workspace_root,
                command.inputs,
                "boat",
                client.limits,
                command.lease.as_ref(),
            ),
        ))
        .await?;
    let journal = Journal::new(workspace_root);
    deadline
        .within(phase::run("boat", "reconcile", async {
            Ok(lifecycle::reconcile(&journal, &client, Budget::default()).await)
        }))
        .await?;
    let snapshot = remote.environment.clone();
    let (sandbox, cleanup) = deadline
        .within(phase::run(
            "boat",
            "create",
            lifecycle::create_owned(
                client.clone(),
                Some(journal),
                ttl.saturating_add(std::time::Duration::from_mins(10)),
                command.lease.clone(),
                move |client, marker| async move {
                    client
                        .create(snapshot.as_deref(), ttl, marker.as_ref())
                        .await
                },
            ),
        ))
        .await?;
    let result = deadline
        .within(Box::pin(run_in_sandbox(
            &client,
            &sandbox,
            &command,
            workspace_root,
            cache,
            stream_to_parent,
            (archive, input_phase),
        )))
        .await;
    let cleaned = phase::run("boat", "cleanup", cleanup.run()).await;
    match (result, cleaned) {
        (Ok(result), Ok(())) => Ok(result),
        (Ok(_), Err(error)) | (Err(error), Ok(())) => Err(error),
        (Err(error), Err(cleanup_error)) => {
            tracing::warn!(provider = "boat", %cleanup_error, "failed to delete remote sandbox after execution failure");
            Err(error)
        }
    }
}

async fn run_in_sandbox(
    client: &Client,
    sandbox: &super::client::Sandbox,
    command: &PreparedCommand<'_>,
    workspace_root: &Path,
    cache: &CacheProvider,
    stream_to_parent: bool,
    (archive, input_phase): (
        super::super::archive::TempArchive,
        super::super::PhasePermit,
    ),
) -> Result<ActionResult> {
    phase::run("boat", "startup", client.wait_started(sandbox)).await?;
    phase::run(
        "boat",
        "upload",
        client.upload(sandbox, archive.path(), archive.len(), INPUT_ARCHIVE),
    )
    .await?;
    drop(archive);
    drop(input_phase);
    client
        .execute(
            sandbox,
            &format!(
                "rm -rf -- {} && mkdir -p -- {} && tar -xf {} -C {}",
                shell_word(GUEST_ROOT),
                shell_word(GUEST_ROOT),
                shell_word(INPUT_ARCHIVE),
                shell_word(GUEST_ROOT)
            ),
            None,
        )
        .await?
        .ensure_success("staging declared inputs")?;
    let result = phase::run(
        "boat",
        "execute",
        Box::pin(capture::execute(
            client,
            sandbox,
            command,
            workspace_root,
            cache,
            stream_to_parent,
        )),
    )
    .await?;
    if command.accepts_exit_code(result.exit_code) && !command.outputs.is_empty() {
        client
            .execute(sandbox, &pack_outputs_command(command.outputs), None)
            .await?
            .ensure_success("packing declared outputs")?;
        let archive = output_archive_file(workspace_root)?;
        let phase = client
            .download(
                sandbox,
                OUTPUT_ARCHIVE,
                archive.path(),
                command.lease.as_ref(),
                2,
            )
            .await?;
        install_output_archive_limited(
            archive.path(),
            workspace_root,
            command.outputs,
            "boat",
            client.limits,
            phase,
        )
        .await?;
    }
    Ok(result)
}
