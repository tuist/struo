use std::path::Path;

use struo_cas::{ActionResult, CacheProvider};

use super::archive::create_input_archive_limited;
use super::PreparedCommand;
use crate::{Error, ExecutionPermit, RemoteExecution, Result};

mod capture;
mod client;
mod endpoint;
mod run;
#[cfg(test)]
mod tests;
mod transport;

use client::Client;

const PROVIDER: &str = "opensandbox";
const ROOT: &str = "/workspace";
const INPUT: &str = "/tmp/struo-inputs.tar";
const OUTPUT: &str = "/tmp/struo-outputs.tar";

pub(super) async fn execute_command(
    remote: &RemoteExecution,
    command: PreparedCommand<'_>,
    workspace: &Path,
    cache: &CacheProvider,
    stream: bool,
) -> Result<ActionResult> {
    Box::pin(execute_with_client(
        remote,
        command,
        workspace,
        cache,
        stream,
        Client::from_env()?,
    ))
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
    let image = remote
        .environment
        .clone()
        .filter(|image| !image.trim().is_empty())
        .ok_or_else(|| Error::RemoteProviderConfig {
            provider: PROVIDER.into(),
            message: "declare an image in an opensandbox infrastructure".into(),
        })?;
    client.secure_access = match remote.properties.get("secure_access") {
        None => true,
        Some(value) => value.as_bool().ok_or_else(|| Error::RemoteProviderConfig {
            provider: PROVIDER.into(),
            message: "execution property secure_access must be a boolean".into(),
        })?,
    };
    client.limits = command
        .lease
        .as_ref()
        .map_or_else(crate::RemoteIoLimits::default, ExecutionPermit::io_limits);
    let mut deadline = super::phase::Deadline::new(Some(command.timeout_ms.unwrap_or(900_000)));
    let (archive, input_phase) = deadline
        .within(super::phase::run(
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
    let journal = super::lifecycle::Journal::new(workspace);
    deadline
        .within(super::phase::run(PROVIDER, "reconcile", async {
            Ok(
                super::lifecycle::reconcile(&journal, &client, super::lifecycle::Budget::default())
                    .await,
            )
        }))
        .await?;
    let resources = command.resources.clone();
    let timeout = command.timeout_ms;
    let (sandbox, cleanup) = deadline
        .within(super::phase::run(
            PROVIDER,
            "create",
            super::lifecycle::create_owned(
                client.clone(),
                Some(journal),
                client::lifetime(timeout).saturating_add(std::time::Duration::from_mins(10)),
                command.lease.clone(),
                move |client, marker| async move {
                    client
                        .create(&image, &resources, timeout, marker.as_ref())
                        .await
                },
            ),
        ))
        .await?;
    let result = deadline
        .within(Box::pin(run::execute(
            &client,
            &sandbox,
            &command,
            workspace,
            cache,
            stream,
            archive,
            input_phase,
        )))
        .await;
    let deleted = super::phase::run(PROVIDER, "cleanup", cleanup.run()).await;
    match (result, deleted) {
        (Ok(result), Ok(())) => Ok(result),
        (Ok(_), Err(error)) | (Err(error), Ok(())) => Err(error),
        (Err(error), Err(cleanup)) => {
            tracing::warn!(provider = PROVIDER, %cleanup, "failed to delete sandbox after execution failure");
            Err(error)
        }
    }
}

fn ensure_success(code: i32, operation: &str) -> Result<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(api_error(&format!(
            "{operation} failed with exit code {code}"
        )))
    }
}

fn word(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn api_error(message: &str) -> Error {
    Error::RemoteProviderApi {
        provider: PROVIDER.into(),
        message: message.into(),
    }
}
