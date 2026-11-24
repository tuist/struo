use std::path::Path;
use std::time::Duration;

use struo_cas::{ActionResult, CacheProvider};

use super::client::{api_error, Client};
use super::{capture, GUEST_ROOT, INPUT_ARCHIVE, OUTPUT_ARCHIVE};
use crate::remote::archive::{
    create_input_archive_limited, install_output_archive_limited, output_archive_file,
};
use crate::remote::lifecycle::{self, Budget, Journal};
use crate::remote::{phase, PreparedCommand};
use crate::{ExecutionPermit, RemoteExecution, Result};

#[allow(clippy::too_many_lines)]
pub(super) async fn execute_with_client(
    remote: &RemoteExecution,
    command: PreparedCommand<'_>,
    workspace: &Path,
    cache: &CacheProvider,
    stream_to_parent: bool,
    mut client: Client,
) -> Result<ActionResult> {
    client.limits = command
        .lease
        .as_ref()
        .map_or_else(crate::RemoteIoLimits::default, ExecutionPermit::io_limits);
    if client.limits.command_idle_timeout.is_some() {
        return Err(crate::Executor::Tensorlake.unsupported("command output idle timeout"));
    }
    let mut deadline = phase::Deadline::new(Some(command.timeout_ms.unwrap_or(600_000)));
    let capture_budget = command.lease.as_ref().map(ExecutionPermit::capture_budget);
    if let Some(budget) = &capture_budget {
        deadline.within(budget.reserve_upfront()).await?;
    }
    let (archive, input_phase) = deadline
        .within(create_input_archive_limited(
            workspace,
            command.inputs,
            "tensorlake",
            client.limits,
            command.lease.as_ref(),
        ))
        .await?;
    let journal = Journal::new(workspace);
    deadline
        .within(async { Ok(lifecycle::reconcile(&journal, &client, Budget::default()).await) })
        .await?;
    let binding = remote.clone();
    let (sandbox, cleanup) = deadline
        .within(lifecycle::create_owned(
            client.clone(),
            Some(journal),
            Duration::from_mins(30),
            command.lease.clone(),
            move |client, marker| async move {
                let marker =
                    marker.ok_or_else(|| api_error("creation requires journaled intent"))?;
                client.create(&binding, &marker.nonce).await
            },
        ))
        .await?;
    let result = deadline
        .within(async {
            let origin = phase::run("tensorlake", "startup", client.ready(&sandbox)).await?;
            phase::run(
                "tensorlake",
                "upload",
                client.upload(
                    &sandbox,
                    &origin,
                    archive.path(),
                    archive.len(),
                    INPUT_ARCHIVE,
                ),
            )
            .await?;
            drop(archive);
            drop(input_phase);
            client
                .setup(
                    &sandbox,
                    &origin,
                    &["rm".into(), "-rf".into(), "--".into(), GUEST_ROOT.into()],
                )
                .await?;
            client
                .setup(
                    &sandbox,
                    &origin,
                    &["mkdir".into(), "-p".into(), GUEST_ROOT.into()],
                )
                .await?;
            client
                .setup(
                    &sandbox,
                    &origin,
                    &[
                        "tar".into(),
                        "-xf".into(),
                        INPUT_ARCHIVE.into(),
                        "-C".into(),
                        GUEST_ROOT.into(),
                    ],
                )
                .await?;
            let result = phase::run(
                "tensorlake",
                "execute",
                Box::pin(capture::execute(
                    &client,
                    &sandbox,
                    &origin,
                    &command,
                    workspace,
                    cache,
                    stream_to_parent,
                )),
            )
            .await?;
            drop(capture_budget);
            if command.accepts_exit_code(result.exit_code) && !command.outputs.is_empty() {
                let mut argv = vec![
                    "tar".into(),
                    "--hard-dereference".into(),
                    "-cf".into(),
                    OUTPUT_ARCHIVE.into(),
                    "-C".into(),
                    GUEST_ROOT.into(),
                ];
                argv.extend(
                    command
                        .outputs
                        .iter()
                        .map(|path| format!("./{}", path.as_str())),
                );
                client.setup(&sandbox, &origin, &argv).await?;
                let archive = output_archive_file(workspace)?;
                let phase = client
                    .download(
                        &sandbox,
                        &origin,
                        OUTPUT_ARCHIVE,
                        archive.path(),
                        command.lease.as_ref(),
                        2,
                    )
                    .await?;
                install_output_archive_limited(
                    archive.path(),
                    workspace,
                    command.outputs,
                    "tensorlake",
                    client.limits,
                    phase,
                )
                .await?;
            }
            Ok(result)
        })
        .await;
    let deleted = phase::run("tensorlake", "cleanup", cleanup.run()).await;
    match (result, deleted) {
        (Ok(result), Ok(())) => Ok(result),
        (Err(error), Err(cleanup_error)) => {
            tracing::warn!(provider = "tensorlake", %cleanup_error, "termination failed after execution failure");
            Err(error)
        }
        (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
    }
}
