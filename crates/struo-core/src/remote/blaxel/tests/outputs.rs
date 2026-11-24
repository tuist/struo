use super::*;

#[cfg(target_os = "linux")]
#[tokio::test]
async fn hard_linked_outputs_restore_as_independent_files() {
    use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};

    let guest = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let reports = guest.path().join("reports");
    std::fs::create_dir(&reports).unwrap();
    std::fs::write(reports.join("one"), b"binary\0\xff").unwrap();
    std::fs::set_permissions(reports.join("one"), std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::hard_link(reports.join("one"), reports.join("two")).unwrap();
    symlink("one", reports.join("link")).unwrap();
    let archive = guest.path().join("output.tar");
    let outputs = [WorkspacePath::try_from("reports").unwrap()];
    let command = execution::pack_outputs_command(&outputs)
        .replace(
            &execution::shell_word(GUEST_ROOT),
            &execution::shell_word(guest.path().to_str().unwrap()),
        )
        .replace(
            &execution::shell_word(OUTPUT_ARCHIVE),
            &execution::shell_word(archive.to_str().unwrap()),
        );
    assert!(std::process::Command::new("sh")
        .args(["-c", &command])
        .status()
        .unwrap()
        .success());
    crate::remote::archive::install_output_archive_limited(
        &archive,
        host.path(),
        &outputs,
        "blaxel",
        crate::RemoteIoLimits::default(),
        crate::remote::PhasePermit::default(),
    )
    .await
    .unwrap();
    for name in ["one", "two", "link"] {
        assert_eq!(
            std::fs::read(host.path().join("reports").join(name)).unwrap(),
            b"binary\0\xff"
        );
    }
    let one = std::fs::metadata(host.path().join("reports/one")).unwrap();
    let two = std::fs::metadata(host.path().join("reports/two")).unwrap();
    assert_ne!(one.ino(), two.ino());
    assert_eq!(one.permissions().mode() & 0o777, 0o755);
    assert_eq!(
        std::fs::read_link(host.path().join("reports/link")).unwrap(),
        Path::new("one")
    );
}

#[tokio::test]
async fn provisioning_rounds_memory_up_in_api_megabytes_and_bounds_lifetime() {
    let workspace = workspace();
    let (url, server) = server::spawn(vec![
        Response::json("CREATE"),
        Response::empty(200),
        Response::empty(404),
    ])
    .await;
    let resources = ResourceRequest {
        memory_bytes: 3_000_000_001,
        ..ResourceRequest::default()
    };
    let (_, cleanup) = client(url)
        .create_owned(
            Journal::new(workspace.path()),
            Some("blaxel/test:version".into()),
            resources,
            Some(1),
            None,
        )
        .await
        .unwrap();
    cleanup.run().await.unwrap();
    let requests = test_server::assert_ready(server).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(body["spec"]["runtime"]["memory"], 3001);
    assert_eq!(body["spec"]["runtime"]["ttl"], "300s");
    assert_eq!(body["spec"]["runtime"]["image"], "blaxel/test:version");
}
