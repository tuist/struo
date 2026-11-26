use super::*;

#[tokio::test]
async fn exact_capacity_unequal_eviction_and_head_access() {
    let root = tempfile::tempdir().unwrap();
    let server = Server::start(root.path(), 20480).await;
    assert_eq!(
        server
            .request("PUT", &key(1), &vec![b'a'; 6000])
            .await
            .status,
        204
    );
    assert_eq!(
        server
            .request("PUT", &key(2), &vec![b'b'; 10288])
            .await
            .status,
        204
    );
    server.stop().await;
    let tied = std::time::SystemTime::now() + Duration::from_secs(3600);
    for number in [1, 2] {
        std::fs::OpenOptions::new()
            .write(true)
            .open(root.path().join(key(number).trim_start_matches('/')))
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_accessed(tied))
            .unwrap();
    }
    let server = Server::start(root.path(), 20480).await;
    assert_eq!(server.request("HEAD", &key(1), b"").await.length, 6000);
    assert_eq!(server.request("PUT", &key(3), b"").await.status, 204);
    assert_eq!(server.request("GET", &key(2), b"").await.status, 404);
    assert_eq!(
        server.request("GET", &key(1), b"").await.body,
        vec![b'a'; 6000]
    );
    assert_eq!(server.request("GET", &key(3), b"").await.status, 200);
    server.stop().await;
    let server = Server::start(root.path(), 4096).await;
    assert_eq!(server.request("GET", &key(1), b"").await.status, 404);
    assert_eq!(server.request("GET", &key(3), b"").await.status, 200);
}

#[tokio::test]
async fn minimum_charge_and_replacement_staging_quota() {
    let root = tempfile::tempdir().unwrap();
    let server = Server::start(root.path(), 8192).await;
    assert_eq!(
        server.request("PUT", &key(1), b"previous").await.status,
        204
    );
    assert_eq!(
        server
            .request("PUT", &key(1), &vec![b'x'; 4097])
            .await
            .status,
        507
    );
    assert_eq!(server.request("GET", &key(1), b"").await.body, b"previous");
    assert_eq!(server.request("PUT", &key(1), b"new").await.status, 204);
    assert_eq!(server.request("PUT", &key(2), b"").await.status, 204);
    assert_eq!(server.request("GET", &key(1), b"").await.body, b"new");
    server.stop().await;
    let server = Server::start(root.path(), 4096).await;
    assert_eq!(server.request("PUT", &key(3), b"").await.status, 204);
    assert_eq!(server.request("PUT", &key(4), b"").await.status, 204);
    assert_eq!(server.request("GET", &key(3), b"").await.status, 404);
    server.stop().await;
    let server = Server::start(root.path(), 4095).await;
    assert_eq!(server.request("PUT", &key(1), b"").await.status, 507);
    assert_eq!(server.request("GET", &key(4), b"").await.status, 404);
}

#[tokio::test]
async fn interrupted_upload_and_restart_preserve_old_value() {
    let root = tempfile::tempdir().unwrap();
    let server = Server::start(root.path(), 1024 * 1024).await;
    assert_eq!(
        server.request("PUT", &key(1), b"previous").await.status,
        204
    );
    for number in [1, 2] {
        assert_eq!(
            server
                .raw(
                    "PUT",
                    &key(number),
                    AUTH,
                    "Content-Length: 80000\r\n",
                    b"partial"
                )
                .await
                .status,
            400
        );
    }
    assert_eq!(server.request("GET", &key(1), b"").await.body, b"previous");
    assert_eq!(server.request("GET", &key(2), b"").await.status, 404);
    let mut socket = TcpStream::connect(server.address).await.unwrap();
    socket
        .write_all(wire("PUT", &key(1), AUTH, "Content-Length: 100000\r\n").as_bytes())
        .await
        .unwrap();
    socket.write_all(&vec![b'x'; 70000]).await.unwrap();
    timeout(Duration::from_secs(5), async {
        loop {
            if std::fs::read_dir(root.path()).unwrap().any(|entry| {
                let entry = entry.unwrap();
                entry.file_name().to_string_lossy().starts_with(".upload-")
                    // Windows directory listings report stale sizes for open files.
                    && std::fs::metadata(entry.path()).unwrap().len() > 0
            }) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    server.stop().await;
    drop(socket);
    std::fs::write(root.path().join(".upload-abandoned"), b"incomplete").unwrap();
    let server = Server::start(root.path(), 1024 * 1024).await;
    assert_eq!(server.request("GET", &key(1), b"").await.body, b"previous");
    assert!(!std::fs::read_dir(root.path()).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".upload-")
    }));
}

#[tokio::test]
async fn restart_reconstructs_usage_and_read_order() {
    let root = tempfile::tempdir().unwrap();
    let server = Server::start(root.path(), 1024 * 1024).await;
    assert_eq!(
        server
            .request("PUT", &key(1), &vec![b'a'; 6000])
            .await
            .status,
        204
    );
    assert_eq!(
        server
            .request("PUT", &key(2), &vec![b'b'; 10288])
            .await
            .status,
        204
    );
    server.stop().await;
    let server = Server::start(root.path(), 20480).await;
    assert_eq!(server.request("GET", &key(1), b"").await.length, 6000);
    assert_eq!(server.request("HEAD", &key(2), b"").await.length, 10288);
    assert_eq!(server.request("PUT", &key(3), b"").await.status, 204);
    assert_eq!(server.request("GET", &key(1), b"").await.status, 404);
    server.stop().await;
    let server = Server::start(root.path(), 4096).await;
    assert_eq!(server.request("GET", &key(2), b"").await.status, 404);
    assert_eq!(server.request("GET", &key(3), b"").await.status, 200);
}
