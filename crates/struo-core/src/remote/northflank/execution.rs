use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use struo_cas::{ActionResult, CacheProvider};

use crate::remote::archive::{
    create_input_archive_limited, install_output_archive_limited, output_archive_file,
};
use crate::remote::lifecycle::{self, Budget, Journal};
use crate::remote::{phase, PreparedCommand};
use crate::stream::{self, Destination};
use crate::{
    resolve_execution_argv, resolve_execution_env, ExecutionPermit, RemoteExecution, Result,
};

use super::client::{api_error, config_error, Client};

const GUEST_ROOT: &str = "/workspace";
const DEFAULT_TIMEOUT_MS: u64 = 600_000;

pub(in crate::remote) async fn execute_command(
    remote: &RemoteExecution,
    command: PreparedCommand<'_>,
    workspace_root: &Path,
    cache: &CacheProvider,
    stream_to_parent: bool,
) -> Result<ActionResult> {
    let client = Client::from_binding(remote)?;
    Box::pin(execute_with_client(
        remote,
        command,
        workspace_root,
        cache,
        stream_to_parent,
        client,
    ))
    .await
}

#[allow(clippy::too_many_lines)]
pub(super) async fn execute_with_client(
    remote: &RemoteExecution,
    command: PreparedCommand<'_>,
    workspace_root: &Path,
    cache: &CacheProvider,
    stream_to_parent: bool,
    mut client: Client,
) -> Result<ActionResult> {
    let image = remote
        .environment
        .clone()
        .filter(|image| !image.trim().is_empty())
        .ok_or_else(|| config_error("Northflank requires an explicit image"))?;
    let timeout_ms = command.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS);
    let expiry = expiry_seconds(timeout_ms)?;
    client.limits = command
        .lease
        .as_ref()
        .map_or_else(crate::RemoteIoLimits::default, ExecutionPermit::io_limits);
    let mut deadline = phase::Deadline::new(Some(timeout_ms));
    deadline
        .within(client.validate_plan(command.resources))
        .await?;
    let (archive, input_phase) = deadline
        .within(phase::run(
            "northflank",
            "prepare",
            create_input_archive_limited(
                workspace_root,
                command.inputs,
                "northflank",
                client.limits,
                command.lease.as_ref(),
            ),
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
    let (service, cleanup) = deadline
        .within(phase::run(
            "northflank",
            "create",
            lifecycle::create_owned(
                client.clone(),
                Some(journal),
                Duration::from_secs(expiry + 600),
                command.lease.clone(),
                move |client, marker| async move {
                    client
                        .create(
                            &image,
                            expiry,
                            &marker.expect("journaled create has marker"),
                        )
                        .await
                },
            ),
        ))
        .await?;
    let result = Box::pin(deadline.within(async {
        let service = phase::run("northflank", "startup", client.wait_ready(service)).await?;
        run_setup(
            &client,
            &service.id,
            &["mkdir".into(), "-p".into(), GUEST_ROOT.into()],
            None,
            workspace_root,
        )
        .await?;
        run_setup(
            &client,
            &service.id,
            &[
                "tar".into(),
                "-xf".into(),
                "-".into(),
                "--no-same-owner".into(),
                "-C".into(),
                GUEST_ROOT.into(),
            ],
            Some(archive.path()),
            workspace_root,
        )
        .await?;
        drop(archive);
        drop(input_phase);

        let stdout = output_archive_file(workspace_root)?;
        let stderr = output_archive_file(workspace_root)?;
        let argv = command_argv(&command)?;
        let exit_code = Box::pin(phase::run(
            "northflank",
            "execute",
            client.exec(
                &service.id,
                &argv,
                None,
                stdout.path(),
                stderr.path(),
                client.limits.max_capture_bytes,
                capture.as_ref(),
            ),
        ))
        .await?;
        let stdout = stream::to_cache(
            tokio::fs::File::from_std(
                stdout
                    .reopen()
                    .map_err(|_| api_error("cannot open stdout capture"))?,
            ),
            Destination::Stdout,
            cache,
            stream_to_parent,
        )
        .await?;
        let stderr = stream::to_cache(
            tokio::fs::File::from_std(
                stderr
                    .reopen()
                    .map_err(|_| api_error("cannot open stderr capture"))?,
            ),
            Destination::Stderr,
            cache,
            stream_to_parent,
        )
        .await?;

        if command.accepts_exit_code(exit_code) && !command.outputs.is_empty() {
            let archive = output_archive_file(workspace_root)?;
            let errors = output_archive_file(workspace_root)?;
            let phase = crate::remote::phase_permit(
                command.lease.as_ref(),
                client.limits.max_archive_bytes.saturating_mul(2),
            )
            .await?;
            let mut argv = vec![
                "tar".into(),
                "-cf".into(),
                "-".into(),
                "-C".into(),
                GUEST_ROOT.into(),
            ];
            argv.extend(
                command
                    .outputs
                    .iter()
                    .map(|path| format!("./{}", path.as_str())),
            );
            let mut transfer = client.clone();
            transfer.limits.command_idle_timeout = Some(client.limits.stall_timeout);
            let code = transfer
                .exec(
                    &service.id,
                    &argv,
                    None,
                    archive.path(),
                    errors.path(),
                    client.limits.max_archive_bytes,
                    None,
                )
                .await?;
            ensure_success(code, "packing declared outputs")?;
            install_output_archive_limited(
                archive.path(),
                workspace_root,
                command.outputs,
                "northflank",
                client.limits,
                phase,
            )
            .await?;
        }
        Ok(ActionResult {
            exit_code,
            stdout: Some(stdout),
            stderr: Some(stderr),
            outputs: BTreeMap::new(),
        })
    }))
    .await;
    let cleanup_result = phase::run("northflank", "cleanup", cleanup.run()).await;
    match (result, cleanup_result) {
        (Ok(result), Ok(())) => Ok(result),
        (Ok(_), Err(error)) | (Err(error), Ok(())) => Err(error),
        (Err(error), Err(cleanup_error)) => {
            tracing::warn!(provider = "northflank", %cleanup_error, "failed to delete service after execution failure");
            Err(error)
        }
    }
}

async fn run_setup(
    client: &Client,
    service: &str,
    argv: &[String],
    input: Option<&Path>,
    workspace: &Path,
) -> Result<()> {
    let stdout = output_archive_file(workspace)?;
    let stderr = output_archive_file(workspace)?;
    let mut client = client.clone();
    client.limits.command_idle_timeout = Some(client.limits.stall_timeout);
    let code = client
        .exec(
            service,
            argv,
            input,
            stdout.path(),
            stderr.path(),
            client.limits.control_bytes(),
            None,
        )
        .await?;
    ensure_success(code, "staging declared inputs")
}

fn command_argv(command: &PreparedCommand<'_>) -> Result<Vec<String>> {
    if command.argv.is_empty() {
        return Err(crate::Error::EmptyArgv);
    }
    let argv = resolve_execution_argv(command.argv, Path::new(GUEST_ROOT));
    let env = resolve_execution_env(command.env, Path::new(GUEST_ROOT));
    let cwd = command.cwd.map_or_else(
        || GUEST_ROOT.into(),
        |cwd| crate::remote::join_path(GUEST_ROOT, cwd.as_str()),
    );
    let mut result = vec!["env".into(), "--".into()];
    result.extend(env.iter().map(|(key, value)| format!("{key}={value}")));
    result.extend([
        "sh".into(),
        "-c".into(),
        format!(
            "cd {} && exec {}",
            shell_word(&cwd),
            argv.iter()
                .map(|arg| shell_word(arg))
                .collect::<Vec<_>>()
                .join(" ")
        ),
    ]);
    Ok(result)
}

fn shell_word(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub(super) fn expiry_seconds(timeout_ms: u64) -> Result<u64> {
    let seconds = timeout_ms.div_ceil(1000).saturating_add(300).max(300);
    if seconds > 604_800 {
        return Err(config_error(
            "Northflank action timeout plus cleanup grace must not exceed seven days",
        ));
    }
    Ok(seconds)
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
