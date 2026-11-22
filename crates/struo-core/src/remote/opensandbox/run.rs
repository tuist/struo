use std::collections::BTreeMap;
use std::path::Path;

use struo_cas::{ActionResult, CacheProvider};

use super::{
    capture,
    client::{Client, Sandbox},
    ensure_success, word, INPUT, OUTPUT, PROVIDER, ROOT,
};
use crate::remote::{
    archive::{install_output_archive_limited, output_archive_file, TempArchive},
    phase, PhasePermit, PreparedCommand,
};
use crate::Result;

#[allow(clippy::too_many_arguments)]
pub(super) async fn execute(
    client: &Client,
    sandbox: &Sandbox,
    command: &PreparedCommand<'_>,
    workspace: &Path,
    cache: &CacheProvider,
    stream: bool,
    archive: TempArchive,
    input_phase: PhasePermit,
) -> Result<ActionResult> {
    let endpoint = phase::run(PROVIDER, "startup", client.ready(sandbox)).await?;
    client
        .upload(&endpoint, archive.path(), archive.len(), INPUT)
        .await?;
    drop(archive);
    drop(input_phase);
    let setup = format!(
        "mkdir -p {} && tar -xf {} -C {}",
        word(ROOT),
        word(INPUT),
        word(ROOT)
    );
    ensure_success(
        client
            .execute(
                &endpoint,
                &setup,
                None,
                &BTreeMap::new(),
                command.timeout_ms,
            )
            .await?,
        "staging inputs",
    )?;
    let result = phase::run(
        PROVIDER,
        "execute",
        Box::pin(capture::execute(
            client, &endpoint, command, workspace, cache, stream,
        )),
    )
    .await?;
    if command.accepts_exit_code(result.exit_code) && !command.outputs.is_empty() {
        let paths = command
            .outputs
            .iter()
            .map(|path| word(&format!("./{}", path.as_str())))
            .collect::<Vec<_>>()
            .join(" ");
        let pack = format!("tar -cf {} -C {} {paths}", word(OUTPUT), word(ROOT));
        ensure_success(
            client
                .execute(&endpoint, &pack, None, &BTreeMap::new(), command.timeout_ms)
                .await?,
            "packing outputs",
        )?;
        let archive = output_archive_file(workspace)?;
        let permit = client
            .download(&endpoint, OUTPUT, archive.path(), command.lease.as_ref(), 2)
            .await?;
        install_output_archive_limited(
            archive.path(),
            workspace,
            command.outputs,
            PROVIDER,
            client.limits,
            permit,
        )
        .await?;
    }
    Ok(result)
}
