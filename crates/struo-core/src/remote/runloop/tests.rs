use super::*;
use crate::remote::test_server::{self, Response};
use crate::{Error, Executor, ResourceRequest, WorkspacePath};
use std::collections::BTreeMap;

mod guest;
mod ownership;
mod resources;

fn sandbox(status: &str) -> Response {
    Response::json(&format!(
        r#"{{"id":"dbx_test","status":"{status}","metadata":{{}}}}"#
    ))
}

fn execution(status: &str, exit: Option<i32>) -> Response {
    Response::json(&serde_json::json!({"devbox_id":"dbx_test", "execution_id":"ex_test", "status":status, "exit_status":exit}).to_string())
}

fn binary(bytes: &[u8]) -> Response {
    let mut response = Response::empty(200);
    response.body = bytes.to_vec();
    response
}

fn client(url: &str) -> Client {
    Client::new(Config {
        url: url.into(),
        key: "test-key".into(),
    })
    .unwrap()
}

async fn run(
    client: Client,
    root: &Path,
    lease: Option<ExecutionPermit>,
    timeout_ms: Option<u64>,
    outputs: &[WorkspacePath],
) -> Result<ActionResult> {
    let cache = CacheProvider::open_local(root.join("cache"));
    let argv = vec![
        "sh".into(),
        "-c".into(),
        "printf '%s|%s' \"$1\" \"$MESSAGE\"".into(),
        "struo-test".into(),
        "literal ; $(false) ' value".into(),
    ];
    let env = BTreeMap::from([("MESSAGE".into(), "quoted ' value".into())]);
    let inputs = [WorkspacePath::try_from("node_modules").unwrap()];
    std::fs::create_dir_all(root.join("node_modules/dep")).unwrap();
    std::fs::write(
        root.join("node_modules/dep/index.js"),
        b"module.exports = 42;\0\xff",
    )
    .unwrap();
    let cwd = WorkspacePath::try_from("node_modules").unwrap();
    let resources = ResourceRequest::default();
    Box::pin(execute_with_client(
        &RemoteExecution::provider("runloop"),
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
        root,
        &cache,
        false,
        client,
    ))
    .await
}

#[tokio::test]
async fn declared_dependencies_binary_capture_and_outputs_roundtrip() {
    let (url, server) = test_server::spawn(vec![
        sandbox("provisioning"),
        sandbox("initializing"),
        sandbox("running"),
        Response::empty(200),
        execution("completed", Some(0)),
        execution("running", Some(0)),
        execution("completed", Some(0)),
        binary(b"out\0\xff"),
        binary(b"err\xfe"),
        execution("completed", Some(0)),
        Response::tar_file("./reports/result.txt", b"passed\0\xff"),
        sandbox("running"),
        sandbox("shutdown"),
    ])
    .await;
    let workspace = tempfile::tempdir().unwrap();
    let outputs = [WorkspacePath::try_from("reports").unwrap()];
    let result =
        test_server::assert_ready(run(client(&url), workspace.path(), None, None, &outputs))
            .await
            .unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(
        result.stdout,
        Some(struo_cas::Digest::from_bytes(
            *blake3::hash(b"out\0\xff").as_bytes()
        ))
    );
    assert_eq!(
        result.stderr,
        Some(struo_cas::Digest::from_bytes(
            *blake3::hash(b"err\xfe").as_bytes()
        ))
    );
    assert_eq!(
        std::fs::read(workspace.path().join("reports/result.txt")).unwrap(),
        b"passed\0\xff"
    );
    let requests = test_server::assert_ready(server).await.unwrap();
    let upload = &requests[3];
    assert_eq!(upload.path, "/devboxes/dbx_test/upload_file");
    assert!(upload
        .body
        .windows(b"module.exports = 42;\0\xff".len())
        .any(|w| w == b"module.exports = 42;\0\xff"));
    assert!(upload
        .body
        .windows(b"node_modules/dep/index.js".len())
        .any(|w| w == b"node_modules/dep/index.js"));
    let body: serde_json::Value = serde_json::from_slice(&requests[5].body).unwrap();
    let command = body["command"].as_str().unwrap();
    let guest = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(guest.path().join("struo-workspace/node_modules")).unwrap();
    let probe = std::process::Command::new("sh")
        .args([
            "-c",
            &command.replace("/home/user", &guest.path().to_string_lossy()),
        ])
        .output()
        .unwrap();
    assert!(probe.status.success());
    let downloaded: serde_json::Value = serde_json::from_slice(&requests[7].body).unwrap();
    assert_eq!(
        std::fs::read(guest.path().join(downloaded["path"].as_str().unwrap())).unwrap(),
        b"literal ; $(false) ' value|quoted ' value"
    );
    assert!(command.contains("/home/user/struo-workspace/node_modules"));
    assert_eq!(
        requests[6].path,
        "/devboxes/dbx_test/executions/ex_test?last_n=0"
    );
    assert_eq!(requests[11].path, "/devboxes/dbx_test/shutdown?force=true");
    assert!(crate::remote::lifecycle::tests::records(workspace.path()).is_empty());
}

#[tokio::test]
async fn nonzero_exit_does_not_restore_outputs() {
    let (url, server) = test_server::spawn(vec![
        sandbox("running"),
        Response::empty(200),
        execution("completed", Some(0)),
        execution("completed", Some(7)),
        binary(b"out"),
        binary(b"err"),
        sandbox("shutdown"),
    ])
    .await;
    let root = tempfile::tempdir().unwrap();
    let result = run(
        client(&url),
        root.path(),
        None,
        None,
        &[WorkspacePath::try_from("reports").unwrap()],
    )
    .await
    .unwrap();
    assert_eq!(result.exit_code, 7);
    assert!(!root.path().join("reports").exists());
    test_server::assert_ready(server).await.unwrap();
}

#[tokio::test]
async fn cancelled_upload_holds_admission_through_confirmed_shutdown() {
    let resources = test_server::single_remote_quota();
    let request = ResourceRequest::default();
    let lease = resources.acquire(Executor::Runloop, &request).await;
    let (upload, upload_hold) = Response::empty(200).held();
    let (shutdown, shutdown_hold) = sandbox("shutdown").held();
    let (url, server) = test_server::spawn(vec![sandbox("running"), upload, shutdown]).await;
    let root = tempfile::tempdir().unwrap();
    let mut execution = Box::pin(run(client(&url), root.path(), Some(lease), None, &[]));
    test_server::assert_ready(async {
        tokio::select! {
            _ = &mut execution => panic!("finished before upload"),
            _ = upload_hold.arrived => {}
        }
    })
    .await;
    drop(execution);
    test_server::assert_pending(resources.acquire(Executor::Runloop, &request)).await;
    drop(upload_hold.release);
    test_server::assert_ready(shutdown_hold.arrived)
        .await
        .unwrap();
    test_server::assert_pending(resources.acquire(Executor::Runloop, &request)).await;
    shutdown_hold.release.send(()).unwrap();
    test_server::assert_ready(server).await.unwrap();
    test_server::assert_ready(resources.acquire(Executor::Runloop, &request)).await;
}

#[tokio::test]
async fn timeout_and_startup_failure_still_shutdown() {
    let (url, server) = test_server::spawn(vec![sandbox("failure"), sandbox("shutdown")]).await;
    let root = tempfile::tempdir().unwrap();
    assert!(run(client(&url), root.path(), None, None, &[])
        .await
        .unwrap_err()
        .to_string()
        .contains("failure"));
    test_server::assert_ready(server).await.unwrap();
    let (upload, hold) = Response::empty(200).held();
    let (url, server) =
        test_server::spawn(vec![sandbox("running"), upload, sandbox("shutdown")]).await;
    let root = tempfile::tempdir().unwrap();
    let mut future = Box::pin(run(client(&url), root.path(), None, Some(1000), &[]));
    test_server::assert_ready(async {
        tokio::select! { _ = &mut future => panic!("finished early"), _ = hold.arrived => {} }
    })
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    assert!(futures::poll!(&mut future).is_pending());
    drop(hold.release);
    assert!(matches!(
        test_server::assert_ready(future).await,
        Err(Error::Timeout(_))
    ));
    test_server::assert_ready(server).await.unwrap();
}
