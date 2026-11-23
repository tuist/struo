use super::*;

#[tokio::test]
async fn capture_uses_reserved_allowance_independently_of_archive_and_transfer_limits() {
    for max_archive_bytes in [1024, 131_072] {
        let stdout = (0..65_537_u32)
            .map(|index| index.to_le_bytes()[0])
            .collect::<Vec<_>>();
        let stderr = b"stderr\0\xff";
        let (url, server) = test_server::spawn(vec![
            exited(0),
            Response {
                body: stdout.clone(),
                ..Response::empty(200)
            },
            Response {
                body: stderr.to_vec(),
                ..Response::empty(200)
            },
        ])
        .await;
        let limits = crate::RemoteIoLimits {
            max_archive_bytes,
            max_staging_bytes: 65_536,
            max_capture_bytes: 131_072,
            max_capture_staging_bytes: 131_072,
            ..crate::RemoteIoLimits::default()
        };
        let resources = test_server::single_remote_quota().with_remote_io_limits(limits);
        let lease = resources
            .acquire(crate::Executor::Tensorlake, &ResourceRequest::default())
            .await;
        let capture = lease.capture_budget();
        capture.reserve_upfront().await.unwrap();
        let _transfer = lease.acquire_staging(65_536).await.unwrap();
        let mut client = client(&url);
        client.limits = limits;
        let workspace = tempfile::tempdir().unwrap();
        let cache = CacheProvider::open_local(workspace.path().join("cache"));
        let request = ResourceRequest::default();
        let command = PreparedCommand {
            lease: Some(lease),
            argv: &["true".into()],
            env: &BTreeMap::new(),
            cwd: None,
            inputs: &[],
            outputs: &[],
            resources: &request,
            redirect: crate::local::Redirect::default(),
            timeout_ms: None,
            success_exit_codes: &[0],
        };
        let result = test_server::assert_ready(Box::pin(super::super::capture::execute(
            &client,
            &sandbox(),
            &client.guest_url(&url).unwrap(),
            &command,
            workspace.path(),
            &cache,
            false,
        )))
        .await
        .unwrap();
        assert_eq!(
            cache.get_blob(&result.stdout.unwrap()).await.unwrap(),
            stdout
        );
        assert_eq!(
            cache.get_blob(&result.stderr.unwrap()).await.unwrap(),
            stderr
        );
        test_server::assert_ready(server).await.unwrap();
    }
}

#[tokio::test]
async fn capture_reservation_remains_held_after_provisioning_starts() {
    let (created, hold) = Response::json(r#"{"sandbox_id":"sb-one"}"#).held();
    let mut created = Some(created);
    let mut name = String::new();
    let mut terminated = false;
    let (url, server) =
        test_server::spawn_with_handler(4, move |request| match request.method.as_str() {
            "POST" => {
                let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
                name = body["name"].as_str().unwrap().into();
                created.take().unwrap()
            }
            "GET" => info(&name, if terminated { "terminated" } else { "running" }, ""),
            "DELETE" => {
                terminated = true;
                Response::empty(204)
            }
            _ => panic!("unexpected request"),
        })
        .await;
    let resources = test_server::single_remote_quota()
        .with_remote_concurrency(2)
        .with_remote_io_limits(crate::RemoteIoLimits {
            max_capture_bytes: 65_536,
            max_capture_staging_bytes: 65_536,
            ..crate::RemoteIoLimits::default()
        });
    let lease = resources
        .acquire(crate::Executor::Tensorlake, &ResourceRequest::default())
        .await;
    let other = resources
        .acquire(crate::Executor::Tensorlake, &ResourceRequest::default())
        .await;
    let capture = other.capture_budget();
    let workspace = workspace();
    let root = workspace.path().to_path_buf();
    let client = client(&url);
    let task = tokio::spawn(async move { run(client, &root, Some(lease), None, &[]).await });
    test_server::assert_ready(hold.arrived).await.unwrap();
    test_server::assert_pending(capture.reserve_upfront()).await;
    task.abort();
    let _ = task.await;
    test_server::assert_ready(capture.reserve_upfront())
        .await
        .unwrap();
    hold.release.send(()).unwrap();
    test_server::assert_ready(server).await.unwrap();
    let _lease = test_server::assert_ready(
        resources.acquire(crate::Executor::Tensorlake, &ResourceRequest::default()),
    )
    .await;
    assert!(crate::remote::lifecycle::tests::records(workspace.path()).is_empty());
}
