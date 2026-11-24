use super::{GUEST_ROOT, OUTPUT_ARCHIVE};
use crate::{Error, Result, WorkspacePath};

pub(super) fn command_line(argv: &[String]) -> Result<String> {
    if argv.is_empty() {
        return Err(Error::EmptyArgv);
    }
    Ok(argv
        .iter()
        .map(|word| shell_word(word))
        .collect::<Vec<_>>()
        .join(" "))
}

pub(super) fn pack_outputs_command(outputs: &[WorkspacePath]) -> String {
    let paths = outputs
        .iter()
        .map(|path| shell_word(&format!("./{}", path.as_str())))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "tar --hard-dereference -cf {} -C {} {paths}",
        shell_word(OUTPUT_ARCHIVE),
        shell_word(GUEST_ROOT)
    )
}

pub(super) fn workdir(cwd: Option<&WorkspacePath>) -> String {
    cwd.map_or_else(
        || GUEST_ROOT.to_string(),
        |cwd| super::super::join_path(GUEST_ROOT, cwd.as_str()),
    )
}

pub(super) fn shell_word(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};

    #[tokio::test]
    async fn output_collection_restores_hard_links_as_independent_files() {
        let guest = tempfile::tempdir().unwrap();
        let host = tempfile::tempdir().unwrap();
        let reports = guest.path().join("reports");
        std::fs::create_dir(&reports).unwrap();
        std::fs::write(reports.join("first"), b"binary\0\xff").unwrap();
        std::fs::set_permissions(
            reports.join("first"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        std::fs::hard_link(reports.join("first"), reports.join("second")).unwrap();
        symlink("first", reports.join("link")).unwrap();
        let archive = guest.path().join("outputs.tar");
        let outputs = [WorkspacePath::try_from("reports").unwrap()];
        let command = pack_outputs_command(&outputs)
            .replace(OUTPUT_ARCHIVE, &archive.to_string_lossy())
            .replace(GUEST_ROOT, &guest.path().to_string_lossy());
        let result = std::process::Command::new("sh")
            .args(["-c", &command])
            .output()
            .unwrap();
        assert!(result.status.success(), "{:?}", result.stderr);
        crate::remote::archive::install_output_archive_limited(
            &archive,
            host.path(),
            &outputs,
            "runloop",
            crate::RemoteIoLimits::default(),
            crate::remote::PhasePermit::default(),
        )
        .await
        .unwrap();
        let first = host.path().join("reports/first");
        let second = host.path().join("reports/second");
        assert_eq!(std::fs::read(&first).unwrap(), b"binary\0\xff");
        assert_eq!(std::fs::read(&second).unwrap(), b"binary\0\xff");
        let first = std::fs::metadata(first).unwrap();
        let second = std::fs::metadata(second).unwrap();
        assert_ne!(first.ino(), second.ino());
        assert_eq!(first.permissions().mode() & 0o777, 0o755);
        assert_eq!(second.permissions().mode() & 0o777, 0o755);
        assert_eq!(
            std::fs::read_link(host.path().join("reports/link")).unwrap(),
            std::path::Path::new("first")
        );
    }
}
