use std::collections::BTreeMap;
use std::io::Read;

use super::*;
use crate::remote::lifecycle::{
    tests::{intent, records, rescope},
    Journal, Lookup,
};
use crate::remote::test_server::{self, Response};
use crate::{Executor, ResourceRequest, WorkspacePath};

#[cfg(unix)]
mod capture;
mod failures;
mod outputs;
mod resources;
mod server;

fn client(url: String) -> Client {
    Client::new(Config::for_test(url)).unwrap()
}
fn sandbox() -> client::Sandbox {
    serde_json::from_value(
        serde_json::json!({ "metadata": {"name": "test"}, "status": "DEPLOYED" }),
    )
    .unwrap()
}
fn completed(code: i32) -> Response {
    Response::json(&format!(
        r#"{{"status":"{}","exitCode":{code},"pid":"123"}}"#,
        if code == 0 { "completed" } else { "failed" }
    ))
}
fn bytes(data: &[u8]) -> Response {
    let mut response = Response::empty(200);
    response.body = data.to_vec();
    response
}
fn upload_responses() -> Vec<Response> {
    vec![
        Response::json(r#"{"uploadId":"upload-1"}"#),
        Response::json(r#"{"partNumber":1,"etag":"part-1"}"#),
        Response::empty(200),
    ]
}

async fn run(
    url: String,
    workspace: &Path,
    lease: Option<ExecutionPermit>,
    timeout_ms: Option<u64>,
) -> Result<ActionResult> {
    let cache = CacheProvider::open_local(workspace.join("cache"));
    let argv = vec!["node".into(), "a'b ; $(exit 9)".into()];
    let env = BTreeMap::from([("TEST_VALUE".into(), "a'b".into())]);
    let inputs = vec![WorkspacePath::try_from("node_modules").unwrap()];
    let outputs = vec![WorkspacePath::try_from("reports").unwrap()];
    let cwd = WorkspacePath::try_from("node_modules").unwrap();
    let resources = ResourceRequest::default();
    execute_with_client(
        &RemoteExecution::provider("blaxel"),
        PreparedCommand {
            argv: &argv,
            env: &env,
            cwd: Some(&cwd),
            inputs: &inputs,
            outputs: &outputs,
            resources: &resources,
            redirect: crate::local::Redirect::default(),
            timeout_ms,
            success_exit_codes: &[0],
            lease,
        },
        workspace,
        &cache,
        false,
        client(url),
    )
    .await
}

fn workspace() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("node_modules")).unwrap();
    std::fs::write(
        dir.path().join("node_modules/dependency.js"),
        b"module.exports = 42;",
    )
    .unwrap();
    std::fs::write(dir.path().join("undeclared"), b"private").unwrap();
    dir
}

#[tokio::test]
async fn declared_dependencies_binary_captures_and_outputs_roundtrip() {
    for code in [0, 7] {
        let mut replies = vec![Response::json("CREATE"), Response::empty(200)];
        replies.extend(upload_responses());
        replies.extend([
            completed(0),
            Response::json(r#"{"status":"running","exitCode":0,"pid":"123"}"#),
            completed(code),
            bytes(b"out\0\xff"),
            bytes(b"err\xfe"),
        ]);
        if code == 0 {
            replies.extend([
                completed(0),
                Response::tar_file("./reports/result", b"passed\0"),
            ]);
        }
        replies.extend([Response::empty(200), Response::empty(404)]);
        let (url, server) = server::spawn(replies).await;
        let workspace = workspace();
        let result = test_server::assert_ready(Box::pin(run(url, workspace.path(), None, None)))
            .await
            .unwrap();
        let requests = test_server::assert_ready(server).await.unwrap();
        assert_eq!(result.exit_code, code);
        let cache = CacheProvider::open_local(workspace.path().join("cache"));
        assert_eq!(
            cache
                .read_blob_limited(&result.stdout.unwrap(), 100, false)
                .await
                .unwrap()
                .0,
            b"out\0\xff"
        );
        assert_eq!(
            cache
                .read_blob_limited(&result.stderr.unwrap(), 100, false)
                .await
                .unwrap()
                .0,
            b"err\xfe"
        );
        assert_eq!(workspace.path().join("reports/result").exists(), code == 0);
        if code == 0 {
            assert_eq!(
                std::fs::read(workspace.path().join("reports/result")).unwrap(),
                b"passed\0"
            );
        }
        for request in &requests {
            assert_eq!(request.headers["authorization"], "Bearer test-key");
            assert_eq!(request.headers["x-blaxel-workspace"], "tests");
        }
        let create: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(create["spec"]["runtime"]["ttl"], "720s");
        assert_eq!(create["spec"]["runtime"]["memory"], 2048);
        assert!(requests[2]
            .path
            .ends_with("filesystem-multipart/initiate/%2Ftmp%2Fstruo-inputs.tar"));
        let upload = &requests[3].body;
        let start = upload.windows(4).position(|s| s == b"\r\n\r\n").unwrap() + 4;
        let mut archive = tar::Archive::new(&upload[start..]);
        let mut paths = Vec::new();
        for entry in archive.entries().unwrap() {
            let mut entry = entry.unwrap();
            let path = entry.path().unwrap().to_string_lossy().to_string();
            if path.ends_with("dependency.js") {
                let mut content = Vec::new();
                entry.read_to_end(&mut content).unwrap();
                assert_eq!(content, b"module.exports = 42;");
            }
            paths.push(path);
        }
        assert!(paths.iter().any(|p| p.ends_with("dependency.js")));
        assert!(!paths.iter().any(|p| p.contains("undeclared")));
        let execute: serde_json::Value = serde_json::from_slice(&requests[6].body).unwrap();
        assert_eq!(execute["workingDir"], "/workspace/struo/node_modules");
        assert_eq!(execute["env"], serde_json::json!({}));
        assert!(execute["command"]
            .as_str()
            .unwrap()
            .contains("a'\\''b ; $(exit 9)"));
        assert_eq!(requests[7].method, "GET");
        assert!(requests[7].path.ends_with("process/123"));
        assert_eq!(requests[requests.len() - 2].method, "DELETE");
        assert!(records(workspace.path()).is_empty());
    }
}

#[tokio::test]
async fn uploads_more_than_the_legacy_proxy_limit_in_bounded_parts() {
    let payload = (0..(9 * 1024 * 1024 + 37))
        .map(|n| u8::try_from(n % 251).unwrap())
        .collect::<Vec<_>>();
    let mut input = tempfile::NamedTempFile::new().unwrap();
    std::io::Write::write_all(&mut input, &payload).unwrap();
    let (url, server) = server::spawn(vec![
        Response::json(r#"{"uploadId":"upload-1"}"#),
        Response::json(r#"{"partNumber":1,"etag":"one"}"#),
        Response::json(r#"{"partNumber":2,"etag":"two"}"#),
        Response::empty(200),
    ])
    .await;
    client(url)
        .upload(&sandbox(), input.path(), payload.len() as u64, "/tmp/in")
        .await
        .unwrap();
    let requests = test_server::assert_ready(server).await.unwrap();
    let mut received = Vec::new();
    for request in &requests[1..3] {
        assert!(request.body.len() < 8 * 1024 * 1024);
        let start = request
            .body
            .windows(4)
            .position(|s| s == b"\r\n\r\n")
            .unwrap()
            + 4;
        let boundary = request.headers["content-type"]
            .split("boundary=")
            .nth(1)
            .unwrap();
        let suffix = format!("\r\n--{boundary}--\r\n");
        assert!(request.body.ends_with(suffix.as_bytes()));
        received.extend_from_slice(&request.body[start..request.body.len() - suffix.len()]);
    }
    assert_eq!(received, payload);
    let completed: serde_json::Value = serde_json::from_slice(&requests[3].body).unwrap();
    assert_eq!(
        completed["parts"],
        serde_json::json!([{"partNumber":1,"etag":"one"},{"partNumber":2,"etag":"two"}])
    );
}

#[tokio::test]
async fn lost_create_response_recovers_only_exact_owned_names() {
    for owned in [true, false] {
        let workspace = workspace();
        let journal = Journal::new(workspace.path());
        let nonce = intent(&journal);
        let name = control::sandbox_name(&nonce);
        let mut replies = vec![Response::json(&serde_json::json!({"metadata":{"name":name,"labels":{"struo-operation":if owned {nonce.as_str()} else {"someone-else"}}},"status":"DEPLOYED"}).to_string())];
        if owned {
            replies.extend([Response::empty(200), Response::empty(404)]);
        }
        let (url, server) = server::spawn(replies).await;
        let client = client(url);
        rescope(
            workspace.path(),
            "blaxel",
            &client.scope(),
            &client.credential(),
        );
        let report = client.reconcile(&journal).await;
        let requests = test_server::assert_ready(server).await.unwrap();
        assert_eq!(report.deleted, usize::from(owned));
        assert_eq!(requests.len(), if owned { 3 } else { 1 });
        assert_eq!(records(workspace.path()).is_empty(), owned);
        assert_eq!(requests[0].path, format!("/sandboxes/{name}"));
    }
}

#[tokio::test]
async fn uncertain_creation_is_not_retried_and_keeps_the_journal() {
    let (url, server) = server::spawn(vec![Response::empty(500)]).await;
    let workspace = workspace();
    assert!(run(url, workspace.path(), None, None).await.is_err());
    assert_eq!(test_server::assert_ready(server).await.unwrap().len(), 1);
    assert_eq!(records(workspace.path())[0]["phase"], "intent");
}

#[tokio::test]
async fn cancellation_holds_admission_until_deletion_is_confirmed() {
    let resources = test_server::single_remote_quota();
    let (get, hold) = Response::empty(404).held();
    let (url, server) = test_server::spawn(vec![Response::empty(200), get]).await;
    let client = client::Client::new(Config::for_test(url)).unwrap();
    let lease = resources
        .acquire(Executor::Blaxel, &ResourceRequest::default())
        .await;
    let guard =
        crate::remote::lifecycle::Cleanup::journaled(client, "test".into(), Some(lease), None);
    drop(guard);
    test_server::assert_ready(hold.arrived).await.unwrap();
    test_server::assert_pending(resources.acquire(Executor::Blaxel, &ResourceRequest::default()))
        .await;
    hold.release.send(()).unwrap();
    test_server::assert_ready(server).await.unwrap();
    test_server::assert_ready(resources.acquire(Executor::Blaxel, &ResourceRequest::default()))
        .await;
}

#[tokio::test]
async fn rejects_partial_json_and_oversized_binary_downloads() {
    for response in [Response::empty(206), Response::json("{}"), bytes(b"12345")] {
        let (url, server) = server::spawn(vec![response]).await;
        let mut client = client(url);
        client.limits.max_archive_bytes = 4;
        let output = tempfile::NamedTempFile::new().unwrap();
        assert!(client
            .download(&sandbox(), "/tmp/out", output.path(), None, 1)
            .await
            .is_err());
        test_server::assert_ready(server).await.unwrap();
    }
}

#[tokio::test]
async fn missing_exit_code_is_not_success_and_standby_is_not_deletion() {
    let (url, server) = server::spawn(vec![Response::json(r#"{"status":"completed"}"#)]).await;
    assert!(client(url)
        .execute(&sandbox(), "true", None, &BTreeMap::new(), None)
        .await
        .is_err());
    test_server::assert_ready(server).await.unwrap();
    let nonce = "12345678901234567890123456789012";
    let (url, server) = server::spawn(vec![Response::json(&serde_json::json!({"metadata":{"name":control::sandbox_name(nonce),"labels":{"struo-operation":nonce}},"status":"DEPLOYED","state":"STANDBY"}).to_string())]).await;
    assert!(matches!(
        client(url).find_marked(nonce).await.unwrap(),
        Lookup::Found(_)
    ));
    test_server::assert_ready(server).await.unwrap();
}
