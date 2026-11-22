use super::*;
use crate::remote::lifecycle::tests::records;
use crate::remote::test_server::{self, Response};
use crate::{Executor, ResourceRequest, WorkspacePath};

fn client(url: &str) -> Client {
    Client::new(url, "test-token".into(), crate::RemoteIoLimits::default()).unwrap()
}

#[tokio::test]
async fn uploads_declared_node_modules_and_restores_outputs_and_separate_captures() {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::create_dir(workspace.path().join("node_modules")).unwrap();
    std::fs::write(
        workspace.path().join("node_modules/dependency.js"),
        "module.exports = 42",
    )
    .unwrap();
    let cache = CacheProvider::open_local(workspace.path().join("cache"));
    let inputs = vec![WorkspacePath::try_from("node_modules").unwrap()];
    let (archive, _permit) = create_input_archive_limited(
        workspace.path(),
        &inputs,
        PROVIDER,
        crate::RemoteIoLimits::default(),
        None,
    )
    .await
    .unwrap();
    let output = vec![WorkspacePath::try_from("reports").unwrap()];
    let argv = vec!["node".into(), "a b;$(false)".into()];
    let env = BTreeMap::from([("MODE".into(), "test".into())]);
    let resources = ResourceRequest::default();
    let command = PreparedCommand {
        argv: &argv,
        env: &env,
        cwd: None,
        inputs: &inputs,
        outputs: &output,
        resources: &resources,
        redirect: crate::local::Redirect::default(),
        timeout_ms: None,
        success_exit_codes: &[7],
        lease: None,
    };
    let mut stdout = Response::empty(200);
    stdout.body = b"stdout\0binary".to_vec();
    let mut stderr = Response::empty(200);
    stderr.body = b"stderr".to_vec();
    let (url, server) = test_server::spawn(vec![
        Response::empty(204),
        Response::json("{}"),
        Response::json(r#"{"exitCode":7}"#),
        stdout,
        stderr,
        Response::json("{}"),
        Response::tar_file("./reports/result.txt", b"passed"),
    ])
    .await;
    let client = client(&url);
    client
        .upload("owned", archive.path(), archive.len())
        .await
        .unwrap();
    let result = run(
        &client,
        &Sandbox { id: "owned".into() },
        &command,
        workspace.path(),
        &cache,
        false,
        None,
    )
    .await
    .unwrap();
    let requests = test_server::assert_ready(server).await.unwrap();
    assert_eq!(result.exit_code, 7);
    assert_eq!(
        result.stdout,
        Some(struo_cas::Digest::from_bytes(
            *blake3::hash(b"stdout\0binary").as_bytes()
        ))
    );
    assert_eq!(
        result.stderr,
        Some(struo_cas::Digest::from_bytes(
            *blake3::hash(b"stderr").as_bytes()
        ))
    );
    assert_eq!(
        std::fs::read(workspace.path().join("reports/result.txt")).unwrap(),
        b"passed"
    );
    let mut uploaded = tar::Archive::new(requests[0].body.as_slice());
    assert!(uploaded.entries().unwrap().any(|entry| entry
        .unwrap()
        .path()
        .unwrap()
        .ends_with("node_modules/dependency.js")));
    let payload: serde_json::Value = serde_json::from_slice(&requests[2].body).unwrap();
    assert_eq!(payload["argv"], json!(argv));
    assert_eq!(payload["env"]["MODE"], "test");
}

#[tokio::test]
async fn uncertain_provisioning_retains_durable_intent() {
    let workspace = tempfile::tempdir().unwrap();
    let cache = CacheProvider::open_local(workspace.path().join("cache"));
    let (url, server) = test_server::spawn(vec![Response::empty(503)]).await;
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
    let error = Box::pin(execute(
        &RemoteExecution::provider(PROVIDER),
        command,
        workspace.path(),
        &cache,
        false,
        client(&url),
    ))
    .await
    .unwrap_err();
    assert!(matches!(
        error,
        Error::RemoteProviderStatus { status: 503, .. }
    ));
    test_server::assert_ready(server).await.unwrap();
    assert_eq!(records(workspace.path())[0]["phase"], "intent");
}

#[tokio::test]
async fn cancellation_holds_remote_admission_until_confirmed_deletion() {
    let workspace = tempfile::tempdir().unwrap();
    let resources = test_server::single_remote_quota();
    let lease = resources
        .acquire(Executor::Cloudflare, &ResourceRequest::default())
        .await;
    let (response, hold) = Response::empty(204).held();
    let (url, server) = test_server::spawn(vec![response]).await;
    let (_sandbox, cleanup) = lifecycle::create_owned(
        client(&url),
        Some(lifecycle::Journal::new(workspace.path())),
        LIFETIME,
        Some(lease),
        |_, _| async { Ok(Sandbox { id: "owned".into() }) },
    )
    .await
    .unwrap();
    drop(cleanup);
    test_server::assert_ready(hold.arrived).await.unwrap();
    test_server::assert_pending(
        resources.acquire(Executor::Cloudflare, &ResourceRequest::default()),
    )
    .await;
    hold.release.send(()).unwrap();
    let requests = test_server::assert_ready(server).await.unwrap();
    assert_eq!(requests[0].method, "DELETE");
    assert_eq!(requests[0].path, "/v1/sandboxes/owned");
    test_server::assert_ready(resources.acquire(Executor::Cloudflare, &ResourceRequest::default()))
        .await;
    assert!(records(workspace.path()).is_empty());
}

#[test]
fn bridge_rejects_insecure_origins_and_path_injection() {
    for url in [
        "http://example.com",
        "https://user:secret@example.com",
        "https://example.com?token=secret",
        "https://example.com/#fragment",
    ] {
        assert!(Client::new(url, "token".into(), crate::RemoteIoLimits::default()).is_err());
    }
    assert!(client::validate_id("../foreign").is_err());
}

#[tokio::test]
async fn large_archives_use_ordered_bounded_upload_requests() {
    let workspace = tempfile::tempdir().unwrap();
    let source = workspace.path().join("archive");
    let first = vec![b'x'; 8 * 1024 * 1024];
    let mut bytes = first.clone();
    bytes.extend_from_slice(b"last");
    std::fs::write(&source, &bytes).unwrap();
    let (url, server) = test_server::spawn(vec![
        Response::empty(204),
        Response::empty(204),
        Response::json("{}"),
    ])
    .await;
    client(&url)
        .upload("owned", &source, bytes.len() as u64)
        .await
        .unwrap();
    let requests = test_server::assert_ready(server).await.unwrap();
    assert_eq!(requests[0].path, "/v1/sandboxes/owned/inputs/0");
    assert_eq!(requests[0].body, first);
    assert_eq!(requests[1].path, "/v1/sandboxes/owned/inputs/8388608");
    assert_eq!(requests[1].body, b"last");
    assert_eq!(requests[2].path, "/v1/sandboxes/owned/stage");
}

#[tokio::test]
async fn recovery_never_deletes_an_id_that_disagrees_with_its_marker() {
    use lifecycle::Provider;
    let (url, server) = test_server::spawn(vec![Response::json(
        r#"[{"id":"foreign","marker":"owned"}]"#,
    )])
    .await;
    assert_eq!(
        client(&url).find("owned").await.unwrap(),
        lifecycle::Lookup::Inconclusive
    );
    test_server::assert_ready(server).await.unwrap();
}

#[tokio::test]
async fn missing_exit_code_fails_instead_of_publishing_a_success() {
    let workspace = tempfile::tempdir().unwrap();
    let cache = CacheProvider::open_local(workspace.path().join("cache"));
    let (url, server) = test_server::spawn(vec![Response::json("{}")]).await;
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
    assert!(run(
        &client(&url),
        &Sandbox { id: "owned".into() },
        &command,
        workspace.path(),
        &cache,
        false,
        None
    )
    .await
    .is_err());
    test_server::assert_ready(server).await.unwrap();
}

#[tokio::test]
async fn capture_capacity_is_reserved_before_provisioning() {
    let limits = crate::RemoteIoLimits {
        max_capture_bytes: 65536,
        max_capture_staging_bytes: 65536,
        ..crate::RemoteIoLimits::default()
    };
    for occupied in [false, true] {
        let limits = crate::RemoteIoLimits {
            max_capture_staging_bytes: if occupied { 65536 } else { 0 },
            ..limits
        };
        let resources = test_server::single_remote_quota().with_remote_io_limits(limits);
        let lease = resources
            .acquire(Executor::Cloudflare, &ResourceRequest::default())
            .await;
        let capture = lease.capture_budget();
        if occupied {
            capture.reserve_upfront().await.unwrap();
        }
        let (response, mut hold) = Response::empty(503).held();
        let (url, server) = test_server::spawn(vec![response]).await;
        let workspace = tempfile::tempdir().unwrap();
        let cache = CacheProvider::open_local(workspace.path().join("cache"));
        let argv = vec!["true".into()];
        let env = BTreeMap::new();
        let request = ResourceRequest::default();
        let command = PreparedCommand {
            argv: &argv,
            env: &env,
            cwd: None,
            inputs: &[],
            outputs: &[],
            resources: &request,
            redirect: crate::local::Redirect::default(),
            timeout_ms: None,
            success_exit_codes: &[0],
            lease: Some(lease),
        };
        let remote = RemoteExecution::provider(PROVIDER);
        let mut execution = Box::pin(execute(
            &remote,
            command,
            workspace.path(),
            &cache,
            false,
            Client::new(&url, "token".into(), limits).unwrap(),
        ));
        if occupied {
            test_server::assert_pending(&mut execution).await;
            assert!(
                hold.arrived.try_recv().is_err(),
                "provisioned before capture admission"
            );
            drop(capture);
            let (result, ()) = test_server::assert_ready(async {
                tokio::join!(execution, async {
                    (&mut hold.arrived).await.unwrap();
                    hold.release.send(()).unwrap();
                })
            })
            .await;
            assert!(matches!(
                result,
                Err(Error::RemoteProviderStatus { status: 503, .. })
            ));
            test_server::assert_ready(server).await.unwrap();
        } else {
            let error = test_server::assert_ready(execution).await.unwrap_err();
            assert!(matches!(
                error,
                Error::ResourceRequirementUnsatisfied { .. }
            ));
            assert!(
                hold.arrived.try_recv().is_err(),
                "provisioned despite an impossible capture allowance"
            );
            assert!(records(workspace.path()).is_empty());
            server.abort();
        }
    }
}

#[tokio::test]
async fn captures_use_their_own_budget_and_share_one_byte_limit() {
    for allowance in [6, 7] {
        let limits = crate::RemoteIoLimits {
            max_staging_bytes: 65536,
            max_capture_staging_bytes: 65536,
            max_capture_bytes: allowance,
            ..crate::RemoteIoLimits::default()
        };
        let resources = test_server::single_remote_quota().with_remote_io_limits(limits);
        let request = ResourceRequest::default();
        let lease = resources.acquire(Executor::Cloudflare, &request).await;
        let capture = lease.capture_budget();
        capture.reserve_upfront().await.unwrap();
        let _transfer = lease.acquire_staging(65536).await.unwrap();
        let mut stdout = Response::empty(200);
        stdout.body = b"four".to_vec();
        let mut stderr = Response::empty(200);
        stderr.body = b"err".to_vec();
        let (url, server) =
            test_server::spawn(vec![Response::json(r#"{"exitCode":0}"#), stdout, stderr]).await;
        let workspace = tempfile::tempdir().unwrap();
        let cache = CacheProvider::open_local(workspace.path().join("cache"));
        let argv = vec!["true".into()];
        let env = BTreeMap::new();
        let command = PreparedCommand {
            argv: &argv,
            env: &env,
            cwd: None,
            inputs: &[],
            outputs: &[],
            resources: &request,
            redirect: crate::local::Redirect::default(),
            timeout_ms: None,
            success_exit_codes: &[0],
            lease: Some(lease),
        };
        let client = Client::new(&url, "token".into(), limits).unwrap();
        let sandbox = Sandbox { id: "owned".into() };
        let result = test_server::assert_ready(Box::pin(run(
            &client,
            &sandbox,
            &command,
            workspace.path(),
            &cache,
            false,
            Some(&capture),
        )))
        .await;
        if allowance == 7 {
            let result = result.unwrap();
            assert_eq!(
                result.stdout,
                Some(struo_cas::Digest::from_bytes(
                    *blake3::hash(b"four").as_bytes()
                ))
            );
            assert_eq!(
                result.stderr,
                Some(struo_cas::Digest::from_bytes(
                    *blake3::hash(b"err").as_bytes()
                ))
            );
        } else {
            assert!(matches!(
                result,
                Err(Error::ExecutorResourceLimit { limit: 2, .. })
            ));
        }
        test_server::assert_ready(server).await.unwrap();
    }
}
