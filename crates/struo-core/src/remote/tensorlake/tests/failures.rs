use super::*;
use crate::remote::lifecycle::{self, tests::records, Journal};

#[tokio::test]
async fn readiness_deadline_still_terminates_the_owned_sandbox() {
    let (arrived_tx, arrived) = tokio::sync::oneshot::channel();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let mut name = String::new();
        let mut arrived = Some(arrived_tx);
        let mut requests = Vec::new();
        let mut terminated = false;
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request = test_server::read_request(&mut stream).await;
            let response = match request.method.as_str() {
                "POST" => {
                    let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
                    name = body["name"].as_str().unwrap().into();
                    Response::json(r#"{"sandbox_id":"sb-one"}"#)
                }
                "DELETE" => {
                    terminated = true;
                    Response::empty(204)
                }
                "GET" => {
                    if let Some(arrived) = arrived.take() {
                        let _ = arrived.send(());
                    }
                    info(&name, if terminated { "terminated" } else { "pending" }, "")
                }
                _ => panic!("work must not start before readiness"),
            };
            let finished = terminated && request.method == "GET";
            requests.push(request);
            test_server::write_response(&mut stream, &response).await;
            if finished {
                return requests;
            }
        }
    });
    let workspace = workspace();
    let client = client(&url);
    let root = workspace.path().to_path_buf();
    let task = tokio::spawn(async move {
        run(
            client,
            &root,
            None,
            Some(crate::remote::phase::testing::BUDGET_MS),
            &[],
        )
        .await
    });
    test_server::assert_ready(arrived).await.unwrap();
    crate::remote::phase::testing::spend_budget().await;
    assert!(matches!(
        test_server::assert_ready(task).await.unwrap(),
        Err(crate::Error::Timeout(_))
    ));
    let requests = test_server::assert_ready(server).await.unwrap();
    assert!(requests
        .iter()
        .any(|request| request.method == "DELETE" && request.path == "/sandboxes/sb-one"));
    assert!(records(workspace.path()).is_empty());
}

#[tokio::test]
async fn failed_termination_retains_recorded_identity_for_recovery() {
    let mut name = String::new();
    let (url, server) =
        test_server::spawn_with_handler(3, move |request| match request.method.as_str() {
            "POST" => {
                let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
                name = body["name"].as_str().unwrap().into();
                Response::json(r#"{"sandbox_id":"sb-one"}"#)
            }
            "GET" => info(&name, "running", ""),
            "DELETE" => Response::empty(403),
            _ => panic!("unexpected request"),
        })
        .await;
    let workspace = tempfile::tempdir().unwrap();
    let (_, cleanup) = lifecycle::create_owned(
        client(&url),
        Some(Journal::new(workspace.path())),
        std::time::Duration::ZERO,
        None,
        |client, marker| async move { client.create(&binding(), &marker.unwrap().nonce).await },
    )
    .await
    .unwrap();
    assert!(cleanup.run().await.is_err());
    test_server::assert_ready(server).await.unwrap();
    let records = records(workspace.path());
    assert_eq!(records.len(), 1);
    assert!(records[0]["sandbox_ids"][0]
        .as_str()
        .unwrap()
        .contains("sb-one"));
}

#[tokio::test]
async fn stdout_and_stderr_share_one_capture_limit() {
    let (url, server) = test_server::spawn(vec![
        exited(0),
        Response {
            body: b"123456".to_vec(),
            ..Response::empty(200)
        },
        Response {
            body: b"abcd".to_vec(),
            ..Response::empty(200)
        },
    ])
    .await;
    let mut client = client(&url);
    client.limits.max_capture_bytes = 9;
    let workspace = tempfile::tempdir().unwrap();
    let resources = ResourceRequest::default();
    let command = PreparedCommand {
        lease: None,
        argv: &["true".into()],
        env: &BTreeMap::new(),
        cwd: None,
        inputs: &[],
        outputs: &[],
        resources: &resources,
        redirect: crate::local::Redirect::default(),
        timeout_ms: None,
        success_exit_codes: &[0],
    };
    let result = super::super::capture::execute(
        &client,
        &sandbox(),
        &client.guest_url(&url).unwrap(),
        &command,
        workspace.path(),
        &CacheProvider::open_local(workspace.path().join("cache")),
        false,
    )
    .await;
    assert!(matches!(
        result,
        Err(crate::Error::ExecutorResourceLimit { limit: 3, .. })
    ));
    test_server::assert_ready(server).await.unwrap();
}

#[tokio::test]
async fn snapshot_creation_does_not_claim_a_warm_pool_or_use_template_markers() {
    let (url, server) =
        test_server::spawn(vec![Response::json(r#"{"sandbox_id":"sb-one"}"#)]).await;
    let mut remote = binding();
    remote
        .properties
        .insert("environment_kind".into(), "snapshot".into());
    client(&url).create(&remote, "nonce").await.unwrap();
    let requests = test_server::assert_ready(server).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(body["snapshot_id"], "node-tests-v1");
    assert!(body.get("image").is_none());
    assert!(body.get("template_id").is_none());
    assert_eq!(requests[0].path, "/sandboxes");
}
