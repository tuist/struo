use sha2::{Digest as _, Sha256};
use tokio_util::sync::CancellationToken;

use super::{action, executor, Cas};
use crate::execution_tool::ExecutionTool;

#[cfg(unix)]
#[tokio::test]
async fn launcher_applies_to_commands_and_verified_tool_bindings_are_reused() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let mut executor = executor(root.path());
    if let Err(error) = executor.probe(root.path()).await {
        eprintln!("ISOLATION NOT EXERCISED: {error}");
        return;
    }
    let data = executor.options.preparation_root.join("data");
    let launcher = data.join("launcher");
    std::fs::write(
        &launcher,
        "#!/bin/sh\nexport LAUNCHER_ACTIVE=1\nexec \"$@\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o755)).unwrap();
    executor.options.tool_launcher = vec![launcher.to_string_lossy().into_owned()];
    let contents = "#!/bin/sh\ntest \"$LAUNCHER_ACTIVE\" = 1 && echo tool-active\n";
    let tool_path = data.join("compiler");
    std::fs::write(&tool_path, contents).unwrap();
    std::fs::set_permissions(&tool_path, std::fs::Permissions::from_mode(0o755)).unwrap();
    let tool = ExecutionTool {
        path: "/analysis/compiler".into(),
        resolve: vec![
            "/bin/sh".into(),
            "-c".into(),
            format!(
                "echo resolved > resolver-ran; printf '%s' '{}'",
                tool_path.display()
            ),
        ],
        suffix: String::new(),
        sha256: hex::encode(Sha256::digest(contents.as_bytes())),
    };
    let mut cas = Cas::open(root.path().join("cas"), 1024 * 1024).unwrap();
    for expected in ["-e", "! -e"] {
        let response = executor
            .execute(
                root.path(),
                &mut cas,
                action(&format!(
                    "{} && test {expected} resolver-ran",
                    tool.marker()
                )),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(response.result.exit_code, 0);
    }
    let bad = ExecutionTool {
        sha256: "00".repeat(32),
        ..tool
    };
    let error = executor
        .execute(
            root.path(),
            &mut cas,
            action(&bad.marker()),
            CancellationToken::new(),
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("SHA256 mismatch"));
}
