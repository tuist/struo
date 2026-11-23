use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use struo_cas::{ActionResult, CacheProvider};

use super::client::{config_error, Client, Sandbox};
use super::transfer::file_error;
use super::{workdir, GUEST_ROOT, INPUT_ARCHIVE, OUTPUT_ARCHIVE};
use crate::remote::{
    archive::{create_input_archive_limited, install_output_archive_limited, output_archive_file},
    lifecycle::{self, Budget, Journal},
    phase, PreparedCommand,
};
use crate::stream::{self, Destination};
use crate::{
    resolve_execution_argv, resolve_execution_env, ExecutionPermit, RemoteExecution, Result,
};

pub(super) async fn execute_with_client(
    remote: &RemoteExecution,
    command: PreparedCommand<'_>,
    workspace_root: &Path,
    cache: &CacheProvider,
    stream_to_parent: bool,
    mut client: Client,
) -> Result<ActionResult> {
    let snapshot = remote
        .environment
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| config_error("LangSmith requires an explicit prepared snapshot"))?
        .to_string();
    client.limits = command
        .lease
        .as_ref()
        .map_or_else(crate::RemoteIoLimits::default, ExecutionPermit::io_limits);
    if client.limits.command_idle_timeout.is_some() {
        return Err(crate::Executor::LangSmith.unsupported("command output idle timeout"));
    }
    let mut deadline = phase::Deadline::new(Some(command.timeout_ms.unwrap_or(600_000)));
    let capture_budget = command.lease.as_ref().map(ExecutionPermit::capture_budget);
    if let Some(budget) = &capture_budget {
        deadline.within(budget.reserve_upfront()).await?;
    }
    let (archive, input_phase) = deadline
        .within(phase::run(
            "langsmith",
            "prepare",
            create_input_archive_limited(
                workspace_root,
                command.inputs,
                "langsmith",
                client.limits,
                command.lease.as_ref(),
            ),
        ))
        .await?;
    let journal = Journal::new(workspace_root);
    deadline
        .within(phase::run("langsmith", "reconcile", async {
            Ok(lifecycle::reconcile(&journal, &client, Budget::default()).await)
        }))
        .await?;
    let (sandbox, cleanup) = deadline.within(phase::run(
        "langsmith", "create",
        lifecycle::create_owned(
            client.clone(), Some(journal), Duration::from_mins(20), command.lease.clone(),
            move |client, marker| async move { client.create(&snapshot, marker.as_ref()).await },
        ),
    )).await?;
    let result = deadline
        .within(Box::pin(run_in_sandbox(
            &client,
            sandbox,
            &command,
            workspace_root,
            cache,
            stream_to_parent,
            (archive, input_phase),
        )))
        .await;
    let cleaned = phase::run("langsmith", "cleanup", cleanup.run()).await;
    match (result, cleaned) {
        (Ok(result), Ok(())) => Ok(result),
        (Ok(_), Err(error)) | (Err(error), Ok(())) => Err(error),
        (Err(error), Err(cleanup_error)) => {
            tracing::warn!(provider = "langsmith", %cleanup_error, "sandbox cleanup failed after action failure");
            Err(error)
        }
    }
}

async fn run_in_sandbox(
    client: &Client,
    sandbox: Sandbox,
    command: &PreparedCommand<'_>,
    workspace: &Path,
    cache: &CacheProvider,
    stream_to_parent: bool,
    (archive, input_phase): (
        crate::remote::archive::TempArchive,
        crate::remote::PhasePermit,
    ),
) -> Result<ActionResult> {
    let sandbox = phase::run("langsmith", "startup", client.wait_started(sandbox)).await?;
    phase::run(
        "langsmith",
        "upload",
        client.upload(&sandbox, archive.path(), archive.len(), INPUT_ARCHIVE),
    )
    .await?;
    drop(archive);
    drop(input_phase);
    let setup = format!("rm -rf -- {GUEST_ROOT} && mkdir -p -- {GUEST_ROOT} && tar -xf {INPUT_ARCHIVE} -C {GUEST_ROOT}");
    client
        .execute(
            &sandbox,
            &["sh".into(), "-c".into(), setup],
            "/tmp",
            workspace,
        )
        .await?
        .ensure_success()?;
    let result = phase::run(
        "langsmith",
        "execute",
        run(
            client,
            &sandbox,
            command,
            workspace,
            cache,
            stream_to_parent,
        ),
    )
    .await?;
    if command.accepts_exit_code(result.exit_code) && !command.outputs.is_empty() {
        let mut argv = vec![
            "tar".into(),
            "-cf".into(),
            OUTPUT_ARCHIVE.into(),
            "-C".into(),
            GUEST_ROOT.into(),
            "--".into(),
        ];
        argv.extend(
            command
                .outputs
                .iter()
                .map(|path| format!("./{}", path.as_str())),
        );
        client
            .execute(&sandbox, &argv, GUEST_ROOT, workspace)
            .await?
            .ensure_success()?;
        let archive = output_archive_file(workspace)?;
        let permit = client
            .download(
                &sandbox,
                OUTPUT_ARCHIVE,
                archive.path(),
                command.lease.as_ref(),
            )
            .await?;
        install_output_archive_limited(
            archive.path(),
            workspace,
            command.outputs,
            "langsmith",
            client.limits,
            permit,
        )
        .await?;
    }
    Ok(result)
}

async fn run(
    client: &Client,
    sandbox: &Sandbox,
    command: &PreparedCommand<'_>,
    workspace: &Path,
    cache: &CacheProvider,
    stream_to_parent: bool,
) -> Result<ActionResult> {
    let argv = resolve_execution_argv(command.argv, Path::new(GUEST_ROOT));
    let env = resolve_execution_env(command.env, Path::new(GUEST_ROOT));
    let capture = client
        .execute_with_env(sandbox, &argv, &env, &workdir(command.cwd), workspace)
        .await?;
    let stdout = tokio::fs::File::open(capture.stdout.path())
        .await
        .map_err(file_error)?;
    let stdout = stream::to_cache(stdout, Destination::Stdout, cache, stream_to_parent).await?;
    let stderr = tokio::fs::File::open(capture.stderr.path())
        .await
        .map_err(file_error)?;
    let stderr = stream::to_cache(stderr, Destination::Stderr, cache, stream_to_parent).await?;
    Ok(ActionResult {
        exit_code: capture.exit_code,
        stdout: Some(stdout),
        stderr: Some(stderr),
        outputs: BTreeMap::new(),
    })
}
