use super::*;

#[tokio::test]
async fn idle_timeout_is_rejected_before_staging_or_provisioning() {
    let workspace = workspace();
    let resources =
        test_server::single_remote_quota().with_remote_io_limits(crate::RemoteIoLimits {
            command_idle_timeout: Some(std::time::Duration::from_secs(1)),
            ..crate::RemoteIoLimits::default()
        });
    let lease = resources
        .acquire(Executor::Vercel, &ResourceRequest::default())
        .await;
    let (client, server) = server(vec![]).await;
    let result = run(client, workspace.path(), 0, Some(lease)).await;
    assert!(matches!(
        result,
        Err(crate::Error::UnsupportedExecutorRequirement { .. })
    ));
    assert!(!workspace.path().join(".struo").exists());
    assert!(test_server::assert_ready(server).await.unwrap().is_empty());
}

#[tokio::test]
async fn deletion_rechecks_ownership_before_initial_and_retried_requests() {
    for retry in [false, true] {
        let nonce = "operation-test";
        let owned = serde_json::json!({"sandbox":{"name":client::name(nonce), "tags":{"struo-operation":nonce}}, "session":{"id":"sbx-test","status":"running"}});
        let mut foreign = owned.clone();
        foreign["sandbox"]["tags"]["struo-operation"] = "foreign".into();
        let mut responses = Vec::new();
        if retry {
            responses.extend([Response::json(&owned.to_string()), Response::empty(500)]);
        }
        responses.push(Response::json(&foreign.to_string()));
        let (client, server) = server(responses).await;
        let result = test_server::assert_ready(
            super::super::super::lifecycle::delete_with_retries(&client, &client::name(nonce)),
        )
        .await;
        assert!(result.unwrap_err().to_string().contains("ownership"));
        let requests = test_server::assert_ready(server).await.unwrap();
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.method == "DELETE")
                .count(),
            usize::from(retry)
        );
        assert_eq!(requests.last().unwrap().method, "GET");
        assert!(requests.last().unwrap().path.contains("resume=false"));
    }
}

#[tokio::test]
async fn missing_wait_exit_code_is_not_success() {
    let workspace = workspace();
    let mut responses = startup();
    responses.extend([command(Some(0)), command(None)]);
    responses.extend(cleanup());
    let (client, server) = server(responses).await;
    let error = run(client, workspace.path(), 0, None).await.unwrap_err();
    assert!(error.to_string().contains("exit code"));
    assert!(!workspace.path().join("report").exists());
    test_server::assert_ready(server).await.unwrap();
}

#[tokio::test]
async fn failed_cleanup_retains_ownership_record() {
    let workspace = workspace();
    let mut responses = startup();
    responses[4] = Response::empty(500);
    responses.extend([Response::empty(200), Response::empty(403)]);
    let (client, server) = server(responses).await;
    assert!(run(client, workspace.path(), 0, None).await.is_err());
    test_server::assert_ready(server).await.unwrap();
    assert_eq!(records(workspace.path()).len(), 1);
}

#[tokio::test]
async fn stdout_and_stderr_share_one_capture_budget() {
    let workspace = workspace();
    let resources =
        test_server::single_remote_quota().with_remote_io_limits(crate::RemoteIoLimits {
            max_capture_bytes: 4,
            ..crate::RemoteIoLimits::default()
        });
    let lease = resources
        .acquire(Executor::Vercel, &ResourceRequest::default())
        .await;
    let mut responses = startup();
    responses.extend([
        command(None),
        command(Some(0)),
        binary(b"123"),
        binary(b"45"),
    ]);
    responses.extend(cleanup());
    let (client, server) = server(responses).await;
    let error = run(client, workspace.path(), 0, Some(lease))
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        crate::Error::ExecutorResourceLimit { limit: 1, .. }
    ));
    assert!(!workspace.path().join("report").exists());
    test_server::assert_ready(server).await.unwrap();
}

#[tokio::test]
async fn pending_readiness_is_polled_and_failed_readiness_is_cleaned() {
    let workspace = workspace();
    let (client, server) = server(vec![
        Response::empty(200),
        Response::json(r#"{"session":{"id":"sbx-test","status":"pending"}}"#),
        Response::json(r#"{"session":{"id":"sbx-test","status":"failed"}}"#),
        Response::empty(200),
        Response::empty(200),
        Response::empty(404),
    ])
    .await;
    assert!(run(client, workspace.path(), 0, None).await.is_err());
    assert_eq!(
        test_server::assert_ready(server).await.unwrap()[4].method,
        "DELETE"
    );
}

#[tokio::test]
async fn upload_deadline_still_cleans_up() {
    let workspace = workspace();
    let (upload, hold) = Response::empty(200).held();
    let mut responses = startup();
    responses[4] = upload;
    responses.extend(cleanup());
    let (client, server) = server(responses).await;
    let mut execution = Box::pin(run(client, workspace.path(), 0, None));
    tokio::select! { result = &mut execution => panic!("finished early: {result:?}"), _ = hold.arrived => {} }
    let release = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(2100)).await;
        drop(hold.release);
    });
    assert!(matches!(
        test_server::assert_ready(execution).await,
        Err(crate::Error::Timeout(_))
    ));
    release.await.unwrap();
    test_server::assert_ready(server).await.unwrap();
    assert!(records(workspace.path()).is_empty());
}
