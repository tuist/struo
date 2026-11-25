use super::*;
use crate::remote::worker_pool::Pool;
use crate::WorkerPoolSpec;

fn spec() -> WorkerPoolSpec {
    WorkerPoolSpec {
        files: BTreeMap::from([
            ("mise.toml".into(), "[tools]\nnode = '22.1.0'\n".into()),
            (
                "mise.lock".into(),
                "[[tools.node]]\nversion = '22.1.0'\n".into(),
            ),
        ]),
        ..WorkerPoolSpec::default()
    }
}

fn binary(root: &Path) -> super::super::service::bootstrap::Binary {
    let path = root.join("worker-program");
    std::fs::write(&path, b"mock worker executable").unwrap();
    super::super::service::bootstrap::Binary {
        path,
        digest: struo_cas::Digest::of_bytes(b"mock worker executable").to_hex(),
    }
}

async fn run(
    client: &Client,
    pool: &Pool,
    spec: WorkerPoolSpec,
    root: &Path,
    timeout_ms: u64,
) -> Result<ActionResult> {
    let cache = CacheProvider::open_local(root.join("cache"));
    super::super::workers::execute_in_pool(
        client,
        pool,
        spec,
        PreparedCommand {
            argv: &["node".into(), "app.js".into()],
            env: &BTreeMap::new(),
            inputs: &[],
            outputs: &[],
            cwd: None,
            resources: &ResourceRequest::default(),
            redirect: crate::local::Redirect::default(),
            timeout_ms: Some(timeout_ms),
            success_exit_codes: &[0],
            lease: None,
        },
        root,
        &cache,
        false,
        crate::remote::CommandFilesystemSetup::default(),
        &binary(root),
    )
    .await
}

#[tokio::test]
async fn reuses_preparation_and_rotates_lock_generations_within_one_slot() {
    let (client, state, server) = mock(Some(7), false).await;
    state.lock().unwrap().retained = true;
    let root = tempfile::tempdir().unwrap();
    let pool = Pool::new(&root.path().join("pool"), 1).unwrap();
    let first = spec();
    first.validate().unwrap();
    for _ in 0..2 {
        assert_eq!(
            run(&client, &pool, first.clone(), root.path(), 10000)
                .await
                .unwrap()
                .exit_code,
            7
        );
    }
    {
        let state = state.lock().unwrap();
        assert_eq!(
            state
                .requests
                .iter()
                .filter(|(method, path)| method == "POST" && path == "/v1/sprites")
                .count(),
            1
        );
        assert_eq!(
            state
                .commands
                .iter()
                .filter(|args| args.first().is_some_and(|arg| arg == "curl"))
                .count(),
            1
        );
        assert_eq!(
            state
                .requests
                .iter()
                .filter(|(_, path)| path.ends_with("/restore"))
                .count(),
            0
        );
        assert!(!state.requests.iter().any(|(method, _)| method == "DELETE"));
    }
    let reopened = Pool::new(&root.path().join("pool"), 1).unwrap();
    let slot = reopened.try_claim(&first.fingerprint()).unwrap().unwrap();
    let identity = slot.record().unwrap().provider_identity.clone();
    assert!(!slot.record().unwrap().dirty);
    drop(slot);
    let mut changed = first.clone();
    changed
        .files
        .get_mut("mise.lock")
        .unwrap()
        .push_str("# new environment\n");
    assert_eq!(
        run(&client, &reopened, changed.clone(), root.path(), 10000)
            .await
            .unwrap()
            .exit_code,
        7
    );
    let slot = reopened.try_claim(&changed.fingerprint()).unwrap().unwrap();
    assert_ne!(slot.record().unwrap().provider_identity, identity);
    assert_eq!(slot.record().unwrap().environment, changed.fingerprint());
    assert_eq!(
        state
            .lock()
            .unwrap()
            .requests
            .iter()
            .filter(|(method, _)| method == "DELETE")
            .count(),
        1
    );
    server.abort();
}

#[tokio::test]
async fn failed_reset_is_not_a_clean_worker_and_requires_recovery() {
    let (client, state, server) = mock(Some(0), false).await;
    {
        let mut state = state.lock().unwrap();
        state.retained = true;
        state.fail_restore = true;
        state.fail_service = true;
    }
    let root = tempfile::tempdir().unwrap();
    let pool = Pool::new(&root.path().join("pool"), 1).unwrap();
    assert!(run(&client, &pool, spec(), root.path(), 10000)
        .await
        .is_err());
    let slot = pool.try_claim(&spec().fingerprint()).unwrap().unwrap();
    assert!(slot.record().unwrap().dirty);
    let identity = slot.record().unwrap().provider_identity.clone();
    drop(slot);
    {
        let mut state = state.lock().unwrap();
        state.fail_restore = false;
        state.fail_service = false;
    }
    assert_eq!(
        run(&client, &pool, spec(), root.path(), 10000)
            .await
            .unwrap()
            .exit_code,
        0
    );
    let slot = pool.try_claim(&spec().fingerprint()).unwrap().unwrap();
    assert!(!slot.record().unwrap().dirty);
    assert_eq!(slot.record().unwrap().provider_identity, identity);
    server.abort();
}

#[tokio::test]
async fn cancellation_holds_worker_and_admission_until_checkpoint_restoration() {
    use crate::remote::test_server::{assert_pending, assert_ready, single_remote_quota};
    let (client, state, server) = mock(Some(0), false).await;
    let arrived = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let reset_arrived = Arc::new(tokio::sync::Notify::new());
    let reset_release = Arc::new(tokio::sync::Notify::new());
    {
        let mut state = state.lock().unwrap();
        state.retained = true;
        state.hold_action = Some((arrived.clone(), release.clone()));
        state.hold_restore = Some((reset_arrived.clone(), reset_release.clone()));
    }
    let root = tempfile::tempdir().unwrap();
    let pool = Pool::new(&root.path().join("pool"), 1).unwrap();
    let resources = single_remote_quota();
    let request = ResourceRequest::default();
    let lease = resources.acquire(Executor::Sprites, &request).await;
    let cache = CacheProvider::open_local(root.path().join("cache"));
    let env = BTreeMap::new();
    let argv = ["node".into()];
    let binary = binary(root.path());
    let mut execution = Box::pin(super::super::workers::execute_in_pool(
        &client,
        &pool,
        spec(),
        PreparedCommand {
            argv: &argv,
            env: &env,
            inputs: &[],
            outputs: &[],
            cwd: None,
            resources: &request,
            redirect: crate::local::Redirect::default(),
            timeout_ms: None,
            success_exit_codes: &[0],
            lease: Some(lease),
        },
        root.path(),
        &cache,
        false,
        crate::remote::CommandFilesystemSetup::default(),
        &binary,
    ));
    assert_ready(async {
        tokio::select! { _ = &mut execution => panic!("action finished before cancellation"), () = arrived.notified() => {} }
    }).await;
    drop(execution);
    assert!(pool.try_claim(&spec().fingerprint()).unwrap().is_none());
    assert_pending(resources.acquire(Executor::Sprites, &request)).await;
    release.notify_one();
    assert_ready(reset_arrived.notified()).await;
    assert!(pool.try_claim(&spec().fingerprint()).unwrap().is_none());
    assert_pending(resources.acquire(Executor::Sprites, &request)).await;
    reset_release.notify_one();
    assert_ready(resources.acquire(Executor::Sprites, &request)).await;
    let slot = pool.try_claim(&spec().fingerprint()).unwrap().unwrap();
    assert!(!slot.record().unwrap().dirty);
    assert!(!state
        .lock()
        .unwrap()
        .requests
        .iter()
        .any(|(method, _)| method == "DELETE"));
    server.abort();
}
