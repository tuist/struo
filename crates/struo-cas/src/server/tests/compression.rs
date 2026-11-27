use super::*;

#[tokio::test]
async fn compression_is_negotiated_without_changing_stored_objects_or_head() {
    let root = tempfile::tempdir().unwrap();
    let server = Server::start(root.path(), 1024 * 1024).await;
    let raw = (0u8..=255).cycle().take(256_000).collect::<Vec<_>>();
    assert_eq!(server.request("PUT", &key(1), &raw).await.status, 204);
    let client = reqwest::Client::builder().no_zstd().build().unwrap();
    let url = format!("http://{}{}", server.address, key(1));
    for (accept, compressed) in [
        ("", false),
        ("identity", false),
        ("gzip", false),
        ("zstd;q=0, *;q=1", false),
        ("zstd;q=invalid", false),
        ("gzip, ZSTD; q=0.5", true),
        ("*", true),
        ("zstd", true),
    ] {
        let response = client
            .get(&url)
            .bearer_auth("test-secret")
            .header("Accept-Encoding", accept)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["vary"], "Accept-Encoding");
        assert_eq!(
            response.headers().contains_key("content-encoding"),
            compressed
        );
        if compressed {
            assert_eq!(response.headers()["content-encoding"], "zstd");
            assert_eq!(response.headers()["x-struo-uncompressed-length"], "256000");
            assert_eq!(response.headers()["transfer-encoding"], "chunked");
        }
        let body = response.bytes().await.unwrap();
        if compressed {
            assert!(body.len() < raw.len() / 4);
            assert_eq!(zstd::stream::decode_all(body.as_ref()).unwrap(), raw);
        } else {
            assert_eq!(body.as_ref(), raw);
        }
    }
    let head = client
        .head(&url)
        .bearer_auth("test-secret")
        .header("Accept-Encoding", "zstd")
        .send()
        .await
        .unwrap();
    assert_eq!(head.headers()["content-length"], "256000");
    assert!(!head.headers().contains_key("content-encoding"));
    assert_eq!(std::fs::read(root.path().join(&key(1)[1..])).unwrap(), raw);

    let remote = std::sync::Arc::new(
        crate::remote::HttpCacheRemote::new(
            &format!("http://{}", server.address),
            None,
            crate::remote::HttpCacheAuth::Bearer("test-secret".into()),
        )
        .unwrap(),
    );
    let digest = crate::Digest::of_bytes(&raw);
    assert_eq!(
        server
            .request("PUT", &format!("/v1.cas.{}", digest.to_hex()), &raw)
            .await
            .status,
        204
    );
    let local = tempfile::tempdir().unwrap();
    let cache =
        crate::CacheProvider::remote(local.path(), remote, crate::remote::RemoteAccess::ReadOnly);
    let output = local.path().join("restored");
    cache.copy_blob_to_file(&digest, &output).await.unwrap();
    assert_eq!(std::fs::read(output).unwrap(), raw);
}

#[tokio::test]
async fn compression_falls_back_to_identity_when_memory_cannot_admit_the_encoder() {
    for memory in [CONNECTION_MEMORY, 64 * 1024 * 1024] {
        let root = tempfile::tempdir().unwrap();
        let budget = TransferBudget::new(memory, 1024 * 1024);
        let server = Server::budgeted(root.path(), 1024 * 1024, budget.clone()).await;
        let raw = vec![b'x'; 128 * 1024];
        assert_eq!(server.request("PUT", &key(1), &raw).await.status, 204);
        let held = budget
            .reserve_memory(memory - CONNECTION_MEMORY)
            .await
            .unwrap();
        let response = timeout(
            Duration::from_secs(2),
            server.raw("GET", &key(1), AUTH, "Accept-Encoding: zstd\r\n", b""),
        )
        .await
        .expect("raw fallback must not wait for encoder memory");
        assert_eq!(response.status, 200);
        assert_eq!(response.length, raw.len() as u64);
        assert_eq!(response.body, raw);
        drop(held);
        server.stop().await;
        assert_eq!(budget.available(), (memory, 1024 * 1024));
    }
}

#[tokio::test]
async fn small_blobs_and_action_records_keep_identity_encoding() {
    let root = tempfile::tempdir().unwrap();
    let budget = TransferBudget::default();
    let server = Server::budgeted(root.path(), 1024 * 1024, budget.clone()).await;
    let client = reqwest::Client::builder().no_zstd().build().unwrap();
    for (path, length, compressed) in [
        (key(1), CHUNK - 1, false),
        (key(2), CHUNK, true),
        (format!("/v1.ac.{:064x}", 3), CHUNK, false),
    ] {
        let raw = vec![b'x'; length];
        assert_eq!(server.request("PUT", &path, &raw).await.status, 204);
        let response = client
            .get(format!("http://{}{path}", server.address))
            .bearer_auth("test-secret")
            .header("Accept-Encoding", "zstd")
            .send()
            .await
            .unwrap();
        assert_eq!(
            response.headers().contains_key("content-encoding"),
            compressed
        );
        let body = response.bytes().await.unwrap();
        if compressed {
            assert_eq!(zstd::stream::decode_all(body.as_ref()).unwrap(), raw);
        } else {
            assert_eq!(body.as_ref(), raw);
        }
    }
    drop(client);
    server.stop().await;
    assert_eq!(
        budget.available(),
        (budget.memory_bytes(), budget.staging_bytes())
    );
}

#[tokio::test]
async fn compressed_and_raw_responses_share_a_persistent_connection() {
    use tokio::io::{AsyncBufReadExt, BufReader};

    let root = tempfile::tempdir().unwrap();
    let server = Server::start(root.path(), 1024 * 1024).await;
    let raw = vec![b'x'; 128 * 1024];
    assert_eq!(server.request("PUT", &key(1), &raw).await.status, 204);
    let mut socket = BufReader::new(TcpStream::connect(server.address).await.unwrap());
    let request = wire("GET", &key(1), AUTH, "Accept-Encoding: zstd\r\n")
        .replace("Connection: close", "Connection: keep-alive");
    let requests = format!("{request}{}", wire("GET", &key(1), AUTH, ""));
    socket
        .get_mut()
        .write_all(requests.as_bytes())
        .await
        .unwrap();
    let mut headers = String::new();
    loop {
        let mut line = String::new();
        socket.read_line(&mut line).await.unwrap();
        assert!(!line.is_empty());
        if line == "\r\n" {
            break;
        }
        headers.push_str(&line);
    }
    assert!(headers.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(headers.contains("Connection: keep-alive\r\n"));
    assert!(headers.contains("Content-Encoding: zstd\r\n"));
    let mut compressed = Vec::new();
    loop {
        let mut line = String::new();
        socket.read_line(&mut line).await.unwrap();
        let length = usize::from_str_radix(line.trim(), 16).unwrap();
        let start = compressed.len();
        compressed.resize(start + length, 0);
        socket.read_exact(&mut compressed[start..]).await.unwrap();
        let mut ending = [0; 2];
        socket.read_exact(&mut ending).await.unwrap();
        assert_eq!(&ending, b"\r\n");
        if length == 0 {
            break;
        }
    }
    assert_eq!(
        zstd::stream::decode_all(compressed.as_slice()).unwrap(),
        raw
    );
    let mut remaining = Vec::new();
    socket.read_to_end(&mut remaining).await.unwrap();
    let expected = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nVary: Accept-Encoding\r\nConnection: close\r\n\r\n",
        raw.len()
    );
    assert_eq!(&remaining[..expected.len()], expected.as_bytes());
    assert_eq!(&remaining[expected.len()..], raw);
}
