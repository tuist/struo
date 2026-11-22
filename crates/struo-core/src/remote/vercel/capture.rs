use std::collections::BTreeMap;
use std::path::Path;

use struo_cas::{ActionResult, CacheProvider};

use super::client::Client;
use super::transport::file_error;
use super::ROOT;
use crate::remote::{archive::output_archive_file, PreparedCommand};
use crate::stream::{self, Destination};
use crate::Result;

pub(super) async fn capture(
    client: &Client,
    id: &str,
    command: &PreparedCommand<'_>,
    workspace: &Path,
    cache: &CacheProvider,
    stream: bool,
) -> Result<ActionResult> {
    let stdout = output_archive_file(workspace)?;
    let stderr = output_archive_file(workspace)?;
    let argv = crate::resolve_execution_argv(command.argv, Path::new(ROOT));
    let env = crate::resolve_execution_env(command.env, Path::new(ROOT));
    let mut wrapped = vec![
        "sh".into(),
        "-c".into(),
        "exec env -i PATH=\"$PATH\" \"$@\" > /tmp/struo-stdout 2> /tmp/struo-stderr".into(),
        "struo".into(),
    ];
    wrapped.extend(env.iter().map(|(key, value)| format!("{key}={value}")));
    wrapped.extend(argv);
    let cwd = command.cwd.map_or_else(
        || ROOT.to_string(),
        |cwd| super::super::join_path(ROOT, cwd.as_str()),
    );
    let exit_code = client.command(id, &wrapped, &cwd, &BTreeMap::new()).await?;
    let mut capture_client = client.clone();
    capture_client.limits.max_archive_bytes = client
        .limits
        .max_capture_bytes
        .min(client.limits.max_archive_bytes);
    let stdout_phase = capture_client
        .download(id, "/tmp/struo-stdout", stdout.path(), None, 1)
        .await?;
    let consumed = tokio::fs::metadata(stdout.path())
        .await
        .map_err(file_error)?
        .len();
    capture_client.limits.max_archive_bytes = client
        .limits
        .max_capture_bytes
        .saturating_sub(consumed)
        .min(client.limits.max_archive_bytes);
    let stderr_phase = capture_client
        .download(id, "/tmp/struo-stderr", stderr.path(), None, 1)
        .await?;
    let (stdout, stderr) = tokio::try_join!(
        cache_capture(stdout.path(), Destination::Stdout, cache, stream),
        cache_capture(stderr.path(), Destination::Stderr, cache, stream),
    )?;
    drop((stdout_phase, stderr_phase));
    Ok(ActionResult {
        exit_code,
        stdout: Some(stdout),
        stderr: Some(stderr),
        outputs: BTreeMap::new(),
    })
}

async fn cache_capture(
    path: &Path,
    destination: Destination,
    cache: &CacheProvider,
    stream: bool,
) -> Result<struo_cas::Digest> {
    stream::to_cache(
        tokio::fs::File::open(path).await.map_err(file_error)?,
        destination,
        cache,
        stream,
    )
    .await
}
