use std::collections::BTreeMap;
use std::path::Path;

use struo_cas::{ActionResult, CacheProvider};

use super::client::{Client, Sandbox};
use super::transport::file_error;
use super::{shell_word, GUEST_ROOT};
use crate::remote::archive::output_archive_file;
use crate::remote::PreparedCommand;
use crate::stream::{self, Destination};
use crate::{resolve_execution_argv, resolve_execution_env, Error, Result};

pub(super) async fn execute(
    client: &Client,
    sandbox: &Sandbox,
    origin: &reqwest::Url,
    command: &PreparedCommand<'_>,
    workspace: &Path,
    cache: &CacheProvider,
    stream_to_parent: bool,
) -> Result<ActionResult> {
    let argv = resolve_execution_argv(command.argv, Path::new(GUEST_ROOT));
    if argv.is_empty() {
        return Err(Error::EmptyArgv);
    }
    let env = resolve_execution_env(command.env, Path::new(GUEST_ROOT));
    let words = env
        .iter()
        .map(|(key, value)| shell_word(&format!("{key}={value}")))
        .chain(argv.iter().map(|word| shell_word(word)))
        .collect::<Vec<_>>()
        .join(" ");
    let script = format!("exec env -- {words} > /tmp/struo-stdout 2> /tmp/struo-stderr");
    let cwd = command.cwd.map_or_else(
        || GUEST_ROOT.into(),
        |cwd| crate::remote::join_path(GUEST_ROOT, cwd.as_str()),
    );
    let exit_code = client
        .process(
            sandbox,
            origin,
            &["sh".into(), "-c".into(), script],
            Some(&cwd),
        )
        .await?;
    let mut capture_client = client.clone();
    let mut remaining = client.limits.max_capture_bytes;
    let mut digests = Vec::new();
    for (remote_path, destination) in [
        ("/tmp/struo-stdout", Destination::Stdout),
        ("/tmp/struo-stderr", Destination::Stderr),
    ] {
        let file = output_archive_file(workspace)?;
        capture_client.limits.max_archive_bytes = client.limits.max_archive_bytes.min(remaining);
        let phase = capture_client
            .download(
                sandbox,
                origin,
                remote_path,
                file.path(),
                command.lease.as_ref(),
                1,
            )
            .await?;
        remaining = remaining.saturating_sub(
            tokio::fs::metadata(file.path())
                .await
                .map_err(file_error)?
                .len(),
        );
        let input = tokio::fs::File::open(file.path())
            .await
            .map_err(file_error)?;
        digests.push(stream::to_cache(input, destination, cache, stream_to_parent).await?);
        drop(phase);
    }
    Ok(ActionResult {
        exit_code,
        stdout: Some(digests[0]),
        stderr: Some(digests[1]),
        outputs: BTreeMap::new(),
    })
}
