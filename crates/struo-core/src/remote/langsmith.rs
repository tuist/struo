use std::path::Path;

use struo_cas::{ActionResult, CacheProvider};

use super::{join_path, PreparedCommand};
use crate::{RemoteExecution, Result, WorkspacePath};

mod client;
mod execution;
mod ownership;
mod stream;
#[cfg(test)]
mod tests;
mod transfer;

use client::{Client, Config};
use execution::execute_with_client;

const GUEST_ROOT: &str = "/tmp/struo-workspace";
const INPUT_ARCHIVE: &str = "/tmp/struo-inputs.tar";
const OUTPUT_ARCHIVE: &str = "/tmp/struo-outputs.tar";

pub(super) async fn execute_command(
    remote: &RemoteExecution,
    command: PreparedCommand<'_>,
    workspace_root: &Path,
    cache: &CacheProvider,
    stream_to_parent: bool,
) -> Result<ActionResult> {
    Box::pin(execute_with_client(
        remote,
        command,
        workspace_root,
        cache,
        stream_to_parent,
        Client::new(Config::from_env()?)?,
    ))
    .await
}

fn workdir(cwd: Option<&WorkspacePath>) -> String {
    cwd.map_or_else(
        || GUEST_ROOT.into(),
        |cwd| join_path(GUEST_ROOT, cwd.as_str()),
    )
}
