use std::os::unix::fs::PermissionsExt;

use super::*;
use crate::Error;

async fn capture(
    client: &Client,
    workspace: &Path,
    argv: &[String],
    env: &BTreeMap<String, String>,
) -> Result<ActionResult> {
    super::super::capture::execute(
        client,
        &sandbox(),
        &PreparedCommand {
            argv,
            env,
            cwd: None,
            inputs: &[],
            outputs: &[],
            resources: &ResourceRequest::default(),
            redirect: crate::local::Redirect::default(),
            timeout_ms: None,
            success_exit_codes: &[0],
            lease: None,
        },
        workspace,
        &CacheProvider::open_local(workspace.join("cache")),
        false,
    )
    .await
}

#[tokio::test]
async fn selected_path_executes_test_instead_of_shell_builtin() {
    let workspace = tempfile::tempdir().unwrap();
    let executable = workspace.path().join("test");
    std::fs::write(
        &executable,
        b"#!/bin/sh\nprintf 'out\\000\\377'\nprintf 'err\\376' >&2\nexit 7\n",
    )
    .unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
    let env = BTreeMap::from([("PATH".into(), workspace.path().display().to_string())]);
    let (url, server) = server::spawn_executing(vec![
        Response::json("EXECUTE"),
        Response::json("CAPTURE"),
        Response::json("CAPTURE"),
    ])
    .await;
    let result = test_server::assert_ready(Box::pin(capture(
        &client(url),
        workspace.path(),
        &["test".into()],
        &env,
    )))
    .await
    .unwrap();
    assert_eq!(result.exit_code, 7);
    let cache = CacheProvider::open_local(workspace.path().join("cache"));
    for (digest, expected) in [
        (result.stdout.unwrap(), b"out\0\xff".as_slice()),
        (result.stderr.unwrap(), b"err\xfe".as_slice()),
    ] {
        assert_eq!(
            cache
                .read_blob_limited(&digest, 100, false)
                .await
                .unwrap()
                .0,
            expected
        );
    }
    let requests = test_server::assert_ready(server).await.unwrap();
    let process: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(process["env"], serde_json::json!({}));
}

#[tokio::test]
async fn selected_environment_and_literal_argv_survive_provider_and_shell_decoration() {
    let workspace = tempfile::tempdir().unwrap();
    let env = BTreeMap::from([
        ("PWD".into(), "selected/not-the-working-directory".into()),
        ("HOME".into(), "/selected/home".into()),
        ("USER".into(), "selected-user".into()),
        ("LOGNAME".into(), "selected-login".into()),
        (
            "non-shell.key".into(),
            "value with ' quotes\nand newline".into(),
        ),
        ("EMPTY".into(), String::new()),
        ("PYTHONHOME".into(), "/not/a/python/home".into()),
    ]);
    let argv = vec![
        "python3".into(),
        "-I".into(),
        "-c".into(),
        "import json, os, sys; keys = json.loads(sys.argv[1]); print(json.dumps([sys.argv[2:], {k: os.environ.get(k) for k in keys}]))".into(),
        serde_json::to_string(&env.keys().collect::<Vec<_>>()).unwrap(),
        String::new(),
        "a'b ; $(exit 9)\n\"literal\" \\".into(),
        "--flag=value".into(),
    ];
    let (url, server) = server::spawn_executing(vec![
        Response::json("EXECUTE"),
        Response::json("CAPTURE"),
        Response::json("CAPTURE"),
    ])
    .await;
    let result = test_server::assert_ready(Box::pin(capture(
        &client(url),
        workspace.path(),
        &argv,
        &env,
    )))
    .await
    .unwrap();
    assert_eq!(result.exit_code, 0);
    let cache = CacheProvider::open_local(workspace.path().join("cache"));
    let stdout = cache
        .read_blob_limited(&result.stdout.unwrap(), 4096, false)
        .await
        .unwrap()
        .0;
    let observed: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(observed, serde_json::json!([&argv[5..], env]));
    assert!(cache
        .read_blob_limited(&result.stderr.unwrap(), 100, false)
        .await
        .unwrap()
        .0
        .is_empty());
    let requests = test_server::assert_ready(server).await.unwrap();
    let process: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(process["env"], serde_json::json!({}));
}

#[tokio::test]
async fn binary_http_captures_use_remaining_combined_allowance_not_archive_cap() {
    for (stdout_len, stderr_len, allowance, succeeds) in [
        (5_usize, 3_usize, 8_usize, true),
        (128 * 1024 + 7, 64 * 1024, 256 * 1024, true),
        (5, 4, 8, false),
        (9, 0, 8, false),
    ] {
        let stdout = (0..stdout_len)
            .map(|n| u8::try_from(n % 256).unwrap())
            .collect::<Vec<_>>();
        let stderr = (0..stderr_len)
            .map(|n| u8::try_from(255 - n % 256).unwrap())
            .collect::<Vec<_>>();
        let mut replies = vec![completed(0), bytes(&stdout)];
        if stdout_len <= allowance {
            replies.push(bytes(&stderr));
        }
        let (url, server) = server::spawn(replies).await;
        let mut client = client(url);
        client.limits.max_archive_bytes = 4;
        client.limits.max_capture_bytes = allowance as u64;
        let workspace = tempfile::tempdir().unwrap();
        let result = test_server::assert_ready(Box::pin(capture(
            &client,
            workspace.path(),
            &["unused".into()],
            &BTreeMap::new(),
        )))
        .await;
        if succeeds {
            let result = result.unwrap();
            let cache = CacheProvider::open_local(workspace.path().join("cache"));
            for (digest, expected) in [
                (result.stdout.unwrap(), &stdout),
                (result.stderr.unwrap(), &stderr),
            ] {
                assert_eq!(
                    cache
                        .read_blob_limited(&digest, allowance as u64, false)
                        .await
                        .unwrap()
                        .0,
                    *expected
                );
            }
        } else {
            assert!(matches!(result, Err(Error::ExecutorResourceLimit { .. })));
        }
        test_server::assert_ready(server).await.unwrap();
    }
}
