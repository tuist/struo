use std::path::Path;

use struo_cas::CacheProvider;

use super::{
    client::{Client, Sprite},
    setup, GUEST_ROOT, INPUT_ARCHIVE,
};
use crate::executor::staging::CapturePermit;
use crate::remote::{
    archive::create_input_archive_limited, join_path, CommandFilesystemSetup, PreparedCommand,
};
use crate::Result;

pub(super) async fn apply(
    client: &Client,
    sprite: &Sprite,
    command: &PreparedCommand<'_>,
    filesystem: CommandFilesystemSetup<'_>,
    workspace: &Path,
    cache: &CacheProvider,
    capture: Option<&CapturePermit>,
) -> Result<()> {
    if !filesystem.clean_paths.is_empty() {
        let mut argv = vec!["rm".into(), "-rf".into(), "--".into()];
        argv.extend(
            filesystem
                .clean_paths
                .iter()
                .map(|path| join_path(GUEST_ROOT, path.as_str())),
        );
        setup(client, sprite, &argv, cache, capture).await?;
    }
    let mut dirs = vec![GUEST_ROOT.to_string()];
    dirs.extend(
        filesystem
            .create_dirs
            .iter()
            .map(|path| join_path(GUEST_ROOT, path.as_str())),
    );
    if let Some(cwd) = command.cwd {
        dirs.push(join_path(GUEST_ROOT, cwd.as_str()));
    }
    for output in command.outputs {
        if let Some(parent) = Path::new(output.as_str()).parent() {
            dirs.push(join_path(GUEST_ROOT, &parent.to_string_lossy()));
        }
    }
    let mut argv = vec!["mkdir".into(), "-p".into(), "--".into()];
    argv.extend(dirs);
    setup(client, sprite, &argv, cache, capture).await?;
    if !filesystem.post_setup_inputs.is_empty() {
        let (archive, _phase) = create_input_archive_limited(
            workspace,
            filesystem.post_setup_inputs,
            "sprites",
            client.limits,
            command.lease.as_ref(),
        )
        .await?;
        client
            .upload(sprite, archive.path(), archive.len(), cache, capture)
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
    }
    Ok(())
}
