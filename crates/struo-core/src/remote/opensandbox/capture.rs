use std::collections::BTreeMap;
use std::path::Path;

use struo_cas::{ActionResult, CacheProvider};

use super::{
    client::{Client, Endpoint},
    word, ROOT,
};
use crate::remote::{archive::output_archive_file, join_path, PreparedCommand};
use crate::stream::{self, Destination};
use crate::{resolve_execution_argv, resolve_execution_env, Error, Result};

pub(super) async fn execute(
    client: &Client,
    endpoint: &Endpoint,
    command: &PreparedCommand<'_>,
    workspace: &Path,
    cache: &CacheProvider,
    stream: bool,
) -> Result<ActionResult> {
    if command.argv.is_empty() {
        return Err(Error::EmptyArgv);
    }
    let argv = resolve_execution_argv(command.argv, Path::new(ROOT));
    let env = resolve_execution_env(command.env, Path::new(ROOT));
    let cwd = command
        .cwd
        .map_or_else(|| ROOT.to_string(), |cwd| join_path(ROOT, cwd.as_str()));
    let line = argv
        .iter()
        .map(|arg| word(arg))
        .collect::<Vec<_>>()
        .join(" ");
    let exit_code = client
        .execute(
            endpoint,
            &format!(
                "(cd {} && {line}) > /tmp/struo-stdout 2> /tmp/struo-stderr",
                word(&cwd)
            ),
            Some(ROOT),
            &env,
            command.timeout_ms,
        )
        .await?;
    let mut digests = Vec::new();
    let mut remaining = client.limits.max_capture_bytes;
    for (source, destination) in [
        ("/tmp/struo-stdout", Destination::Stdout),
        ("/tmp/struo-stderr", Destination::Stderr),
    ] {
        let file = output_archive_file(workspace)?;
        let mut capture = client.clone();
        capture.limits.max_archive_bytes = capture.limits.max_archive_bytes.min(remaining);
        let permit = capture
            .download(endpoint, source, file.path(), command.lease.as_ref(), 1)
            .await?;
        let input = tokio::fs::File::open(file.path())
            .await
            .map_err(file_error)?;
        remaining = remaining.saturating_sub(input.metadata().await.map_err(file_error)?.len());
        digests.push(stream::to_cache(input, destination, cache, stream).await?);
        drop(permit);
    }
    Ok(ActionResult {
        exit_code,
        stdout: Some(digests[0]),
        stderr: Some(digests[1]),
        outputs: BTreeMap::new(),
    })
}

fn file_error(source: std::io::Error) -> Error {
    Error::FileAction {
        action: "read_remote_capture",
        path: ".struo/tmp".into(),
        source,
    }
}
