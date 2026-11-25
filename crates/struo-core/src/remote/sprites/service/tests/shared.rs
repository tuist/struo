mod support;

use std::sync::Arc;

use struo_cas::{remote::blob_key, CacheProvider, Digest, RemoteAccess, RemoteStore};
use tokio::io::AsyncReadExt;

use support::{run, Endpoint, Worker};

#[tokio::test]
async fn independent_workers_exchange_artifacts_through_a_real_shared_cache() {
    let root = tempfile::tempdir().unwrap();
    let cache_server = Endpoint::cache(root.path()).await;
    let remote = Arc::new(cache_server.remote());
    let cache = CacheProvider::remote(
        root.path().join("local"),
        remote.clone(),
        RemoteAccess::ReadWrite,
    );
    let Some(first) = Worker::start(&root.path().join("first"), 128 * 1024 * 1024).await else {
        return;
    };
    let Some(second) = Worker::start(&root.path().join("second"), 128 * 1024 * 1024).await else {
        return;
    };
    let workspace = root.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::write(workspace.join("source"), "17\n").unwrap();
    run(&first.service, &cache, &workspace, "source", &["dependency", "tree"],
        "read n <source; echo $((n + 25)) >dependency; mkdir tree; cp dependency tree/value; chmod +x tree/value; ln -s value tree/link; test -z \"${SPRITES_TOKEN-}\"; test -z \"${SHARED_CACHE_TOKEN-}\"; echo first").await;
    let digest = Digest::of_bytes(b"42\n");
    let mut body = remote.get(&blob_key(&digest)).await.unwrap().unwrap();
    let mut bytes = Vec::new();
    body.reader.read_to_end(&mut bytes).await.unwrap();
    assert_eq!(bytes, b"42\n");
    assert_eq!(
        std::fs::read_to_string(workspace.join("tree/link")).unwrap(),
        "42\n"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_ne!(
            std::fs::metadata(workspace.join("tree/value"))
                .unwrap()
                .permissions()
                .mode()
                & 0o111,
            0
        );
    }
    run(
        &second.service,
        &cache,
        &workspace,
        "tree",
        &["answer"],
        "read n <tree/link; echo $((n * 2)) >answer; echo second",
    )
    .await;
    assert_eq!(
        std::fs::read_to_string(workspace.join("answer")).unwrap(),
        "84\n"
    );
    let requests = second.endpoint.requests.lock().unwrap();
    assert!(requests.iter().any(|line| line == "POST /v1/cas/pull"));
    assert!(
        !requests.iter().any(|line| line.starts_with("PUT ")),
        "coordinator relayed an input: {requests:?}"
    );
    assert!(
        !requests.iter().any(|line| line.starts_with("GET /v1/cas/")),
        "coordinator fetched worker outputs instead of shared outputs: {requests:?}"
    );
    assert!(cache_server
        .requests
        .lock()
        .unwrap()
        .iter()
        .any(|line| line == &format!("GET /v1.cas.{}", digest.to_hex())));
}

#[tokio::test]
#[expect(clippy::too_many_lines)]
async fn read_only_corruption_outages_and_small_budgets_preserve_execution() {
    let root = tempfile::tempdir().unwrap();
    let mut cache_server = Endpoint::cache(root.path()).await;
    let remote = Arc::new(cache_server.remote());
    let writable = CacheProvider::remote(
        root.path().join("local"),
        remote.clone(),
        RemoteAccess::ReadWrite,
    );
    let readonly = CacheProvider::remote(
        root.path().join("readonly"),
        remote.clone(),
        RemoteAccess::ReadOnly,
    );
    let Some(worker) = Worker::start(&root.path().join("worker"), 128 * 1024 * 1024).await else {
        return;
    };
    let workspace = root.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let seed = b"readonly input\n";
    std::fs::write(workspace.join("input"), seed).unwrap();
    remote
        .put_bytes(
            &blob_key(&Digest::of_bytes(seed)),
            bytes::Bytes::from_static(seed),
        )
        .await
        .unwrap();
    cache_server.requests.lock().unwrap().clear();
    run(
        &worker.service,
        &readonly,
        &workspace,
        "input",
        &["result"],
        "cat input >result; echo read-only",
    )
    .await;
    assert_eq!(std::fs::read(workspace.join("result")).unwrap(), seed);
    assert!(!cache_server
        .requests
        .lock()
        .unwrap()
        .iter()
        .any(|line| line.starts_with("PUT ")));
    assert!(!worker
        .endpoint
        .requests
        .lock()
        .unwrap()
        .iter()
        .any(|line| line.starts_with("PUT ")));

    let original = b"correct source\n";
    std::fs::write(workspace.join("input"), original).unwrap();
    remote
        .put_bytes(
            &blob_key(&Digest::of_bytes(original)),
            bytes::Bytes::from_static(b"corrupt source\n"),
        )
        .await
        .unwrap();
    worker.endpoint.requests.lock().unwrap().clear();
    run(
        &worker.service,
        &writable,
        &workspace,
        "input",
        &["result"],
        "cat input >result; echo integrity",
    )
    .await;
    assert_eq!(std::fs::read(workspace.join("result")).unwrap(), original);
    assert!(worker
        .endpoint
        .requests
        .lock()
        .unwrap()
        .iter()
        .any(|line| line.starts_with("PUT /v1/cas/")));

    cache_server.stop();
    worker.endpoint.requests.lock().unwrap().clear();
    std::fs::write(workspace.join("input"), "cache unavailable\n").unwrap();
    run(
        &worker.service,
        &writable,
        &workspace,
        "input",
        &["result"],
        "cat input >result; echo outage",
    )
    .await;
    assert_eq!(
        std::fs::read_to_string(workspace.join("result")).unwrap(),
        "cache unavailable\n"
    );
    {
        let requests = worker.endpoint.requests.lock().unwrap();
        assert_eq!(
            requests
                .iter()
                .filter(|line| line.as_str() == "POST /v1/actions")
                .count(),
            1
        );
        assert!(requests.iter().any(|line| line.starts_with("GET /v1/cas/")));
    }

    let Some(small) = Worker::start(&root.path().join("small"), 32 * 1024 * 1024).await else {
        return;
    };
    run(
        &small.service,
        &writable,
        &workspace,
        "input",
        &["result"],
        "cat input >result; echo budget",
    )
    .await;
    assert_eq!(
        std::fs::read_to_string(workspace.join("result")).unwrap(),
        "cache unavailable\n"
    );
}
