use std::path::Path;
use std::time::Duration;

use struo_cas::{ActionResult, CacheProvider};

use super::{client::Client, preparation, service};
use crate::remote::lifecycle::Provider;
use crate::remote::worker_pool::Pool;
use crate::remote::{phase::Deadline, CommandFilesystemSetup, PreparedCommand};
use crate::{ExecutionPermit, Result, WorkerPoolSpec, Xdg};

mod lease;
use lease::Worker;

pub(super) async fn execute(
    spec: WorkerPoolSpec,
    command: PreparedCommand<'_>,
    workspace: &Path,
    cache: &CacheProvider,
    stream: bool,
    filesystem: CommandFilesystemSetup<'_>,
) -> Result<ActionResult> {
    let mut client = Client::from_env()?;
    client.limits = command
        .lease
        .as_ref()
        .map_or_else(crate::RemoteIoLimits::default, ExecutionPermit::io_limits);
    let canonical = workspace
        .canonicalize()
        .map_err(|source| crate::Error::FileAction {
            action: "worker_pool_workspace",
            path: workspace.display().to_string(),
            source,
        })?;
    let scope = serde_json::to_vec(&(client.scope(), client.credential(), canonical)).unwrap();
    let root = Xdg::from_env()
        .struo_data()
        .join("workers")
        .join("sprites")
        .join(blake3::hash(&scope).to_hex().as_str());
    let pool = Pool::new(&root, spec.max_workers)?;
    let binary = service::bootstrap::binary().await?;
    Box::pin(execute_in_pool(
        &client, &pool, spec, command, workspace, cache, stream, filesystem, binary,
    ))
    .await
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn execute_in_pool(
    client: &Client,
    pool: &Pool,
    spec: WorkerPoolSpec,
    command: PreparedCommand<'_>,
    workspace: &Path,
    cache: &CacheProvider,
    stream: bool,
    filesystem: CommandFilesystemSetup<'_>,
    binary: &service::bootstrap::Binary,
) -> Result<ActionResult> {
    let fingerprint = spec.fingerprint();
    let mut deadline = Deadline::new(command.timeout_ms);
    let capture = command.lease.as_ref().map(ExecutionPermit::capture_budget);
    if let Some(capture) = &capture {
        deadline.within(capture.reserve_upfront()).await?;
    }
    let slot = deadline
        .within(async {
            loop {
                if let Some(slot) = pool.try_claim(&fingerprint)? {
                    break Ok(slot);
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await?;
    let mut worker = Worker {
        client: client.clone(),
        slot: Some(slot),
        permit: command.lease.clone(),
    };
    let result = deadline.within(Box::pin(async {
        worker.acquire(&spec, &fingerprint).await?;
        if worker.record().checkpoint.is_none() {
            let sprite = worker.sprite()?;
            preparation::prepare(client, &sprite, &spec, cache, capture.as_ref()).await?;
        }
        let sprite = worker.sprite()?;
        let binding = worker.record().service.as_deref()
            .map(serde_json::from_str::<service::Binding>).transpose()
            .map_err(|_| super::client::api_error("invalid retained worker service binding"))?;
        let restart = binding.is_some();
        let binding = if let Some(binding) = binding.filter(|binding| binding.binary == binary.digest) {
            binding
        } else {
                let binding = crate::remote::phase::run("sprites", "worker_service_install",
                    service::install(client, &sprite, &fingerprint, binary, cache, capture.as_ref(), command.lease.as_ref(), restart)).await?;
                // Recovery restores a ready service, never a half-installed executable.
                let checkpoint = client.checkpoint(&sprite, &worker.record().nonce).await?;
                let record = worker.slot.as_mut().unwrap().record_mut().unwrap();
                record.service = Some(serde_json::to_string(&binding).unwrap());
                record.checkpoint = Some(checkpoint);
                worker.slot.as_ref().unwrap().save()?;
                binding
        };
        tracing::debug!(provider = "sprites", worker = %sprite.name, environment = %fingerprint, "leased prepared worker");
        crate::remote::phase::run("sprites", "worker_action",
            service::Service::new(&binding, client.limits)?.execute(&fingerprint, &command, workspace, cache, stream, filesystem)).await
    })).await;
    let cleanup = if result.is_ok() {
        worker.release()
    } else {
        worker.finish().await
    };
    match (result, cleanup) {
        (Ok(result), Ok(())) => Ok(result),
        (Err(error), Err(cleanup_error)) => {
            tracing::warn!(provider = "sprites", %cleanup_error, "worker reset failed after action failure");
            Err(error)
        }
        (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
    }
}
