use super::*;
use crate::remote::lifecycle::{tests::records, Provider};
use crate::remote::test_server::{self, Request, Response};
use crate::{Executor, ResourceRequest, WorkspacePath};

mod failures;

fn client(endpoint: String) -> Client {
    Client::new(Config {
        endpoint,
        token: "test-token".into(),
        team: "team-test".into(),
        project: "prj-test".into(),
    })
    .unwrap()
}

async fn server(mut responses: Vec<Response>) -> (Client, tokio::task::JoinHandle<Vec<Request>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let mut requests = Vec::new();
        let mut owned = serde_json::Value::Null;
        for response in &mut responses {
            let (mut connection, _) = listener.accept().await.unwrap();
            let request = test_server::read_request(&mut connection).await;
            if request.path == "/v2/sandboxes?teamId=team-test"
                && request.method == "POST"
                && response.status == 200
            {
                let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
                owned = serde_json::json!({"sandbox":{"name":body["name"], "tags":body["tags"]}, "session":{"id":"sbx-test","status":"pending"}});
                response.body = serde_json::to_vec(&owned).unwrap();
            }
            if request.method == "GET"
                && request.path.starts_with("/v2/sandboxes/struo-")
                && response.status == 200
                && response.body.is_empty()
            {
                response.body = serde_json::to_vec(&owned).unwrap();
            }
            if let Some(hold) = response.hold.take() {
                let _ = hold.arrived.send(());
                let _ = hold.release.await;
            }
            test_server::write_response(&mut connection, response).await;
            requests.push(request);
        }
        requests
    });
    (client(endpoint), task)
}

fn command(code: Option<i32>) -> Response {
    Response::json(
        &serde_json::json!({"command":{"id":"cmd-test", "sessionId":"sbx-test", "exitCode":code}})
            .to_string(),
    )
}

fn binary(bytes: &[u8]) -> Response {
    let mut response = Response::empty(200);
    response.body = bytes.to_vec();
    response
}

fn startup() -> Vec<Response> {
    vec![
        Response::empty(200),
        Response::json(r#"{"session":{"id":"sbx-test","status":"running"}}"#),
        command(None),
        command(Some(0)),
        Response::empty(200),
    ]
}

fn cleanup() -> [Response; 3] {
    [
        Response::empty(200),
        Response::empty(200),
        Response::empty(404),
    ]
}

async fn run(
    client: Client,
    workspace: &Path,
    code: i32,
    lease: Option<ExecutionPermit>,
) -> Result<ActionResult> {
    let argv = vec!["node".into(), "tests/a b;$(touch unwanted).js".into()];
    let inputs = vec![WorkspacePath::try_from("node_modules").unwrap()];
    let outputs = vec![WorkspacePath::try_from("report").unwrap()];
    let env = BTreeMap::from([("VALUE".into(), "a b;$x".into())]);
    let resources = ResourceRequest::default();
    Box::pin(execute(
        client,
        &RemoteExecution::provider("vercel"),
        PreparedCommand {
            lease,
            argv: &argv,
            env: &env,
            cwd: None,
            inputs: &inputs,
            outputs: &outputs,
            resources: &resources,
            redirect: crate::local::Redirect::default(),
            timeout_ms: Some(2000),
            success_exit_codes: &[code],
        },
        workspace,
        &CacheProvider::open_local(workspace.join("cache")),
        false,
    ))
    .await
}

fn workspace() -> tempfile::TempDir {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::create_dir(workspace.path().join("node_modules")).unwrap();
    std::fs::write(
        workspace.path().join("node_modules/dependency"),
        b"dep\0\xff",
    )
    .unwrap();
    std::fs::write(workspace.path().join(".gitignore"), "node_modules\n").unwrap();
    workspace
}

#[tokio::test]
async fn transfers_declared_dependencies_binary_capture_and_outputs_then_deletes() {
    let workspace = workspace();
    let mut responses = startup();
    responses.extend([
        command(None),
        command(Some(7)),
        binary(b"out\0\xff"),
        binary(b"err\xfe"),
        command(None),
        command(Some(0)),
    ]);
    let mut output = Response::tar_file("report/result", b"ok\0\xff");
    output.content_type = "application/octet-stream";
    responses.push(output);
    responses.extend(cleanup());
    let (client, server) = server(responses).await;
    let result = test_server::assert_ready(run(client, workspace.path(), 7, None))
        .await
        .unwrap();
    let requests = test_server::assert_ready(server).await.unwrap();
    assert_eq!(result.exit_code, 7);
    assert_eq!(
        result.stdout.unwrap(),
        struo_cas::Digest::from_bytes(*blake3::hash(b"out\0\xff").as_bytes())
    );
    assert_eq!(
        result.stderr.unwrap(),
        struo_cas::Digest::from_bytes(*blake3::hash(b"err\xfe").as_bytes())
    );
    assert_eq!(
        std::fs::read(workspace.path().join("report/result")).unwrap(),
        b"ok\0\xff"
    );
    let create: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(create["persistent"], false);
    assert_eq!(create["runtime"], "node24");
    assert_eq!(create["projectId"], "prj-test");
    assert_eq!(create["timeout"], 122_000);
    for request in &requests {
        assert_eq!(request.headers["authorization"], "Bearer test-token");
        assert!(request.path.contains("teamId=team-test"));
    }
    assert_eq!(requests[4].headers["content-type"], "application/gzip");
    assert_eq!(requests[4].headers["x-cwd"], ROOT);
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(&requests[4].body[..]));
    let mut contents = BTreeMap::new();
    for entry in archive.entries().unwrap() {
        let mut entry = entry.unwrap();
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut bytes).unwrap();
        contents.insert(entry.path().unwrap().to_string_lossy().into_owned(), bytes);
    }
    assert_eq!(contents["node_modules/dependency"], b"dep\0\xff");
    let execution: serde_json::Value = serde_json::from_slice(&requests[5].body).unwrap();
    assert_eq!(execution["args"][3], "VALUE=a b;$x");
    assert_eq!(execution["args"][5], "tests/a b;$(touch unwanted).js");
    assert!(requests[6].path.contains("wait=true"));
    assert!(requests[13].path.contains("deleteOrphanSnapshots=true"));
    assert_eq!(requests[13].method, "DELETE");
    assert!(records(workspace.path()).is_empty());
}

#[tokio::test]
async fn failed_command_does_not_restore_outputs() {
    let workspace = workspace();
    let mut responses = startup();
    responses.extend([
        command(None),
        command(Some(9)),
        binary(b""),
        binary(b"failure"),
    ]);
    responses.extend(cleanup());
    let (client, server) = server(responses).await;
    let result = test_server::assert_ready(run(client, workspace.path(), 0, None))
        .await
        .unwrap();
    assert_eq!(result.exit_code, 9);
    assert!(!workspace.path().join("report").exists());
    assert_eq!(test_server::assert_ready(server).await.unwrap().len(), 12);
}

#[tokio::test]
async fn upload_failure_still_deletes() {
    let workspace = workspace();
    let mut responses = startup();
    responses[4] = Response::empty(500);
    responses.extend(cleanup());
    let (client, server) = server(responses).await;
    assert!(
        test_server::assert_ready(run(client, workspace.path(), 0, None))
            .await
            .is_err()
    );
    let requests = test_server::assert_ready(server).await.unwrap();
    assert_eq!(requests[6].method, "DELETE");
    assert!(records(workspace.path()).is_empty());
}

#[tokio::test]
async fn uncertain_create_is_not_retried_and_remains_journaled() {
    let workspace = workspace();
    let (client, server) = server(vec![Response::empty(500)]).await;
    assert!(run(client, workspace.path(), 0, None).await.is_err());
    assert_eq!(test_server::assert_ready(server).await.unwrap().len(), 1);
    assert_eq!(records(workspace.path())[0]["phase"], "intent");
}

#[tokio::test]
async fn recovery_verifies_exact_name_and_tag_before_deleting() {
    for foreign in [false, true] {
        let workspace = workspace();
        let journal = super::super::lifecycle::Journal::new(workspace.path());
        let nonce = super::super::lifecycle::tests::intent(&journal);
        let body = serde_json::json!({"sandbox":{"name":client::name(&nonce), "tags":{"struo-operation":if foreign {"other"} else {&nonce}}}, "session":{"id":"sbx-test","status":"stopped"}});
        let mut responses = vec![Response::json(&body.to_string())];
        if !foreign {
            responses.extend([
                Response::json(&body.to_string()),
                Response::empty(200),
                Response::empty(404),
            ]);
        }
        let (client, server) = server(responses).await;
        super::super::lifecycle::tests::rescope(
            workspace.path(),
            "vercel",
            &client.scope(),
            &client.credential(),
        );
        let report = super::super::lifecycle::reconcile(
            &journal,
            &client,
            super::super::lifecycle::Budget::default(),
        )
        .await;
        let requests = test_server::assert_ready(server).await.unwrap();
        assert!(requests[0].path.contains("resume=false"));
        assert_eq!(report.deleted, usize::from(!foreign));
        assert_eq!(records(workspace.path()).is_empty(), !foreign);
    }
}

#[tokio::test]
async fn downloads_reject_json_partial_and_oversized_bodies() {
    for response in [Response::json("{}"), Response::empty(206), binary(b"12345")] {
        let workspace = workspace();
        let (mut client, server) = server(vec![response]).await;
        client.limits.max_archive_bytes = 4;
        assert!(client
            .download(
                "sbx-test",
                "/tmp/file",
                &workspace.path().join("file"),
                None,
                1
            )
            .await
            .is_err());
        test_server::assert_ready(server).await.unwrap();
    }
}

#[tokio::test]
async fn cancellation_holds_remote_admission_until_cleanup_finishes() {
    let workspace = workspace();
    let resources = test_server::single_remote_quota();
    let lease = resources
        .acquire(Executor::Vercel, &ResourceRequest::default())
        .await;
    let (upload, hold) = Response::empty(200).held();
    let mut responses = startup();
    responses[4] = upload;
    responses.extend(cleanup());
    let (client, server) = server(responses).await;
    let mut execution = Box::pin(run(client, workspace.path(), 0, Some(lease)));
    tokio::select! { result = &mut execution => panic!("finished early: {result:?}"), _ = hold.arrived => {} }
    drop(execution);
    test_server::assert_pending(resources.acquire(Executor::Vercel, &ResourceRequest::default()))
        .await;
    drop(hold.release);
    test_server::assert_ready(server).await.unwrap();
    test_server::assert_ready(resources.acquire(Executor::Vercel, &ResourceRequest::default()))
        .await;
}
