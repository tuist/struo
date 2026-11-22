use std::collections::BTreeMap;
use std::path::Path;

use super::client::Client;
use super::{ensure_success, ROOT};
use crate::remote::archive::{install_output_archive_limited, output_archive_file};
use crate::remote::PreparedCommand;
use crate::{Result, WorkspacePath};

const OUTPUT: &str = "/tmp/struo-outputs.tar";

pub(super) fn arguments(outputs: &[WorkspacePath]) -> Vec<String> {
    let mut args = vec![
        "tar".into(),
        "--hard-dereference".into(),
        "-cf".into(),
        OUTPUT.into(),
        "-C".into(),
        ROOT.into(),
    ];
    args.extend(outputs.iter().map(|path| format!("./{}", path.as_str())));
    args
}

pub(super) async fn restore(
    client: &Client,
    id: &str,
    command: &PreparedCommand<'_>,
    workspace: &Path,
) -> Result<()> {
    ensure_success(
        client
            .command(id, &arguments(command.outputs), ROOT, &BTreeMap::new())
            .await?,
        "packing outputs",
    )?;
    let output = output_archive_file(workspace)?;
    let phase = client
        .download(id, OUTPUT, output.path(), command.lease.as_ref(), 2)
        .await?;
    install_output_archive_limited(
        output.path(),
        workspace,
        command.outputs,
        "vercel",
        client.limits,
        phase,
    )
    .await
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};

    use super::*;
    use crate::remote::PhasePermit;

    #[tokio::test]
    async fn restores_hard_linked_outputs_as_files_and_preserves_symlinks() {
        let source = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        let report = source.path().join("report");
        std::fs::create_dir(&report).unwrap();
        std::fs::write(report.join("original"), b"binary\0\xff").unwrap();
        std::fs::set_permissions(
            report.join("original"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        std::fs::hard_link(report.join("original"), report.join("copy")).unwrap();
        symlink("original", report.join("link")).unwrap();
        let archive = source.path().join("outputs.tar");
        let outputs = [WorkspacePath::try_from("report").unwrap()];
        let mut arguments = arguments(&outputs);
        arguments[3] = archive.to_str().unwrap().into();
        arguments[5] = source.path().to_str().unwrap().into();
        assert!(std::process::Command::new(&arguments[0])
            .args(&arguments[1..])
            .status()
            .unwrap()
            .success());
        install_output_archive_limited(
            &archive,
            destination.path(),
            &outputs,
            "vercel",
            crate::RemoteIoLimits::default(),
            PhasePermit::default(),
        )
        .await
        .unwrap();
        let restored = destination.path().join("report");
        for name in ["original", "copy"] {
            let path = restored.join(name);
            assert_eq!(std::fs::read(&path).unwrap(), b"binary\0\xff");
            assert_eq!(std::fs::metadata(path).unwrap().mode() & 0o777, 0o755);
        }
        assert_ne!(
            std::fs::metadata(restored.join("original")).unwrap().ino(),
            std::fs::metadata(restored.join("copy")).unwrap().ino()
        );
        assert_eq!(
            std::fs::read_link(restored.join("link")).unwrap(),
            Path::new("original")
        );
    }
}
