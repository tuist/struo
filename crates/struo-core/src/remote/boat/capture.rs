use std::collections::BTreeMap;
use std::path::Path;

use struo_cas::{ActionResult, CacheProvider};

use super::client::{Client, Sandbox};
use super::{command_line, shell_word, workdir, GUEST_ROOT};
use crate::remote::archive::output_archive_file;
use crate::remote::PreparedCommand;
use crate::stream::{self, Destination};
use crate::{resolve_execution_argv, resolve_execution_env, Error, Result};

pub(super) async fn execute(
    client: &Client,
    sandbox: &Sandbox,
    command: &PreparedCommand<'_>,
    workspace_root: &Path,
    cache: &CacheProvider,
    stream_to_parent: bool,
) -> Result<ActionResult> {
    let stdout = output_archive_file(workspace_root)?;
    let stderr = output_archive_file(workspace_root)?;
    let guest_stdout = "/tmp/struo-stdout";
    let guest_stderr = "/tmp/struo-stderr";
    let argv = resolve_execution_argv(command.argv, Path::new(GUEST_ROOT));
    let env = resolve_execution_env(command.env, Path::new(GUEST_ROOT));
    let env = env
        .iter()
        .map(|(key, value)| shell_word(&format!("{key}={value}")))
        .collect::<Vec<_>>()
        .join(" ");
    let response = client
        .execute(
            sandbox,
            &format!(
                "exec env {env} {} > {} 2> {}",
                command_line(&argv)?,
                shell_word(guest_stdout),
                shell_word(guest_stderr)
            ),
            Some(&workdir(command.cwd)),
        )
        .await?;
    let exit_code = response.exit_code()?;
    let mut capture_client = client.clone();
    capture_client.limits.max_archive_bytes = client
        .limits
        .max_archive_bytes
        .min(client.limits.max_capture_bytes);
    let stdout_phase = capture_client
        .download(
            sandbox,
            guest_stdout,
            stdout.path(),
            command.lease.as_ref(),
            1,
        )
        .await?;
    let consumed = tokio::fs::metadata(stdout.path())
        .await
        .map_err(file_error)?
        .len();
    let stdout_digest =
        to_cache(stdout.path(), Destination::Stdout, cache, stream_to_parent).await?;
    drop(stdout);
    drop(stdout_phase);
    capture_client.limits.max_archive_bytes = client
        .limits
        .max_archive_bytes
        .min(client.limits.max_capture_bytes.saturating_sub(consumed));
    let stderr_phase = capture_client
        .download(
            sandbox,
            guest_stderr,
            stderr.path(),
            command.lease.as_ref(),
            1,
        )
        .await?;
    let stderr_digest =
        to_cache(stderr.path(), Destination::Stderr, cache, stream_to_parent).await?;
    drop(stderr);
    drop(stderr_phase);
    Ok(ActionResult {
        exit_code,
        stdout: Some(stdout_digest),
        stderr: Some(stderr_digest),
        outputs: BTreeMap::new(),
    })
}

async fn to_cache(
    path: &Path,
    destination: Destination,
    cache: &CacheProvider,
    stream_to_parent: bool,
) -> Result<struo_cas::Digest> {
    let file = tokio::fs::File::open(path).await.map_err(file_error)?;
    stream::to_cache(file, destination, cache, stream_to_parent).await
}

fn file_error(source: std::io::Error) -> Error {
    Error::FileAction {
        action: "read_remote_capture",
        path: ".struo/tmp".into(),
        source,
    }
}
