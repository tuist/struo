use std::os::unix::fs::PermissionsExt;

use super::*;
use crate::WorkspacePath;

#[tokio::test]
async fn path_cwd_environment_and_shared_redirects_match_command_semantics() {
    let (workspace, cache) = setup();
    let bin = workspace.path().join("sub/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let tool = bin.join("probe");
    let cwd = WorkspacePath::try_from("sub".to_string()).unwrap();
    let output = WorkspacePath::try_from("logs/combined".to_string()).unwrap();
    let env = BTreeMap::from([
        ("PATH".into(), "missing:bin".into()),
        ("VALUE".into(), "a=b c".into()),
    ]);
    for header in ["#!/bin/sh\n", ""] {
        std::fs::write(&tool, format!("{header}printf '%s|%s|%s|' \"$PWD\" \"$VALUE\" \"$1\"; printf err >&2; read line && exit 4; exit 7\n")).unwrap();
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
        for program in ["probe", "bin/probe"] {
            let argv = vec![program.into(), "quoted arg".into()];
            let mut invocation = invocation(&argv, &env, Some(3000), NetworkPolicy::Unrestricted);
            invocation.cwd = Some(&cwd);
            invocation.redirect = Redirect {
                stdout: Some(&output),
                stderr: Some(&output),
            };
            let result = execute_command(invocation, workspace.path(), &cache)
                .await
                .unwrap();
            assert_eq!(result.exit_code, 7);
            assert_eq!((result.stdout, result.stderr), (None, None));
            assert_eq!(
                std::fs::read_to_string(output.resolve(workspace.path())).unwrap(),
                format!(
                    "{}|a=b c|quoted arg|err",
                    workspace.path().join("sub").display()
                )
            );
        }
    }
    let argv = vec!["/usr/bin/env".into()];
    let empty = BTreeMap::new();
    let result = execute_command(
        invocation(&argv, &empty, None, NetworkPolicy::Unrestricted),
        workspace.path(),
        &cache,
    )
    .await
    .unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(cache
        .get_blob(&result.stdout.unwrap())
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn missing_program_bad_cwd_and_denied_executable_fail_at_spawn() {
    let (workspace, cache) = setup();
    std::fs::write(workspace.path().join("blocked"), "not executable").unwrap();
    let env = BTreeMap::from([("PATH".into(), ":missing".into())]);
    let missing = WorkspacePath::try_from("missing".to_string()).unwrap();
    for (program, cwd, code) in [
        ("", None, libc::ENOENT),
        ("no-such-program", None, libc::ENOENT),
        ("/bin/true", Some(&missing), libc::ENOENT),
        ("blocked", None, libc::EACCES),
    ] {
        let argv = vec![program.into()];
        let mut invocation = invocation(&argv, &env, None, NetworkPolicy::Unrestricted);
        invocation.cwd = cwd;
        let error = execute_command(invocation, workspace.path(), &cache)
            .await
            .unwrap_err();
        assert!(
            matches!(error, Error::Spawn { source, .. } if source.raw_os_error() == Some(code))
        );
    }
}
