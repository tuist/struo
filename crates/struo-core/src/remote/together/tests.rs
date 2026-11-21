use super::*;
use crate::remote::lifecycle::{Journal, Provider};
use crate::remote::test_server::{self, Response};
use crate::{Executor, ResourceRequest, WorkspacePath};

#[cfg(target_os = "linux")]
mod hard_links;

fn exited(code: i32) -> Response {
    Response::json(&format!(
        r#"{{"id":"exec-1","status":"EXITED","exitCode":{code}}}"#
    ))
}

fn bytes(body: &[u8]) -> Response {
    Response {
        body: body.to_vec(),
        ..Response::empty(200)
    }
}

fn created(guest: &str) -> Response {
    Response::json(&format!(
        r#"{{"id":"sandbox-1","status":"running","agent":{{"url":"{guest}","token":"agent-key"}}}}"#
    ))
}

async fn execute(
    client: Client,
    workspace: &Path,
    outputs: &[WorkspacePath],
    lease: Option<ExecutionPermit>,
    timeout: Option<u64>,
) -> Result<ActionResult> {
    let mut remote = RemoteExecution::provider("test-fleet");
    remote.environment = Some("snapshot-id".into());
    let cache = CacheProvider::open_local(workspace.join("cache"));
    let argv = vec!["node".into(), "a b;$HOME".into()];
    let env = BTreeMap::from([("SELECTED".into(), "value".into())]);
    let inputs = vec![WorkspacePath::try_from("node_modules").unwrap()];
    let cwd = WorkspacePath::try_from("tools/tests").unwrap();
    let resources = ResourceRequest::default();
    execute_with_client(
        &remote,
        PreparedCommand {
            argv: &argv,
            env: &env,
            cwd: Some(&cwd),
            inputs: &inputs,
            outputs,
            resources: &resources,
            redirect: crate::local::Redirect::default(),
            timeout_ms: timeout,
            success_exit_codes: &[0],
            lease,
        },
        workspace,
        &cache,
        false,
        client,
    )
    .await
}

fn workspace() -> tempfile::TempDir {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::create_dir(workspace.path().join("node_modules")).unwrap();
    std::fs::write(
        workspace.path().join("node_modules/dependency.js"),
        b"module.exports = 42;",
    )
    .unwrap();
    workspace
}

#[tokio::test]
async fn transfers_declared_dependencies_binary_capture_and_outputs() {
    let mut output = Response::tar_file("./report/result.bin", &[0, 255, 128]);
    output.content_type = "application/octet-stream";
    let (guest, guest_server) = test_server::spawn(vec![
        Response::empty(201),
        exited(0),
        exited(0),
        exited(0),
        Response::json(r#"{"id":"exec-1","status":"RUNNING","exitCode":0}"#),
        exited(0),
        bytes(&[0, 255, b'o']),
        bytes(&[128, b'e']),
        exited(0),
        output,
    ])
    .await;
    let (url, control_server) = test_server::spawn(vec![
        created(&guest),
        Response::json(r#"{"id":"sandbox-1","status":"terminated"}"#),
    ])
    .await;
    let workspace = workspace();
    let cache = CacheProvider::open_local(workspace.path().join("cache"));
    let result = execute(
        Client::new(url, "test-key".into()).unwrap(),
        workspace.path(),
        &[WorkspacePath::try_from("report").unwrap()],
        None,
        None,
    )
    .await
    .unwrap();
    let requests = test_server::assert_ready(guest_server).await.unwrap();
    let control = test_server::assert_ready(control_server).await.unwrap();

    assert_eq!(result.exit_code, 0);
    assert_eq!(
        cache.get_blob(&result.stdout.unwrap()).await.unwrap(),
        [0, 255, b'o']
    );
    assert_eq!(
        cache.get_blob(&result.stderr.unwrap()).await.unwrap(),
        [128, b'e']
    );
    assert_eq!(
        std::fs::read(workspace.path().join("report/result.bin")).unwrap(),
        [0, 255, 128]
    );
    let mut archive = tar::Archive::new(&requests[0].body[..]);
    let paths: Vec<_> = archive
        .entries()
        .unwrap()
        .map(|entry| entry.unwrap().path().unwrap().into_owned())
        .collect();
    assert!(paths
        .iter()
        .any(|path| path == Path::new("node_modules/dependency.js")));
    let invocation: serde_json::Value = serde_json::from_slice(&requests[4].body).unwrap();
    assert_eq!(invocation["command"], "sh");
    assert_eq!(invocation["args"][4], "a b;$HOME");
    assert_eq!(invocation["cwd"], format!("{ROOT}/tools/tests"));
    assert_eq!(invocation["env"], serde_json::json!({"SELECTED":"value"}));
    let create: serde_json::Value = serde_json::from_slice(&control[0].body).unwrap();
    assert_eq!(create["snapshot_id"], "snapshot-id");
    assert_eq!(create["ttl"], 600);
    assert!(create["tags"]["struo-operation"].is_string());
    assert!(create.get("termination_policy").is_none());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&control[1].body).unwrap(),
        serde_json::json!({"snapshot":null})
    );
    assert!(crate::remote::lifecycle::tests::records(workspace.path()).is_empty());
}

#[tokio::test]
async fn failures_do_not_install_outputs_and_still_terminate() {
    let (guest, guest_server) = test_server::spawn(vec![
        Response::empty(201),
        exited(0),
        exited(0),
        exited(0),
        exited(7),
        bytes(b"out"),
        bytes(b"err"),
    ])
    .await;
    let (url, server) = test_server::spawn(vec![
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
    assert_eq!(result.exit_code, 7);
    assert!(!workspace.path().join("report").exists());
    test_server::assert_ready(guest_server).await.unwrap();
    assert_eq!(
        test_server::assert_ready(server).await.unwrap()[1].path,
        "/v1/sandboxes/sandbox-1/terminate"
    );
}

#[tokio::test]
async fn cancellation_holds_admission_until_termination_is_confirmed() {
    let resources = test_server::single_remote_quota();
    let lease = resources
        .acquire(Executor::Together, &ResourceRequest::default())
        .await;
    let (upload, upload_hold) = Response::empty(201).held();
    let (guest, guest_server) = test_server::spawn(vec![upload]).await;
    let (delete, delete_hold) =
        Response::json(r#"{"id":"sandbox-1","status":"terminated"}"#).held();
    let (url, server) = test_server::spawn(vec![created(&guest), delete]).await;
    let workspace = workspace();
    let mut execution = Box::pin(execute(
        Client::new(url, "test-key".into()).unwrap(),
        workspace.path(),
        &[],
        Some(lease),
        None,
    ));
    test_server::assert_ready(async {
        tokio::select! { _ = &mut execution => panic!("execution finished early"), _ = upload_hold.arrived => {} }
    }).await;
    drop(execution);
    test_server::assert_ready(delete_hold.arrived)
        .await
        .unwrap();
    test_server::assert_pending(resources.acquire(Executor::Together, &ResourceRequest::default()))
        .await;
    delete_hold.release.send(()).unwrap();
    upload_hold.release.send(()).unwrap();
    test_server::assert_ready(server).await.unwrap();
    test_server::assert_ready(guest_server).await.unwrap();
    test_server::assert_ready(resources.acquire(Executor::Together, &ResourceRequest::default()))
        .await;
}

#[tokio::test]
async fn lost_create_is_recovered_only_by_returned_marker() {
    use crate::remote::lifecycle::tests::{intent, records, rescope};
    let workspace = workspace();
    let journal = Journal::new(workspace.path());
    let nonce = intent(&journal);
    let list = serde_json::json!({"data":[{"id":"lost", "status":"running", "tags":{"struo-operation":nonce}}],"next_cursor":null});
    let (url, server) = test_server::spawn(vec![
        Response::json(&list.to_string()),
        Response::json(r#"{"id":"lost","status":"terminating"}"#),
        Response::json(r#"{"id":"lost","status":"terminated"}"#),
    ])
    .await;
    let client = Client::new(url, "test-key".into()).unwrap();
    rescope(
        workspace.path(),
        "together",
        &client.scope(),
        &client.credential(),
    );
    client.reconcile(&journal).await;
    let requests = test_server::assert_ready(server).await.unwrap();
    assert!(requests[0].path.contains("tags%5Bstruo-operation%5D="));
    assert_eq!(requests[1].path, "/v1/sandboxes/lost/terminate");
    assert_eq!(requests[2].path, "/v1/sandboxes/lost/wait");
    assert!(records(workspace.path()).is_empty());
}

#[tokio::test]
async fn uncertain_create_is_not_retried_and_retains_its_intent() {
    let (url, server) = test_server::spawn(vec![Response::empty(502)]).await;
    let workspace = workspace();
    let result = execute(
        Client::new(url, "test-key".into()).unwrap(),
        workspace.path(),
        &[],
        None,
        None,
    )
    .await;
    assert!(matches!(
        result,
        Err(crate::Error::RemoteProviderStatus { status: 502, .. })
    ));
    assert_eq!(test_server::assert_ready(server).await.unwrap().len(), 1);
    let records = crate::remote::lifecycle::tests::records(workspace.path());
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["phase"], "intent");
}

#[tokio::test]
async fn foreign_and_truncated_listings_cannot_prove_absence() {
    for list in [
        serde_json::json!({"data":[{"id":"foreign", "status":"running", "tags":{"struo-operation":"another-run"}}],"next_cursor":null}),
        serde_json::json!({"data":[],"next_cursor":"more"}),
    ] {
        let (url, server) = test_server::spawn(vec![Response::json(&list.to_string())]).await;
        let client = Client::new(url, "test-key".into()).unwrap();
        assert_eq!(
            client.find("our-run").await.unwrap(),
            crate::remote::lifecycle::Lookup::Inconclusive
        );
        assert_eq!(test_server::assert_ready(server).await.unwrap().len(), 1);
    }
}

#[tokio::test]
async fn timeout_during_upload_still_terminates() {
    let (upload, upload_hold) = Response::empty(201).held();
    let (guest, guest_server) = test_server::spawn(vec![upload]).await;
    let (url, server) = test_server::spawn(vec![
        created(&guest),
        Response::json(r#"{"id":"sandbox-1","status":"terminated"}"#),
    ])
    .await;
    let workspace = workspace();
    let mut execution = Box::pin(execute(
        Client::new(url, "test-key".into()).unwrap(),
        workspace.path(),
        &[],
        None,
        Some(crate::remote::phase::testing::BUDGET_MS),
    ));
    test_server::assert_ready(async {
        tokio::select! { _ = &mut execution => panic!("execution finished early"), _ = upload_hold.arrived => {} }
    }).await;
    crate::remote::phase::testing::spend_budget().await;
    let result = test_server::assert_ready(execution).await;
    assert!(matches!(result, Err(crate::Error::Timeout(_))));
    upload_hold.release.send(()).unwrap();
    test_server::assert_ready(guest_server).await.unwrap();
    assert_eq!(
        test_server::assert_ready(server).await.unwrap()[1].path,
        "/v1/sandboxes/sandbox-1/terminate"
    );
}

#[tokio::test]
async fn oversized_or_text_encoded_downloads_fail_closed_and_terminate() {
    for response in [
        bytes(b"12345"),
        Response::json(r#"{"content":"AP8=","encoding":"base64"}"#),
    ] {
        let (guest, guest_server) = test_server::spawn(vec![
            Response::empty(201),
            exited(0),
            exited(0),
            exited(0),
            exited(0),
            response,
        ])
        .await;
        let (url, server) = test_server::spawn(vec![
            created(&guest),
            Response::json(r#"{"id":"sandbox-1","status":"terminated"}"#),
        ])
        .await;
        let resources =
            test_server::single_remote_quota().with_remote_io_limits(crate::RemoteIoLimits {
                max_capture_bytes: 4,
                ..crate::RemoteIoLimits::default()
            });
        let lease = resources
            .acquire(Executor::Together, &ResourceRequest::default())
            .await;
        let workspace = workspace();
        let result = execute(
            Client::new(url, "test-key".into()).unwrap(),
            workspace.path(),
            &[],
            Some(lease),
            None,
        )
        .await;
        assert!(matches!(
            result,
            Err(crate::Error::ExecutorResourceLimit { .. } | crate::Error::RemoteProviderApi { .. })
        ));
        test_server::assert_ready(guest_server).await.unwrap();
        assert_eq!(
            test_server::assert_ready(server).await.unwrap()[1].path,
            "/v1/sandboxes/sandbox-1/terminate"
        );
        assert!(crate::remote::lifecycle::tests::records(workspace.path()).is_empty());
    }
}
