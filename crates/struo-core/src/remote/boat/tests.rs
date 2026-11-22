use std::collections::BTreeMap;
use std::io::Read;

use super::*;
use crate::remote::lifecycle::{self, tests::records, Budget, Journal, Provider};
use crate::remote::test_server::{self, Response};
use crate::{ExecutionPermit, Executor, ResourceRequest};

const READY: &str = r#"{"sandbox":{"id":"bx_23456789","state":"ready"}}"#;
const STARTED: &str = r#"{"processId":42}"#;
const EXITED: &str = r#"{"status":"exited","running":false,"exitCode":0}"#;
const DELETED: &str = r#"{"operation":{"id":"bdop_0123456789abcdef0123456789abcdef","kind":"sandbox","targetId":"bx_23456789","status":"pending"}}"#;

fn client(url: String) -> Client {
    Client::new(Config {
        url,
        api_key: "test-key".into(),
    })
    .unwrap()
}

fn file_url(url: &str, method: &str) -> Response {
    Response::json(
        &serde_json::json!({"url":format!("{url}/binary?token=secret"),"method":method})
            .to_string(),
    )
}

async fn run(
    client: Client,
    workspace: &Path,
    lease: Option<ExecutionPermit>,
    timeout_ms: Option<u64>,
    outputs: &[WorkspacePath],
) -> Result<ActionResult> {
    let cache = CacheProvider::open_local(workspace.join("cache"));
    let argv = vec![
        "node".into(),
        "node_modules/runner.mjs".into(),
        "literal ' ; $HOME".into(),
    ];
    let inputs = [WorkspacePath::try_from("node_modules").unwrap()];
    let env = BTreeMap::from([("VALUE".into(), "value ' ; $HOME".into())]);
    let resources = ResourceRequest::default();
    let cwd = WorkspacePath::try_from("tests").unwrap();
    let mut remote = RemoteExecution::provider("boat");
    remote.environment = Some("node-tests-v1".into());
    Box::pin(execute_with_client(
        &remote,
        PreparedCommand {
            lease,
            argv: &argv,
            env: &env,
            cwd: Some(&cwd),
            inputs: &inputs,
            outputs,
            resources: &resources,
            redirect: crate::local::Redirect::default(),
            timeout_ms,
            success_exit_codes: &[0],
        },
        workspace,
        &cache,
        false,
        client,
    ))
    .await
}

fn workspace() -> tempfile::TempDir {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::create_dir(workspace.path().join("node_modules")).unwrap();
    std::fs::write(
        workspace.path().join("node_modules/runner.mjs"),
        b"dependency\0\xff",
    )
    .unwrap();
    std::fs::write(workspace.path().join("undeclared"), "secret").unwrap();
    workspace
}

#[tokio::test]
async fn creates_private_sandboxes_with_and_without_a_snapshot() {
    for snapshot in [Some("node-tests-v1"), None] {
        let (url, server) = test_server::spawn(vec![Response::json(READY)]).await;
        client(url)
            .create(snapshot, std::time::Duration::from_secs(3600), None)
            .await
            .unwrap();
        let requests = test_server::assert_ready(server).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(body["visibility"], "private");
    }
}

#[tokio::test]
async fn transfers_declared_dependencies_captures_binary_streams_and_installs_outputs() {
    for exit in [0, 7] {
        let (url, server) = test_server::spawn_with(|url| {
            let mut responses = vec![
                Response::json(READY),
                file_url(url, "PUT"),
                Response::empty(200),
                Response::json(STARTED),
                Response::json(EXITED),
                Response::json(STARTED),
                Response::json(r#"{"status":"running","running":true,"exitCode":0}"#),
                Response::json(&format!(
                    r#"{{"status":"exited","running":false,"exitCode":{exit}}}"#
                )),
                file_url(url, "GET"),
                Response {
                    body: b"stdout\0\xff".to_vec(),
                    ..Response::empty(200)
                },
                file_url(url, "GET"),
                Response {
                    body: b"stderr\xfe".to_vec(),
                    ..Response::empty(200)
                },
            ];
            if exit == 0 {
                responses.extend([
                    Response::json(STARTED),
                    Response::json(EXITED),
                    file_url(url, "GET"),
                    Response::tar_file("./reports/result", b"passed\0\xff"),
                ]);
            }
            responses.push(Response::json(DELETED));
            responses
        })
        .await;
        let workspace = workspace();
        let outputs = [WorkspacePath::try_from("reports").unwrap()];
        let result =
            test_server::assert_ready(run(client(url), workspace.path(), None, None, &outputs))
                .await
                .unwrap();
        let requests = test_server::assert_ready(server).await.unwrap();
        let cache = CacheProvider::open_local(workspace.path().join("cache"));
        assert_eq!(result.exit_code, exit);
        assert_eq!(
            cache.get_blob(&result.stdout.unwrap()).await.unwrap(),
            b"stdout\0\xff"
        );
        assert_eq!(
            cache.get_blob(&result.stderr.unwrap()).await.unwrap(),
            b"stderr\xfe"
        );
        assert_eq!(workspace.path().join("reports/result").exists(), exit == 0);
        if exit == 0 {
            assert_eq!(
                std::fs::read(workspace.path().join("reports/result")).unwrap(),
                b"passed\0\xff"
            );
        }
        let create: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(create["from"], "node-tests-v1");
        assert_eq!(create["noEnv"], true);
        assert_eq!(create["snapshots"], false);
        assert_eq!(create["monitoring"], false);
        assert_eq!(create["ttlSeconds"], 3600);
        assert_eq!(
            create["labels"]["struo-operation"],
            requests[0].headers["idempotency-key"]
        );
        let mut archive = tar::Archive::new(requests[2].body.as_slice());
        let mut dependency = None;
        for entry in archive.entries().unwrap() {
            let mut entry = entry.unwrap();
            let path = entry.path().unwrap().to_string_lossy().into_owned();
            assert!(!path.contains("undeclared"));
            if path.ends_with("runner.mjs") {
                let mut bytes = Vec::new();
                entry.read_to_end(&mut bytes).unwrap();
                dependency = Some(bytes);
            }
        }
        assert_eq!(dependency.unwrap(), b"dependency\0\xff");
        let executed: serde_json::Value = serde_json::from_slice(&requests[5].body).unwrap();
        let command = executed["command"].as_str().unwrap();
        assert_eq!(executed["detached"], true);
        assert!(command.contains("cd '/home/user/struo-workspace/tests'"));
        for request in &requests {
            if request.path.starts_with("/binary") {
                assert!(!request.headers.contains_key("authorization"));
            } else {
                assert_eq!(request.headers["authorization"], "Bearer test-key");
            }
        }
        let deletion = requests.last().unwrap();
        assert_eq!(deletion.method, "DELETE");
        assert_eq!(deletion.path, "/sandboxes/bx_23456789");
        assert_eq!(deletion.headers["x-ascii-confirm-delete"], "bx_23456789");
        assert!(records(workspace.path()).is_empty());
    }
}

#[tokio::test]
async fn failed_transfer_deletes_and_failed_deletion_keeps_ownership() {
    for deletion_status in [200, 403, 404] {
        let (url, server) = test_server::spawn(vec![
            Response::json(READY),
            Response::empty(400),
            if deletion_status == 200 {
                Response::json(DELETED)
            } else {
                Response::empty(deletion_status)
            },
        ])
        .await;
        let workspace = workspace();
        let error = test_server::assert_ready(run(client(url), workspace.path(), None, None, &[]))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("400"));
        let requests = test_server::assert_ready(server).await.unwrap();
        assert_eq!(requests.last().unwrap().method, "DELETE");
        assert_eq!(
            records(workspace.path()).len(),
            usize::from(deletion_status != 200)
        );
    }
}

#[tokio::test]
async fn cancelled_create_holds_admission_until_permanent_delete_is_accepted() {
    let resources = test_server::single_remote_quota();
    let lease = resources
        .acquire(Executor::Boat, &ResourceRequest::default())
        .await;
    let (created, create_hold) = Response::json(READY).held();
    let (deleted, delete_hold) = Response::json(DELETED).held();
    let (url, server) = test_server::spawn(vec![created, deleted]).await;
    let workspace = workspace();
    let mut execution = Box::pin(run(client(url), workspace.path(), Some(lease), None, &[]));
    test_server::assert_ready(async { tokio::select! { _ = &mut execution => panic!("create was held"), _ = create_hold.arrived => {} } }).await;
    drop(execution);
    test_server::assert_pending(resources.acquire(Executor::Boat, &ResourceRequest::default()))
        .await;
    create_hold.release.send(()).unwrap();
    test_server::assert_ready(delete_hold.arrived)
        .await
        .unwrap();
    test_server::assert_pending(resources.acquire(Executor::Boat, &ResourceRequest::default()))
        .await;
    delete_hold.release.send(()).unwrap();
    test_server::assert_ready(server).await.unwrap();
    test_server::assert_ready(resources.acquire(Executor::Boat, &ResourceRequest::default())).await;
    assert!(records(workspace.path()).is_empty());
}

#[tokio::test]
async fn uncertain_create_is_not_retried_and_is_recovered_by_exact_label() {
    use crate::remote::lifecycle::tests::{intent, rescope};
    let workspace = workspace();
    let (url, server) = test_server::spawn(vec![Response::empty(500)]).await;
    assert!(run(client(url), workspace.path(), None, None, &[])
        .await
        .is_err());
    assert_eq!(test_server::assert_ready(server).await.unwrap().len(), 1);
    assert_eq!(records(workspace.path())[0]["phase"], "intent");

    let workspace = tempfile::tempdir().unwrap();
    let journal = Journal::new(workspace.path());
    let nonce = intent(&journal);
    let listed = serde_json::json!({ "sandboxes": [{ "id": "bx_23456789", "state": "archived", "labels": { "struo-operation": nonce } }], "pageInfo": { "hasMore": false } });
    let (url, server) = test_server::spawn(vec![
        Response::json(&listed.to_string()),
        Response::json(DELETED),
    ])
    .await;
    let client = client(url);
    rescope(
        workspace.path(),
        "boat",
        &client.scope(),
        &client.credential(),
    );
    let report = lifecycle::reconcile(&journal, &client, Budget::default()).await;
    let requests = test_server::assert_ready(server).await.unwrap();
    assert!(requests[0]
        .path
        .contains(&format!("label=struo-operation%3D{nonce}")));
    assert_eq!(report.deleted, 1);
    assert!(records(workspace.path()).is_empty());
}

#[tokio::test]
async fn truncated_and_foreign_listings_do_not_prove_absence() {
    for body in [
        r#"{"sandboxes":[],"pageInfo":{"hasMore":true}}"#,
        r#"{"sandboxes":[{"id":"bx_23456789","state":"ready","labels":{}}],"pageInfo":{"hasMore":false}}"#,
    ] {
        let (url, server) = test_server::spawn(vec![Response::json(body)]).await;
        assert_eq!(
            client(url).find("not-ours").await.unwrap(),
            lifecycle::Lookup::Inconclusive
        );
        test_server::assert_ready(server).await.unwrap();
    }
}

#[tokio::test]
async fn empty_wallet_listing_retains_settled_intents_and_known_sandboxes() {
    use crate::remote::lifecycle::tests::{intent, mark_created, rescope};
    for known in [false, true] {
        let workspace = tempfile::tempdir().unwrap();
        let journal = Journal::new(workspace.path());
        let nonce = intent(&journal);
        if known {
            mark_created(workspace.path(), "bx_23456789");
        }
        let (url, server) = test_server::spawn(vec![Response::json(
            r#"{"sandboxes":[],"pageInfo":{"hasMore":false}}"#,
        )])
        .await;
        let client = client(url);
        rescope(
            workspace.path(),
            "boat",
            &client.scope(),
            &client.credential(),
        );
        let report = lifecycle::reconcile(&journal, &client, Budget::default()).await;
        test_server::assert_ready(server).await.unwrap();
        assert_eq!(report.absent, 0);
        assert_eq!(report.retained, 1);
        assert_eq!(records(workspace.path())[0]["nonce"], nonce);
    }
}

#[cfg(unix)]
async fn replay_captured_action(
    workspace: &Path,
    argv: &[String],
    env: &BTreeMap<String, String>,
) -> std::process::Output {
    let (url, server) = test_server::spawn_with(|url| {
        vec![
            Response::json(STARTED),
            Response::json(EXITED),
            file_url(url, "GET"),
            Response::empty(200),
            file_url(url, "GET"),
            Response::empty(200),
        ]
    })
    .await;
    let sandbox = serde_json::from_value(
        serde_json::from_str::<serde_json::Value>(READY).unwrap()["sandbox"].clone(),
    )
    .unwrap();
    let cache = CacheProvider::open_local(workspace.join("cache"));
    Box::pin(capture::execute(
        &client(url),
        &sandbox,
        &PreparedCommand {
            lease: None,
            argv,
            env,
            cwd: None,
            inputs: &[],
            outputs: &[],
            resources: &ResourceRequest::default(),
            redirect: crate::local::Redirect::default(),
            timeout_ms: None,
            success_exit_codes: &[0],
        },
        workspace,
        &cache,
        false,
    ))
    .await
    .unwrap();
    let requests = test_server::assert_ready(server).await.unwrap();
    let request: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    let stdout = workspace.join("stdout");
    let stderr = workspace.join("stderr");
    let command = request["command"]
        .as_str()
        .unwrap()
        .replace(GUEST_ROOT, workspace.to_str().unwrap())
        .replace("/tmp/struo-stdout", stdout.to_str().unwrap())
        .replace("/tmp/struo-stderr", stderr.to_str().unwrap());
    let status = tokio::process::Command::new("/bin/sh")
        .args(["-c", &command])
        .env("PATH", "/usr/bin:/bin")
        .status()
        .await
        .unwrap();
    std::process::Output {
        status,
        stdout: std::fs::read(stdout).unwrap(),
        stderr: std::fs::read(stderr).unwrap(),
    }
}

#[cfg(unix)]
#[tokio::test]
async fn executes_literal_programs_independently_of_the_action_path() {
    use std::os::unix::fs::PermissionsExt;
    let workspace = tempfile::tempdir().unwrap();
    let bin = workspace.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let printf = bin.join("printf");
    std::fs::write(&printf, "#!/bin/sh\nprintf '%s' \"$VALUE\"\n").unwrap();
    std::fs::set_permissions(&printf, std::fs::Permissions::from_mode(0o755)).unwrap();
    let value = "literal ' ; $HOME";
    for (argv, path) in [
        (
            vec![
                "/bin/sh".into(),
                "-c".into(),
                "printf '%s' \"$VALUE\"".into(),
            ],
            "/nonexistent".into(),
        ),
        (
            vec!["printf".into(), "ignored".into()],
            format!("{}:/usr/bin:/bin", bin.display()),
        ),
    ] {
        let env = BTreeMap::from([("PATH".into(), path), ("VALUE".into(), value.into())]);
        let output = replay_captured_action(workspace.path(), &argv, &env).await;
        assert!(output.status.success(), "{argv:?} failed");
        assert_eq!(output.stdout, value.as_bytes());
        assert!(output.stderr.is_empty());
    }
}

#[cfg(unix)]
#[tokio::test]
async fn preserves_shell_sensitive_action_environment() {
    let workspace = tempfile::tempdir().unwrap();
    let mut observed = Vec::new();
    for (key, value) in [("IFS", ":"), ("PWD", "/literal")] {
        let argv = vec!["/usr/bin/printenv".into(), key.into()];
        let env = BTreeMap::from([(key.into(), value.into())]);
        let output = replay_captured_action(workspace.path(), &argv, &env).await;
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        observed.push(output.stdout);
    }
    assert_eq!(observed, [b":\n".to_vec(), b"/literal\n".to_vec()]);
}

#[tokio::test]
async fn missing_exit_code_fails_and_deletes() {
    let (url, server) = test_server::spawn_with(|url| {
        vec![
            Response::json(READY),
            file_url(url, "PUT"),
            Response::empty(200),
            Response::json(STARTED),
            Response::json(r#"{"status":"exited","running":false,"exitCode":null}"#),
            Response::json(DELETED),
        ]
    })
    .await;
    let workspace = workspace();
    let error = test_server::assert_ready(run(client(url), workspace.path(), None, None, &[]))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("exit code"));
    assert_eq!(
        test_server::assert_ready(server)
            .await
            .unwrap()
            .last()
            .unwrap()
            .method,
        "DELETE"
    );
}

#[test]
fn ttl_covers_the_timeout_without_disabling_the_safety_net() {
    assert_eq!(client::lifetime(Some(1001)).unwrap().as_secs(), 302);
    assert_eq!(
        client::lifetime(Some(2_591_700_000)).unwrap().as_secs(),
        2_592_000
    );
    assert!(client::lifetime(Some(2_591_700_001)).is_err());
}

#[tokio::test]
async fn expired_action_budget_still_deletes_the_sandbox() {
    use crate::remote::phase::testing;
    let (upload, mut hold) = Response::empty(200).held();
    let (url, server) = test_server::spawn_with(|url| {
        vec![
            Response::json(READY),
            file_url(url, "PUT"),
            upload,
            Response::json(DELETED),
        ]
    })
    .await;
    let workspace = workspace();
    let mut execution = Box::pin(run(
        client(url),
        workspace.path(),
        None,
        Some(testing::BUDGET_MS),
        &[],
    ));
    test_server::assert_ready(async { tokio::select! { _ = &mut execution => panic!("upload was held"), _ = &mut hold.arrived => {} } }).await;
    testing::spend_budget().await;
    assert!(futures::poll!(&mut execution).is_pending());
    drop(hold.release);
    assert!(matches!(
        test_server::assert_ready(execution).await,
        Err(Error::Timeout(_))
    ));
    assert_eq!(
        test_server::assert_ready(server)
            .await
            .unwrap()
            .last()
            .unwrap()
            .method,
        "DELETE"
    );
}

#[tokio::test]
async fn stdout_and_stderr_share_the_capture_limit() {
    let (url, server) = test_server::spawn_with(|url| {
        vec![
            Response::json(READY),
            file_url(url, "PUT"),
            Response::empty(200),
            Response::json(STARTED),
            Response::json(EXITED),
            Response::json(STARTED),
            Response::json(EXITED),
            file_url(url, "GET"),
            Response {
                body: b"stdout".to_vec(),
                ..Response::empty(200)
            },
            file_url(url, "GET"),
            Response {
                body: b"stderr".to_vec(),
                ..Response::empty(200)
            },
            Response::json(DELETED),
        ]
    })
    .await;
    let resources =
        test_server::single_remote_quota().with_remote_io_limits(crate::RemoteIoLimits {
            max_capture_bytes: 10,
            ..crate::RemoteIoLimits::default()
        });
    let lease = resources
        .acquire(Executor::Boat, &ResourceRequest::default())
        .await;
    let workspace = workspace();
    let error = run(client(url), workspace.path(), Some(lease), None, &[])
        .await
        .unwrap_err();
    assert!(error.to_string().contains("archive bytes"));
    assert_eq!(
        test_server::assert_ready(server)
            .await
            .unwrap()
            .last()
            .unwrap()
            .method,
        "DELETE"
    );
    assert!(records(workspace.path()).is_empty());
}

#[tokio::test]
async fn binary_download_rejects_partial_files_and_overflow() {
    for (status, bytes) in [(206, 1), (200, 5)] {
        let (url, server) = test_server::spawn_with(|url| {
            vec![
                file_url(url, "GET"),
                Response {
                    body: vec![0; bytes],
                    ..Response::empty(status)
                },
            ]
        })
        .await;
        let mut client = client(url);
        client.limits.max_archive_bytes = 4;
        let sandbox = serde_json::from_str::<serde_json::Value>(READY).unwrap()["sandbox"].clone();
        let sandbox = serde_json::from_value(sandbox).unwrap();
        let file = tempfile::NamedTempFile::new().unwrap();
        assert!(client
            .download(&sandbox, "/tmp/outputs", file.path(), None, 1)
            .await
            .is_err());
        test_server::assert_ready(server).await.unwrap();
        assert_eq!(std::fs::metadata(file.path()).unwrap().len(), 0);
    }
}
