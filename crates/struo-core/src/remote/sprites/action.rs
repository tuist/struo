use std::path::Path;

use struo_cas::{ActionResult, CacheProvider};

use super::{
    client::{Client, Sprite},
    setup, GUEST_ROOT, INPUT_ARCHIVE, OUTPUT_ARCHIVE,
};
use crate::executor::staging::CapturePermit;
use crate::remote::{
    archive::{install_output_archive_limited, output_archive_file, TempArchive},
    join_path, CommandFilesystemSetup, PhasePermit, PreparedCommand,
};
use crate::{resolve_execution_argv, resolve_execution_env, Result};

#[allow(clippy::too_many_arguments)]
pub(super) async fn run(
    client: &Client,
    sprite: &Sprite,
    command: &PreparedCommand<'_>,
    workspace: &Path,
    cache: &CacheProvider,
    stream: bool,
    archive: TempArchive,
    input_phase: PhasePermit,
    capture: Option<&CapturePermit>,
    locked: bool,
    filesystem: CommandFilesystemSetup<'_>,
) -> Result<ActionResult> {
    client
        .upload(sprite, archive.path(), archive.len(), cache, capture)
        .await?;
    drop(archive);
    drop(input_phase);
    setup(
        client,
        sprite,
        &["mkdir".into(), "-p".into(), GUEST_ROOT.into()],
        cache,
        capture,
    )
    .await?;
    setup(
        client,
        sprite,
        &[
            "tar".into(),
            "-xf".into(),
            INPUT_ARCHIVE.into(),
            "-C".into(),
            GUEST_ROOT.into(),
        ],
        cache,
        capture,
    )
    .await?;
    super::filesystem::apply(
        client, sprite, command, filesystem, workspace, cache, capture,
    )
    .await?;
    let (argv, env) = if locked {
        Box::pin(super::tools::command(
            client, sprite, command, cache, capture,
        ))
        .await?
    } else {
        (
            resolve_execution_argv(command.argv, Path::new(GUEST_ROOT)),
            resolve_execution_env(command.env, Path::new(GUEST_ROOT)),
        )
    };
    let cwd = command.cwd.map_or_else(
        || GUEST_ROOT.to_string(),
        |cwd| join_path(GUEST_ROOT, cwd.as_str()),
    );
    let result = crate::remote::phase::run(
        "sprites",
        "execute",
        Box::pin(client.execute(sprite, &argv, Some(&cwd), &env, cache, stream, capture)),
    )
    .await?;
    if command.accepts_exit_code(result.exit_code) && !command.outputs.is_empty() {
        let roots = crate::remote::path::output_roots(command.outputs);
        let mut argv = vec![
            "tar".into(),
            "--hard-dereference".into(),
            "-cf".into(),
            OUTPUT_ARCHIVE.into(),
            "-C".into(),
            GUEST_ROOT.into(),
        ];
        argv.extend(roots.iter().map(|path| format!("./{}", path.as_str())));
        setup(client, sprite, &argv, cache, capture).await?;
        let archive = output_archive_file(workspace)?;
        let phase = client
            .download(sprite, archive.path(), command.lease.as_ref())
            .await?;
        install_output_archive_limited(
            archive.path(),
            workspace,
            command.outputs,
            "sprites",
            client.limits,
            phase,
        )
        .await?;
    }
    Ok(result)
}
