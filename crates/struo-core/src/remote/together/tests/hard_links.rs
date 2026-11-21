use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};

use super::*;

#[tokio::test]
async fn real_tar_restores_hard_linked_outputs_without_dereferencing_symlinks() {
    let guest_workspace = tempfile::tempdir().unwrap();
    let report = guest_workspace.path().join("report");
    std::fs::create_dir(&report).unwrap();
    std::fs::write(report.join("original"), [0, 255, 128]).unwrap();
    std::fs::set_permissions(
        report.join("original"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    std::fs::hard_link(report.join("original"), report.join("hard-link")).unwrap();
    symlink("original", report.join("symlink")).unwrap();
    let archive = guest_workspace.path().join("outputs.tar");
    let mut responses = vec![
        Response::empty(201),
        exited(0),
        exited(0),
        exited(0),
        exited(0),
        bytes(b"out"),
        bytes(b"err"),
    ]
    .into_iter();
    let (guest, guest_server) = test_server::spawn_with_handler(9, move |request| {
        if let Some(response) = responses.next() {
            return response;
        }
        if request.method == "POST" {
            let invocation: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
            assert_eq!(invocation["command"], "tar");
            let args = invocation["args"].as_array().unwrap().iter().map(|arg| {
                let arg = arg.as_str().unwrap();
                match arg {
                    ROOT => guest_workspace.path().to_str().unwrap(),
                    OUTPUT => archive.to_str().unwrap(),
                    _ => arg,
                }
            });
            let output = std::process::Command::new("tar")
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "tar failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            return exited(0);
        }
        bytes(&std::fs::read(&archive).unwrap())
    })
    .await;
    let (url, control_server) = test_server::spawn(vec![
        created(&guest),
        Response::json(r#"{"id":"sandbox-1","status":"terminated"}"#),
    ])
    .await;
    let workspace = workspace();
    let result = execute(
        Client::new(url, "test-key".into()).unwrap(),
        workspace.path(),
        &[WorkspacePath::try_from("report").unwrap()],
        None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(result.exit_code, 0);
    for path in ["original", "hard-link", "symlink"] {
        assert_eq!(
            std::fs::read(workspace.path().join("report").join(path)).unwrap(),
            [0, 255, 128]
        );
    }
    let report = workspace.path().join("report");
    assert_eq!(
        std::fs::read_link(report.join("symlink")).unwrap(),
        Path::new("original")
    );
    let original = std::fs::metadata(report.join("original")).unwrap();
    let hard_link = std::fs::metadata(report.join("hard-link")).unwrap();
    assert_eq!(original.permissions().mode() & 0o777, 0o755);
    assert_eq!(hard_link.permissions().mode() & 0o777, 0o755);
    assert_ne!(original.ino(), hard_link.ino());
    test_server::assert_ready(guest_server).await.unwrap();
    test_server::assert_ready(control_server).await.unwrap();
}
