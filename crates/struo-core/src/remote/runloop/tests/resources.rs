use super::*;

#[tokio::test]
async fn output_restoration_releases_capture_for_a_staged_competitor() {
    let limits = crate::RemoteIoLimits {
        max_staging_bytes: 128 * 1024,
        max_capture_bytes: 64 * 1024,
        max_capture_staging_bytes: 64 * 1024,
        ..crate::RemoteIoLimits::default()
    };
    let resources = test_server::single_remote_quota()
        .with_remote_concurrency(2)
        .with_remote_io_limits(limits);
    let first = resources
        .acquire(Executor::Runloop, &ResourceRequest::default())
        .await;
    let second = resources
        .acquire(Executor::Runloop, &ResourceRequest::default())
        .await;
    let (command, hold) = execution("completed", Some(0)).held();
    let (url, server) = test_server::spawn(vec![
        sandbox("running"),
        Response::empty(200),
        execution("completed", Some(0)),
        command,
        binary(b"stdout"),
        binary(b"stderr"),
        execution("completed", Some(0)),
        Response::tar_file("reports/result", b"passed"),
        sandbox("shutdown"),
    ])
    .await;
    let workspace = tempfile::tempdir().unwrap();
    let outputs = [WorkspacePath::try_from("reports").unwrap()];
    let mut execute = Box::pin(run(
        client(&url),
        workspace.path(),
        Some(first),
        None,
        &outputs,
    ));
    test_server::assert_ready(async {
        tokio::select! {
            result = &mut execute => panic!("execution finished before command: {result:?}"),
            _ = hold.arrived => {}
        }
    })
    .await;
    let staging = second
        .acquire_staging(limits.max_staging_bytes)
        .await
        .unwrap();
    let capture = second.capture_budget();
    test_server::assert_pending(capture.reserve_upfront()).await;
    hold.release.send(()).unwrap();
    let competitor = async {
        capture.reserve_upfront().await.unwrap();
        drop(staging);
    };
    let (result, ()) = test_server::assert_ready(async { tokio::join!(execute, competitor) }).await;
    assert_eq!(result.unwrap().exit_code, 0);
    assert_eq!(
        std::fs::read(workspace.path().join("reports/result")).unwrap(),
        b"passed"
    );
    test_server::assert_ready(server).await.unwrap();
    assert!(crate::remote::lifecycle::tests::records(workspace.path()).is_empty());
}

#[tokio::test]
async fn capture_uses_reserved_allowance_independently_of_archive_and_transfer_limits() {
    for max_archive_bytes in [1024, 131_072] {
        let stdout = (0..65_537_u32)
            .map(|index| index.to_le_bytes()[0])
            .collect::<Vec<_>>();
        let stderr = b"stderr\0\xff";
        let (url, server) = test_server::spawn(vec![
            execution("completed", Some(0)),
            binary(&stdout),
            binary(stderr),
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
            .acquire(Executor::Runloop, &ResourceRequest::default())
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
        let sandbox = serde_json::from_slice(&sandbox("running").body).unwrap();
        let result = test_server::assert_ready(Box::pin(capture::execute(
            &client,
            &sandbox,
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
