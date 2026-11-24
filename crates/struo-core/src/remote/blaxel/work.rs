use std::collections::BTreeMap;
use std::path::Path;

use struo_cas::{ActionResult, CacheProvider};

use super::client::{Client, Sandbox};
use super::execution::{ensure_success, pack_outputs_command, shell_word};
use super::{capture, GUEST_ROOT, INPUT_ARCHIVE, OUTPUT_ARCHIVE};
use crate::executor::staging::CapturePermit;
use crate::remote::archive::{install_output_archive_limited, output_archive_file, TempArchive};
use crate::remote::{phase, PhasePermit, PreparedCommand};
use crate::Result;

pub(super) async fn run(
    client: &Client,
    sandbox: Sandbox,
    command: &PreparedCommand<'_>,
    workspace_root: &Path,
    cache: &CacheProvider,
    stream_to_parent: bool,
    input: (TempArchive, PhasePermit, Option<CapturePermit>),
) -> Result<ActionResult> {
    let sandbox = phase::run("blaxel", "startup", client.wait_started(sandbox)).await?;
    let (archive, input_phase, capture_phase) = input;
    client
        .upload(&sandbox, archive.path(), archive.len(), INPUT_ARCHIVE)
        .await?;
    drop(archive);
    drop(input_phase);
    ensure_success(
        &client
            .execute(
                &sandbox,
                &format!(
                    "rm -rf {root} && mkdir -p {root} && tar -xf {archive} -C {root}",
                    root = shell_word(GUEST_ROOT),
                    archive = shell_word(INPUT_ARCHIVE)
                ),
                None,
                &BTreeMap::new(),
                command.timeout_ms,
            )
            .await?,
        "staging declared inputs",
    )?;
    let result = phase::run(
        "blaxel",
        "execute",
        Box::pin(capture::execute(
            client,
            &sandbox,
            command,
            workspace_root,
            cache,
            stream_to_parent,
        )),
    )
    .await?;
    drop(capture_phase);
    if command.accepts_exit_code(result.exit_code) && !command.outputs.is_empty() {
        ensure_success(
            &client
                .execute(
                    &sandbox,
                    &pack_outputs_command(command.outputs),
                    None,
                    &BTreeMap::new(),
                    command.timeout_ms,
                )
                .await?,
            "packing declared outputs",
        )?;
        let archive = output_archive_file(workspace_root)?;
        let phase = client
            .download(
                &sandbox,
                OUTPUT_ARCHIVE,
                archive.path(),
                command.lease.as_ref(),
                2,
            )
            .await?;
        install_output_archive_limited(
            archive.path(),
            workspace_root,
            command.outputs,
            "blaxel",
            client.limits,
            phase,
        )
        .await?;
    }
    Ok(result)
}
