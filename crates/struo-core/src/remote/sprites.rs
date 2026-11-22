use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use struo_cas::{ActionResult, CacheProvider};

use super::archive::{
    create_input_archive_limited, install_output_archive_limited, output_archive_file,
};
use super::lifecycle::{self, Budget, Journal};
use super::{join_path, PreparedCommand};
use crate::executor::staging::CapturePermit;
use crate::{resolve_execution_argv, resolve_execution_env};
use crate::{ExecutionPermit, RemoteExecution, Result};

mod client;
mod exec;
#[cfg(test)]
mod tests;
mod transfer;
mod transport;

use client::{api_error, Client, Sprite};

const GUEST_ROOT: &str = "/home/sprite/struo";
const INPUT_ARCHIVE: &str = "/tmp/struo-inputs.tar";
const OUTPUT_ARCHIVE: &str = "/tmp/struo-outputs.tar";

pub(super) async fn execute_command(
    _remote: &RemoteExecution,
    command: PreparedCommand<'_>,
    workspace_root: &Path,
    cache: &CacheProvider,
    stream_to_parent: bool,
) -> Result<ActionResult> {
    execute_with_client(
        command,
        workspace_root,
        cache,
        stream_to_parent,
        Client::from_env()?,
    )
    .await
}

#[allow(clippy::too_many_lines)]
async fn execute_with_client(
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
    let mut deadline = super::phase::Deadline::new(command.timeout_ms);
    let (archive, input_phase) = deadline
        .within(create_input_archive_limited(
            workspace_root,
            command.inputs,
            "sprites",
            client.limits,
            command.lease.as_ref(),
        ))
        .await?;
    let capture = command.lease.as_ref().map(ExecutionPermit::capture_budget);
    if let Some(capture) = &capture {
        deadline.within(capture.reserve_upfront()).await?;
    }
    let journal = Journal::new(workspace_root);
    deadline
        .within(async { Ok(lifecycle::reconcile(&journal, &client, Budget::default()).await) })
        .await?;
    let (sprite, cleanup) = deadline
        .within(lifecycle::create_owned(
            client.clone(),
            Some(journal),
            Duration::from_mins(10),
            command.lease.clone(),
            |client, marker| async move {
                let marker =
                    marker.ok_or_else(|| api_error("Sprite creation requires a journaled name"))?;
                client.create(&marker.nonce).await
            },
        ))
        .await?;
    let result = deadline
        .within(async {
            client
                .upload(
                    &sprite,
                    archive.path(),
                    archive.len(),
                    cache,
                    capture.as_ref(),
                )
                .await?;
            drop(archive);
            drop(input_phase);
            setup(
                &client,
                &sprite,
                &["mkdir".into(), "-p".into(), GUEST_ROOT.into()],
                cache,
                capture.as_ref(),
            )
            .await?;
            setup(
                &client,
                &sprite,
                &[
                    "tar".into(),
                    "-xf".into(),
                    INPUT_ARCHIVE.into(),
                    "-C".into(),
                    GUEST_ROOT.into(),
                ],
                cache,
                capture.as_ref(),
            )
            .await?;
            let argv = resolve_execution_argv(command.argv, Path::new(GUEST_ROOT));
            let env = resolve_execution_env(command.env, Path::new(GUEST_ROOT));
            let cwd = command.cwd.map_or_else(
                || GUEST_ROOT.to_string(),
                |cwd| join_path(GUEST_ROOT, cwd.as_str()),
            );
            let result = super::phase::run(
                "sprites",
                "execute",
                Box::pin(client.execute(
                    &sprite,
                    &argv,
                    Some(&cwd),
                    &env,
                    cache,
                    stream_to_parent,
                    capture.as_ref(),
                )),
            )
            .await?;
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
                setup(&client, &sprite, &argv, cache, capture.as_ref()).await?;
                let archive = output_archive_file(workspace_root)?;
                let phase = client
                    .download(&sprite, archive.path(), command.lease.as_ref())
                    .await?;
                install_output_archive_limited(
                    archive.path(),
                    workspace_root,
                    command.outputs,
                    "sprites",
                    client.limits,
                    phase,
                )
                .await?;
            }
            Ok(result)
        })
        .await;
    let deleted = super::phase::run("sprites", "cleanup", cleanup.run()).await;
    match (result, deleted) {
        (Ok(result), Ok(())) => Ok(result),
        (Err(error), Err(cleanup_error)) => {
            tracing::warn!(provider = "sprites", %cleanup_error, "failed to delete Sprite after execution failure");
            Err(error)
        }
        (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
    }
}

async fn setup(
    client: &Client,
    sprite: &Sprite,
    argv: &[String],
    cache: &CacheProvider,
    budget: Option<&CapturePermit>,
) -> Result<()> {
    let result =
        Box::pin(client.execute(sprite, argv, None, &BTreeMap::new(), cache, false, budget))
            .await?;
    if result.exit_code != 0 {
        return Err(api_error("remote filesystem setup failed"));
    }
    Ok(())
}
