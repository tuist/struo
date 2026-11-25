use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

use super::super::cas::invalid;
use super::super::{sandbox, ServeOptions};
use crate::local::process_group::GroupChild;

pub(super) struct Output {
    pub exit_code: i32,
    pub stdout: PathBuf,
    pub stderr: PathBuf,
}

#[expect(clippy::too_many_arguments)]
pub(super) async fn run(
    options: &ServeOptions,
    workspace: &Path,
    argv: &[String],
    env: &BTreeMap<String, String>,
    cwd: &str,
    root: &Path,
    timeout_ms: Option<u64>,
    limit: u64,
    cancel: &CancellationToken,
    redirect: crate::local::Redirect<'_>,
) -> io::Result<Output> {
    let mut command = sandbox::command(options, workspace, argv, env, cwd)?;
    let stdout_file = redirect
        .stdout
        .map(|path| redirect_file(workspace, path))
        .transpose()?;
    let stderr_file = redirect
        .stderr
        .map(|path| {
            if redirect.stdout == Some(path) {
                stdout_file
                    .as_ref()
                    .ok_or_else(|| invalid("missing stdout redirect"))?
                    .try_clone()
            } else {
                redirect_file(workspace, path)
            }
        })
        .transpose()?;
    if let Some(file) = stdout_file {
        command.stdout(std::process::Stdio::from(file));
    }
    if let Some(file) = stderr_file {
        command.stderr(std::process::Stdio::from(file));
    }
    let mut group = GroupChild::spawn(&mut command)?;
    let stdout = root.join("stdout");
    let stderr = root.join("stderr");
    let remaining = Arc::new(AtomicU64::new(limit));
    let out = group.take_stdout();
    let err = group.take_stderr();
    let capture = async {
        tokio::try_join!(
            Box::pin(drain(out, &stdout, remaining.clone())),
            Box::pin(drain(err, &stderr, remaining))
        )?;
        Ok::<_, io::Error>(())
    };
    let work = async {
        tokio::try_join!(
            async {
                group.exited().await?;
                group.kill_group();
                Ok::<_, io::Error>(())
            },
            capture
        )?;
        Ok::<_, io::Error>(())
    };
    let result = tokio::select! {
        result = tokio::time::timeout(Duration::from_millis(timeout_ms.unwrap_or(3_600_000).min(3_600_000)), work) => {
            result.unwrap_or_else(|_| Err(io::Error::new(io::ErrorKind::TimedOut, "worker action timed out")))
        }
        () = cancel.cancelled() => Err(io::Error::new(io::ErrorKind::Interrupted, "worker action cancelled")),
    };
    group.kill_group();
    // Reaping the outer namespace supervisor confirms namespace teardown before
    // filesystem cleanup, including children that escaped the process group.
    let status = tokio::time::timeout(Duration::from_secs(5), group.reap())
        .await
        .map_err(|_| io::Error::other("worker namespace cleanup uncertain"))?
        .map_err(|_| io::Error::other("worker namespace cleanup failed"))?;
    result?;
    Ok(Output {
        exit_code: status.code().unwrap_or(-1),
        stdout,
        stderr,
    })
}

fn redirect_file(workspace: &Path, path: &crate::WorkspacePath) -> io::Result<std::fs::File> {
    if path.as_str().is_empty() {
        return Err(invalid("cannot redirect to workspace root"));
    }
    let path = sandbox::checked_path(workspace, path, false)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::File::create(path)
}

async fn drain(
    pipe: Option<impl AsyncRead + Unpin>,
    path: &Path,
    remaining: Arc<AtomicU64>,
) -> io::Result<()> {
    let mut file = tokio::fs::File::create(path).await?;
    let Some(mut pipe) = pipe else {
        return Ok(());
    };
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read = pipe.read(&mut buffer).await?;
        if read == 0 {
            return file.flush().await;
        }
        remaining
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |left| {
                left.checked_sub(read as u64)
            })
            .map_err(|_| invalid("CAS capacity exceeded by command output"))?;
        file.write_all(&buffer[..read]).await?;
    }
}
