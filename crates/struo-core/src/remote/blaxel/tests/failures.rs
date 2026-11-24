use super::*;

#[tokio::test]
async fn startup_and_upload_failures_delete_the_owned_sandbox() {
    for uploading in [false, true] {
        let mut replies = vec![Response::json("CREATE")];
        if uploading {
            replies.extend([
                Response::empty(200),
                Response::json(r#"{"uploadId":"upload-1"}"#),
                Response::empty(500),
                Response::empty(200),
            ]);
        } else {
            replies.push(Response::empty(403));
        }
        replies.extend([Response::empty(200), Response::empty(404)]);
        let (url, server) = server::spawn(replies).await;
        let workspace = workspace();
        assert!(
            test_server::assert_ready(Box::pin(run(url, workspace.path(), None, None)))
                .await
                .is_err()
        );
        let requests = test_server::assert_ready(server).await.unwrap();
        assert_eq!(requests[requests.len() - 2].method, "DELETE");
        assert!(requests[requests.len() - 2]
            .path
            .starts_with("/sandboxes/struo-"));
        assert!(records(workspace.path()).is_empty());
    }
}

#[tokio::test]
async fn failed_deletion_keeps_the_owned_record_for_recovery() {
    let (url, server) = server::spawn(vec![
        Response::json("CREATE"),
        Response::empty(403),
        Response::empty(403),
    ])
    .await;
    let workspace = workspace();
    assert!(run(url, workspace.path(), None, None).await.is_err());
    let requests = test_server::assert_ready(server).await.unwrap();
    assert_eq!(requests[2].method, "DELETE");
    assert_eq!(records(workspace.path())[0]["phase"], "created");
}

#[tokio::test]
async fn captures_enforce_one_combined_limit() {
    let workspace = workspace();
    let cache = CacheProvider::open_local(workspace.path().join("cache"));
    let (url, server) = server::spawn(vec![completed(0), bytes(b"123"), bytes(b"456")]).await;
    let mut client = client(url);
    client.limits.max_capture_bytes = 5;
    let argv = vec!["true".into()];
    let env = BTreeMap::new();
    let resources = ResourceRequest::default();
    let command = PreparedCommand {
        argv: &argv,
        env: &env,
        cwd: None,
        inputs: &[],
        outputs: &[],
        resources: &resources,
        redirect: crate::local::Redirect::default(),
        timeout_ms: None,
        success_exit_codes: &[0],
        lease: None,
    };
    assert!(matches!(
        super::super::capture::execute(
            &client,
            &sandbox(),
            &command,
            workspace.path(),
            &cache,
            false
        )
        .await,
        Err(crate::Error::ExecutorResourceLimit { limit: 2, .. })
    ));
    test_server::assert_ready(server).await.unwrap();
}

#[tokio::test]
async fn timeout_still_runs_explicit_cleanup() {
    let (url, server, hold) = server::spawn_held(
        vec![
            Response::json("CREATE"),
            Response::empty(200),
            Response::empty(200),
            Response::empty(404),
        ],
        1,
    )
    .await;
    let workspace = workspace();
    let mut execute = Box::pin(run(
        url,
        workspace.path(),
        None,
        Some(crate::remote::phase::testing::BUDGET_MS),
    ));
    test_server::assert_ready(async {
        tokio::select! { _ = &mut execute => panic!("execution finished before held response"), _ = hold.arrived => {} }
    }).await;
    crate::remote::phase::testing::spend_budget().await;
    hold.release.send(()).unwrap();
    assert!(matches!(
        test_server::assert_ready(execute).await,
        Err(crate::Error::Timeout(_))
    ));
    let requests = test_server::assert_ready(server).await.unwrap();
    assert_eq!(requests[2].method, "DELETE");
    assert!(records(workspace.path()).is_empty());
}

#[tokio::test]
async fn startup_waits_for_deployment_and_guest_health() {
    let deployed = r#"{"metadata":{"name":"test"},"status":"DEPLOYED"}"#;
    let (url, server) = server::spawn(vec![
        Response::json(deployed),
        Response::empty(503),
        Response::json(deployed),
        Response::empty(200),
    ])
    .await;
    let mut pending = sandbox();
    pending.status = Some("DEPLOYING".into());
    test_server::assert_ready(client(url).wait_started(pending))
        .await
        .unwrap();
    let requests = test_server::assert_ready(server).await.unwrap();
    assert_eq!(
        requests.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(),
        [
            "/sandboxes/test",
            "/tests/sandboxes/test/health",
            "/sandboxes/test",
            "/tests/sandboxes/test/health"
        ]
    );
}

#[tokio::test]
async fn too_many_multipart_parts_are_rejected_before_upload() {
    let mut client = client("http://127.0.0.1:1".into());
    client.limits.max_archive_bytes = u64::MAX;
    assert!(matches!(
        client
            .upload(
                &sandbox(),
                Path::new("not-read"),
                10_000 * 5 * 1024 * 1024 + 1,
                "/tmp/input"
            )
            .await,
        Err(crate::Error::ExecutorResourceLimit { limit: 10_000, .. })
    ));
}
