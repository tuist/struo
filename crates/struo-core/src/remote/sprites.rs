use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use struo_cas::{ActionResult, CacheProvider};

use super::archive::create_input_archive_limited;
use super::lifecycle::{self, Budget, Journal};
use super::{CommandFilesystemSetup, PreparedCommand};
use crate::executor::staging::CapturePermit;
use crate::{ExecutionPermit, RemoteExecution, Result};

mod action;
mod checkpoint;
mod client;
mod exec;
mod filesystem;
mod preparation;
mod service;
#[cfg(test)]
mod tests;
mod tools;
mod transfer;
mod transport;
mod workers;

use client::{api_error, Client, Sprite};

const GUEST_ROOT: &str = "/home/sprite/struo";
const INPUT_ARCHIVE: &str = "/tmp/struo-inputs.tar";
const OUTPUT_ARCHIVE: &str = "/tmp/struo-outputs.tar";

pub(super) async fn execute_command(
    remote: &RemoteExecution,
    command: PreparedCommand<'_>,
    workspace_root: &Path,
    cache: &CacheProvider,
    stream_to_parent: bool,
    filesystem: CommandFilesystemSetup<'_>,
) -> Result<ActionResult> {
    if let Some(value) = remote.properties.get("worker_pool") {
        let spec: crate::WorkerPoolSpec = serde_json::from_value(value.clone())
            .map_err(|_| api_error("invalid locked worker pool configuration"))?;
        spec.validate()?;
        return workers::execute(
            spec,
            command,
            workspace_root,
            cache,
            stream_to_parent,
            filesystem,
        )
        .await;
    }
    execute_with_client(
        command,
        workspace_root,
        cache,
        stream_to_parent,
        Client::from_env()?,
        filesystem,
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
    filesystem: CommandFilesystemSetup<'_>,
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
        .within(action::run(
            &client,
            &sprite,
            &command,
            workspace_root,
            cache,
            stream_to_parent,
            archive,
            input_phase,
            capture.as_ref(),
            false,
            filesystem,
        ))
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
