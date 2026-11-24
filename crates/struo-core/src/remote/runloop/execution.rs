use std::{collections::BTreeMap, path::Path};
use struo_cas::{ActionResult, CacheProvider};

use super::client::{Client, Sandbox};
use super::{
    capture, ensure_success, pack_outputs_command, shell_word, GUEST_ROOT, INPUT_ARCHIVE,
    OUTPUT_ARCHIVE,
};
use crate::executor::staging::CapturePermit;
use crate::remote::archive::{install_output_archive_limited, output_archive_file, TempArchive};
use crate::remote::{PhasePermit, PreparedCommand};
use crate::Result;

#[allow(clippy::too_many_arguments)]
pub(super) async fn run(
    client: &Client,
    sandbox: Sandbox,
    command: &PreparedCommand<'_>,
    workspace_root: &Path,
    cache: &CacheProvider,
    stream_to_parent: bool,
    archive: TempArchive,
    input_phase: PhasePermit,
    capture_phase: Option<CapturePermit>,
) -> Result<ActionResult> {
    let sandbox = client.wait_started(sandbox).await?;
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
                    "rm -rf {} && mkdir -p {} && tar -xf {} -C {}",
                    shell_word(GUEST_ROOT),
                    shell_word(GUEST_ROOT),
                    shell_word(INPUT_ARCHIVE),
                    shell_word(GUEST_ROOT)
                ),
                None,
                &BTreeMap::new(),
            )
            .await?,
        "staging declared inputs",
    )?;
    let result = capture::execute(
        client,
        &sandbox,
        command,
        workspace_root,
        cache,
        stream_to_parent,
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
            "runloop",
            client.limits,
            phase,
        )
        .await?;
    }
    Ok(result)
}
