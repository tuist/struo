use super::*;

#[tokio::test]
async fn connection_limit_and_header_bound() {
    let root = tempfile::tempdir().unwrap();
    let server = Server::start(root.path(), 1024 * 1024).await;
    let mut clients = Vec::new();
    for _ in 0..8 {
        let mut client = TcpStream::connect(server.address).await.unwrap();
        client.write_all(b"GET /").await.unwrap();
        clients.push(client);
    }
    // Wait until every socket has consumed a connection slot.
    tokio::time::sleep(Duration::from_millis(50)).await;
    let mut excess = TcpStream::connect(server.address).await.unwrap();
    assert_eq!(
        timeout(Duration::from_secs(2), excess.read(&mut [0; 1]))
            .await
            .unwrap()
            .unwrap(),
        0
    );
    drop(clients);
    tokio::time::sleep(Duration::from_millis(50)).await;
    let headers = format!("X-Large: {}\r\n", "x".repeat(CHUNK));
    assert_eq!(
        server.raw("GET", &key(1), AUTH, &headers, b"").await.status,
        400
    );
}

#[tokio::test]
async fn shared_transfer_memory_admission_and_staging_limits() {
    let root = tempfile::tempdir().unwrap();
    let budget = TransferBudget::new((2 * CHUNK) as u64, 4096);
    let server = Server::budgeted(root.path(), 1024 * 1024, budget.clone()).await;
    let memory = budget.reserve_memory((2 * CHUNK) as u64).await.unwrap();
    let mut client = TcpStream::connect(server.address).await.unwrap();
    client
        .write_all(wire("GET", &key(1), AUTH, "").as_bytes())
        .await
        .unwrap();
    let mut first = [0; 1];
    assert!(timeout(Duration::from_millis(50), client.read(&mut first))
        .await
        .is_err());
    drop(memory);
    assert_eq!(response(&mut client).await.status, 404);
    assert_eq!(
        server.request("PUT", &key(1), &vec![0; 4097]).await.status,
        507
    );
    assert_eq!(server.request("PUT", &key(1), b"small").await.status, 204);
    assert_eq!(budget.available(), (2 * CHUNK as u64, 4096));
}

#[tokio::test(start_paused = true)]
async fn idle_connections_timeout_and_release_reservations() {
    let root = tempfile::tempdir().unwrap();
    let budget = TransferBudget::new((2 * CHUNK) as u64, 4096);
    let server = Server::budgeted(root.path(), 1024 * 1024, budget.clone()).await;
    let mut client = TcpStream::connect(server.address).await.unwrap();
    client.write_all(b"GET /").await.unwrap();
    tokio::time::sleep(Duration::from_secs(11)).await;
    assert_eq!(response(&mut client).await.status, 408);
    assert_eq!(budget.available(), (2 * CHUNK as u64, 4096));
}

#[tokio::test]
async fn active_download_cannot_be_evicted() {
    let root = tempfile::tempdir().unwrap();
    let length = 16 * 1024 * 1024;
    let server = Server::start(root.path(), length).await;
    let payload = vec![b'x'; usize::try_from(length).unwrap()];
    assert_eq!(server.request("PUT", &key(1), &payload).await.status, 204);
    let mut reader = TcpStream::connect(server.address).await.unwrap();
    reader
        .write_all(wire("GET", &key(1), AUTH, "").as_bytes())
        .await
        .unwrap();
    let mut header = Vec::new();
    while !header.ends_with(b"\r\n\r\n") {
        header.push(reader.read_u8().await.unwrap());
    }
    let path = key(2);
    let writer = server.request("PUT", &path, b"");
    tokio::pin!(writer);
    assert!(timeout(Duration::from_millis(100), &mut writer)
        .await
        .is_err());
    assert!(root.path().join(&key(1)[1..]).exists());
    assert!(!root.path().join(&key(2)[1..]).exists());
    let mut received = Vec::new();
    reader.read_to_end(&mut received).await.unwrap();
    assert_eq!(received, payload);
    assert_eq!(writer.await.status, 204);
    assert_eq!(server.request("GET", &key(1), b"").await.status, 404);
}
