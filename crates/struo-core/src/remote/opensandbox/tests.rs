use super::*;
use crate::remote::{
    lifecycle::{self, Provider},
    test_server::{self, Response},
};
use crate::{Executor, ResourceRequest, WorkspacePath};
use std::collections::BTreeMap;

#[test]
fn shell_words_are_literal() {
    assert_eq!(word("a'b $HOME"), "'a'\\''b $HOME'");
}

fn binding() -> RemoteExecution {
    let mut binding = RemoteExecution::provider(PROVIDER);
    binding.environment = Some("node:22-bookworm".into());
    binding
        .properties
        .insert("secure_access".into(), false.into());
    binding
}

fn started() -> Response {
    Response {
        content_type: "text/event-stream",
        ..Response::json(
            "{\"type\":\"init\",\"text\":\"cmd-1\"}\n\n{\"type\":\"execution_complete\"}\n\n",
        )
    }
}

fn finished(code: i32) -> Response {
    Response::json(&format!(r#"{{"running":false,"exit_code":{code}}}"#))
}

async fn run(
    client: Client,
    workspace: &Path,
    lease: Option<ExecutionPermit>,
    inputs: &[WorkspacePath],
    outputs: &[WorkspacePath],
) -> Result<ActionResult> {
    let argv = vec!["printf".into(), "%s".into(), "a'b $HOME".into()];
    let env = BTreeMap::from([("SELECTED".into(), "value".into())]);
    let resources = ResourceRequest::new(2, 251 * 1024 * 1024);
    let cache = CacheProvider::open_local(workspace.join("cache"));
    Box::pin(execute_with_client(
        &binding(),
        PreparedCommand {
            argv: &argv,
            env: &env,
            cwd: None,
            redirect: crate::local::Redirect::default(),
            inputs,
            outputs,
            resources: &resources,
            timeout_ms: Some(10_000),
            success_exit_codes: &[0],
            lease,
        },
        workspace,
        &cache,
        false,
        client,
    ))
    .await
}

async fn control(endpoint: &str) -> (Client, tokio::task::JoinHandle<Vec<test_server::Request>>) {
    let (url, server) = test_server::spawn(vec![
        Response::json(r#"{"id":"sandbox-1"}"#),
        Response::json(r#"{"status":{"state":"Running"}}"#),
        Response::json(&serde_json::json!({"endpoint": endpoint, "headers": {"X-EXECD-ACCESS-TOKEN": "guest-token"}}).to_string()),
        Response::empty(204),
    ]).await;
    (
        Client::new(&format!("{url}/v1"), "lifecycle-key".into()).unwrap(),
        server,
    )
}

#[cfg(unix)]
fn dependency_inputs(workspace: &Path) -> Vec<WorkspacePath> {
    use std::os::unix::fs::PermissionsExt;
    let dependency = workspace.join("node_modules/pkg/tool");
    std::fs::create_dir_all(dependency.parent().unwrap()).unwrap();
    std::fs::write(&dependency, b"dependency\0\xff").unwrap();
    std::fs::set_permissions(&dependency, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::os::unix::fs::symlink("pkg/tool", workspace.join("node_modules/tool")).unwrap();
    std::fs::write(workspace.join(".gitignore"), b"node_modules/\n").unwrap();
    std::fs::write(workspace.join("undeclared"), b"private").unwrap();
    vec![WorkspacePath::try_from("node_modules").unwrap()]
}

#[cfg(unix)]
#[tokio::test]
async fn transfers_declared_dependencies_and_binary_outputs_and_preserves_exit_status() {
    for code in [0, 7] {
        let mut replies = vec![
            Response::empty(200),
            Response::empty(200),
            started(),
            finished(0),
            started(),
            Response::json(r#"{"running":true,"exit_code":null}"#),
            finished(code),
            Response {
                body: b"out\0\xff".to_vec(),
                ..Response::empty(200)
            },
            Response::json("stderr\n"),
        ];
        if code == 0 {
            replies.extend([
                started(),
                finished(0),
                Response::tar_file("./reports/result.bin", b"result\0\xff"),
            ]);
        }
        let (guest, guest_server) = test_server::spawn(replies).await;
        let (client, server) = control(&format!("{guest}/proxy")).await;
        let workspace = tempfile::tempdir().unwrap();
        let inputs = dependency_inputs(workspace.path());
        let outputs = vec![WorkspacePath::try_from("reports").unwrap()];

        let result =
            test_server::assert_ready(run(client, workspace.path(), None, &inputs, &outputs))
                .await
                .unwrap();
        assert_eq!(result.exit_code, code);
        assert_eq!(
            result.stdout,
            Some(struo_cas::Digest::from_bytes(
                *blake3::hash(b"out\0\xff").as_bytes()
            ))
        );
        if code == 0 {
            assert_eq!(
                std::fs::read(workspace.path().join("reports/result.bin")).unwrap(),
                b"result\0\xff"
            );
        } else {
            assert!(!workspace.path().join("reports").exists());
        }
        let requests = test_server::assert_ready(guest_server).await.unwrap();
        assert_eq!(requests[0].path, "/proxy/ping");
        assert_eq!(requests[1].path, "/proxy/files/upload");
        let upload = &requests[1].body;
        let text = String::from_utf8_lossy(upload);
        assert!(text.contains("name=\"metadata\"; filename=\"metadata\""));
        assert!(text.contains("\"mode\":600"));
        let file = upload
            .windows(11)
            .position(|bytes| bytes == b"name=\"file\"")
            .unwrap();
        let start = file
            + upload[file..]
                .windows(4)
                .position(|bytes| bytes == b"\r\n\r\n")
                .unwrap()
            + 4;
        let mut archive = tar::Archive::new(&upload[start..]);
        let mut names = Vec::new();
        for entry in archive.entries().unwrap() {
            let mut entry = entry.unwrap();
            let path = entry.path().unwrap().to_string_lossy().to_string();
            if path.ends_with("pkg/tool") {
                assert_eq!(entry.header().mode().unwrap() & 0o777, 0o755);
                let mut bytes = Vec::new();
                std::io::Read::read_to_end(&mut entry, &mut bytes).unwrap();
                assert_eq!(bytes, b"dependency\0\xff");
            }
            if path.ends_with("node_modules/tool") {
                assert!(entry.header().entry_type().is_symlink());
            }
            names.push(path);
        }
        assert!(names.iter().any(|path| path.ends_with("pkg/tool")));
        assert!(!names.iter().any(|path| path.contains("undeclared")));
        let action: serde_json::Value = serde_json::from_slice(&requests[4].body).unwrap();
        assert_eq!(action["envs"]["SELECTED"], "value");
        assert_eq!(action["cwd"], ROOT);
        assert!(action["command"]
            .as_str()
            .unwrap()
            .contains("'a'\\''b $HOME'"));
        let requests = test_server::assert_ready(server).await.unwrap();
        let create: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(create["image"]["uri"], "node:22-bookworm");
        assert_eq!(create["secureAccess"], false);
        assert_eq!(create["resourceLimits"]["memory"], "251Mi");
        assert_eq!(create["resourceLimits"]["cpu"], "2");
        assert!(create["metadata"][lifecycle::MARKER_KEY]
            .as_str()
            .is_some_and(|nonce| !nonce.is_empty()));
        assert_eq!(requests.last().unwrap().method, "DELETE");
        assert!(lifecycle::tests::records(workspace.path()).is_empty());
    }
}

#[tokio::test]
async fn undeclared_guest_memory_uses_action_default() {
    let (url, server) = test_server::spawn(vec![Response::json(r#"{"id":"sandbox-1"}"#)]).await;
    let client = Client::new(&url, "key".into()).unwrap();
    client
        .create("node:22-bookworm", &ResourceRequest::default(), None, None)
        .await
        .unwrap();
    let requests = test_server::assert_ready(server).await.unwrap();
    let create: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(create["resourceLimits"]["memory"], "250Mi");
    assert_eq!(create["resourceLimits"]["cpu"], "1");
    assert_eq!(create["secureAccess"], true);
}

#[tokio::test]
async fn startup_waits_through_allocated_until_running() {
    let (guest, guest_server) = test_server::spawn(vec![Response::empty(200)]).await;
    let (url, server) = test_server::spawn(vec![
        Response::json(r#"{"status":{"state":"Pending"}}"#),
        Response::json(r#"{"status":{"state":"Allocated"}}"#),
        Response::json(r#"{"status":{"state":"Running"}}"#),
        Response::json(&serde_json::json!({"endpoint": guest}).to_string()),
    ])
    .await;
    let client = Client::new(&url, "key".into()).unwrap();
    client
        .ready(&client::Sandbox {
            id: "sandbox-1".into(),
        })
        .await
        .unwrap();
    let requests = test_server::assert_ready(server).await.unwrap();
    assert_eq!(requests.len(), 4);
    assert_eq!(
        test_server::assert_ready(guest_server).await.unwrap()[0].path,
        "/ping"
    );
}

#[tokio::test]
async fn resolved_headers_authorize_guest_without_leaking_lifecycle_key() {
    let (url, server) = lifecycle::tests::capture(vec![(200, "{}".into())]).await;
    let client = Client::new("https://control.example/v1", "lifecycle-secret".into()).unwrap();
    let endpoint = client::Endpoint {
        endpoint: format!("{url}/prefix"),
        headers: BTreeMap::from([
            ("X-EXECD-ACCESS-TOKEN".into(), "guest-secret".into()),
            ("X-Routing-Key".into(), "route".into()),
        ]),
    };
    client
        .guest(&endpoint, reqwest::Method::GET, "/ping")
        .unwrap()
        .send()
        .await
        .unwrap();
    let requests = test_server::assert_ready(server).await.unwrap();
    assert_eq!(requests[0].path, "/prefix/ping");
    assert_eq!(requests[0].headers["x-execd-access-token"], "guest-secret");
    assert_eq!(requests[0].headers["x-routing-key"], "route");
    assert!(!requests[0].headers.contains_key("open-sandbox-api-key"));
}

#[cfg(any(target_os = "linux", target_vendor = "apple"))]
#[tokio::test]
async fn cancellation_holds_admission_until_owned_deletion_finishes() {
    let resources = test_server::single_remote_quota();
    let lease = resources
        .acquire(Executor::OpenSandbox, &ResourceRequest::default())
        .await;
    let (create, mut create_hold) = Response::json(r#"{"id":"sandbox-1"}"#).held();
    let (delete, delete_hold) = Response::empty(204).held();
    let (url, server) = test_server::spawn(vec![create, delete]).await;
    let client = Client::new(&url, "key".into()).unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let mut execution = Box::pin(run(client, workspace.path(), Some(lease), &[], &[]));
    test_server::assert_ready(async {
        tokio::select! { _ = &mut execution => panic!("create finished early"), _ = &mut create_hold.arrived => {} }
    }).await;
    drop(execution);
    test_server::assert_pending(
        resources.acquire(Executor::OpenSandbox, &ResourceRequest::default()),
    )
    .await;
    create_hold.release.send(()).unwrap();
    test_server::assert_ready(delete_hold.arrived)
        .await
        .unwrap();
    test_server::assert_pending(
        resources.acquire(Executor::OpenSandbox, &ResourceRequest::default()),
    )
    .await;
    delete_hold.release.send(()).unwrap();
    let requests = test_server::assert_ready(server).await.unwrap();
    assert_eq!(requests[1].method, "DELETE");
    test_server::assert_ready(
        resources.acquire(Executor::OpenSandbox, &ResourceRequest::default()),
    )
    .await;
    assert!(lifecycle::tests::records(workspace.path()).is_empty());
}

#[tokio::test]
async fn recovery_only_deletes_matching_ownership_markers() {
    let workspace = tempfile::tempdir().unwrap();
    let journal = lifecycle::Journal::new(workspace.path());
    let nonce = lifecycle::tests::intent(&journal);
    let listing = serde_json::json!({"items":[{"id":"ours","metadata":{lifecycle::MARKER_KEY: nonce}},{"id":"foreign","metadata":{}}],"pagination":{"hasNextPage":false}});
    let (url, server) =
        lifecycle::tests::capture(vec![(200, listing.to_string()), (204, String::new())]).await;
    let client = Client::new(&url, "control-secret".into()).unwrap();
    lifecycle::tests::rescope(
        workspace.path(),
        PROVIDER,
        &client.scope(),
        &client.credential(),
    );
    lifecycle::reconcile(&journal, &client, lifecycle::Budget::default()).await;
    let requests = test_server::assert_ready(server).await.unwrap();
    assert_eq!(requests[1].path, "/sandboxes/ours");
    assert!(requests.iter().all(|request| request
        .headers
        .get("open-sandbox-api-key")
        .is_some_and(|value| value == "control-secret")));
}

#[tokio::test]
async fn incomplete_or_foreign_listings_cannot_prove_absence() {
    for response in [
        r#"{"items":[],"pagination":{"hasNextPage":true}}"#,
        r#"{"items":[{"id":"foreign"}],"pagination":{"hasNextPage":false}}"#,
    ] {
        let (url, server) = test_server::spawn(vec![Response::json(response)]).await;
        let client = Client::new(&url, "key".into()).unwrap();
        assert_eq!(
            client.find("nonce").await.unwrap(),
            lifecycle::Lookup::Inconclusive
        );
        test_server::assert_ready(server).await.unwrap();
    }
}

#[cfg(any(target_os = "linux", target_vendor = "apple"))]
#[tokio::test]
async fn upload_failure_deletes_sandbox_and_releases_admission() {
    let resources = test_server::single_remote_quota();
    let lease = resources
        .acquire(Executor::OpenSandbox, &ResourceRequest::default())
        .await;
    let (url, guest) = test_server::spawn(vec![Response::empty(200), Response::empty(400)]).await;
    let (client, server) = control(&url).await;
    let workspace = tempfile::tempdir().unwrap();
    assert!(matches!(
        run(client, workspace.path(), Some(lease), &[], &[]).await,
        Err(Error::RemoteProviderStatus { status: 400, .. })
    ));
    test_server::assert_ready(guest).await.unwrap();
    let requests = test_server::assert_ready(server).await.unwrap();
    assert_eq!(requests.last().unwrap().method, "DELETE");
    test_server::assert_ready(
        resources.acquire(Executor::OpenSandbox, &ResourceRequest::default()),
    )
    .await;
    assert!(lifecycle::tests::records(workspace.path()).is_empty());
}

#[cfg(any(target_os = "linux", target_vendor = "apple"))]
#[tokio::test]
async fn action_timeout_still_awaits_cleanup() {
    let (upload, mut hold) = Response::empty(200).held();
    let (url, guest) = test_server::spawn(vec![Response::empty(200), upload]).await;
    let (client, server) = control(&url).await;
    let workspace = tempfile::tempdir().unwrap();
    let mut execution = Box::pin(run(client, workspace.path(), None, &[], &[]));
    test_server::assert_ready(async {
        tokio::select! { _ = &mut execution => panic!("upload finished early"), _ = &mut hold.arrived => {} }
    }).await;
    crate::remote::phase::testing::spend_budget().await;
    let result = test_server::assert_ready(execution).await;
    assert!(matches!(result, Err(Error::Timeout(_))));
    hold.release.send(()).unwrap();
    test_server::assert_ready(guest).await.unwrap();
    let requests = test_server::assert_ready(server).await.unwrap();
    assert_eq!(requests.last().unwrap().method, "DELETE");
    assert!(lifecycle::tests::records(workspace.path()).is_empty());
}

#[tokio::test]
async fn missing_exit_code_and_oversized_download_fail_closed() {
    let (url, server) =
        test_server::spawn(vec![started(), Response::json(r#"{"running":false}"#)]).await;
    let client = Client::new(&url, "key".into()).unwrap();
    let endpoint = client::Endpoint {
        endpoint: url,
        headers: BTreeMap::new(),
    };
    assert!(client
        .execute(&endpoint, "true", None, &BTreeMap::new(), None)
        .await
        .unwrap_err()
        .to_string()
        .contains("exit_code"));
    test_server::assert_ready(server).await.unwrap();
    for status in [200, 206] {
        let response = Response {
            body: b"five!".to_vec(),
            ..Response::empty(status)
        };
        let (url, server) = test_server::spawn(vec![response]).await;
        let mut client = Client::new(&url, "key".into()).unwrap();
        client.limits.max_archive_bytes = 4;
        let endpoint = client::Endpoint {
            endpoint: url,
            headers: BTreeMap::new(),
        };
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), b"original").unwrap();
        assert!(client
            .download(&endpoint, "/file", file.path(), None, 1)
            .await
            .is_err());
        assert_eq!(std::fs::read(file.path()).unwrap(), b"original");
        test_server::assert_ready(server).await.unwrap();
    }
}
