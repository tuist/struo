use std::path::Path;

use struo_cas::{ActionResult, CacheProvider};

use super::PreparedCommand;
use crate::{RemoteExecution, Result};

mod capture;
mod client;
mod execution;
mod ownership;
mod provisioning;
#[cfg(test)]
mod tests;
mod transport;

use client::Client;

const GUEST_ROOT: &str = "/home/tl-user/struo-workspace";
const INPUT_ARCHIVE: &str = "/tmp/struo-inputs.tar";
const OUTPUT_ARCHIVE: &str = "/tmp/struo-outputs.tar";

pub(super) async fn execute_command(
    remote: &RemoteExecution,
    command: PreparedCommand<'_>,
    workspace_root: &Path,
    cache: &CacheProvider,
    stream_to_parent: bool,
) -> Result<ActionResult> {
    execution::execute_with_client(
        remote,
        command,
        workspace_root,
        cache,
        stream_to_parent,
        Client::from_env(remote)?,
    )
    .await
}

fn shell_word(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
