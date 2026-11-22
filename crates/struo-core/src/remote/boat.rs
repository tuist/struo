use std::path::Path;

use struo_cas::{ActionResult, CacheProvider};

use super::{join_path, PreparedCommand};
use crate::{Error, RemoteExecution, Result, WorkspacePath};

mod capture;
mod client;
mod execution;
mod ownership;
#[cfg(test)]
mod tests;
mod transport;

use client::{Client, Config};
use execution::execute_with_client;

const GUEST_ROOT: &str = "/home/user/struo-workspace";
const INPUT_ARCHIVE: &str = "/tmp/struo-inputs.tar";
const OUTPUT_ARCHIVE: &str = "/tmp/struo-outputs.tar";

pub(super) async fn execute_command(
    remote: &RemoteExecution,
    command: PreparedCommand<'_>,
    workspace_root: &Path,
    cache: &CacheProvider,
    stream_to_parent: bool,
) -> Result<ActionResult> {
    execute_with_client(
        remote,
        command,
        workspace_root,
        cache,
        stream_to_parent,
        Client::new(Config::from_env()?)?,
    )
    .await
}

fn command_line(argv: &[String]) -> Result<String> {
    if argv.is_empty() {
        return Err(Error::EmptyArgv);
    }
    Ok(argv
        .iter()
        .map(|word| shell_word(word))
        .collect::<Vec<_>>()
        .join(" "))
}

fn pack_outputs_command(outputs: &[WorkspacePath]) -> String {
    let paths = outputs
        .iter()
        .map(|path| shell_word(&format!("./{}", path.as_str())))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "tar -cf {} -C {} {paths}",
        shell_word(OUTPUT_ARCHIVE),
        shell_word(GUEST_ROOT)
    )
}

fn workdir(cwd: Option<&WorkspacePath>) -> String {
    cwd.map_or_else(
        || GUEST_ROOT.to_string(),
        |cwd| join_path(GUEST_ROOT, cwd.as_str()),
    )
}

fn shell_word(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
