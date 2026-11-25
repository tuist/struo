use super::*;

#[tokio::test]
async fn auth_precedes_paths_methods_and_lengths() {
    let root = tempfile::tempdir().unwrap();
    let server = Server::start(root.path(), 1024 * 1024).await;
    assert_eq!(server.request("PUT", &key(1), b"secret").await.status, 204);
    for method in ["GET", "HEAD", "PUT", "DELETE"] {
        for path in [key(1), key(2), "/invalid".to_owned()] {
            for auth in [
                "",
                "Bearer wrong",
                "bearer test-secret",
                "Bearer test-secret ",
            ] {
                assert_eq!(
                    server
                        .raw(method, &path, auth, "Content-Length: 999999999\r\n", b"")
                        .await
                        .status,
                    401
                );
            }
        }
    }
    assert_eq!(
        server
            .raw(
                "GET",
                &key(1),
                AUTH,
                &format!("Authorization: {AUTH}\r\n"),
                b""
            )
            .await
            .status,
        401
    );
    assert_eq!(server.request("GET", &key(1), b"").await.body, b"secret");
}

#[tokio::test]
async fn opaque_paths_and_methods() {
    let root = tempfile::tempdir().unwrap();
    let server = Server::start(root.path(), 1024 * 1024).await;
    for path in [
        "/".to_owned(),
        key(1).to_uppercase(),
        format!("{}?x=1", key(1)),
        format!("{}#x", key(1)),
        format!("{}/", key(1)),
        key(1)[..70].to_owned(),
        format!("{}0", key(1)),
        format!("/v1.other.{}", "0".repeat(64)),
        format!("/{}", key(1)),
        format!("/..{}", key(1)),
        format!("/v1.cas.{}", "g".repeat(64)),
        format!("/%76{}", &key(1)[2..]),
        format!("http://localhost{}", key(1)),
    ] {
        assert_eq!(
            server.request("GET", &path, b"").await.status,
            400,
            "{path}"
        );
    }
    assert_eq!(server.request("DELETE", &key(1), b"").await.status, 405);
    for path in [key(1), format!("/v1.ac.{:064x}", 1)] {
        assert_eq!(server.request("PUT", &path, b"ok").await.status, 204);
        assert_eq!(server.request("GET", &path, b"").await.body, b"ok");
        let head = server.request("HEAD", &path, b"").await;
        assert_eq!(head.length, 2);
        assert!(head.body.is_empty());
    }
}

#[tokio::test]
async fn strict_framing_and_expect_rejection_before_body() {
    let root = tempfile::tempdir().unwrap();
    let server = Server::start(root.path(), 1024 * 1024).await;
    let cases = [
        (String::new(), 411),
        ("Content-Length: -1\r\n".to_owned(), 400),
        ("Content-Length: +1\r\n".to_owned(), 400),
        ("Content-Length: 1 \r\n".to_owned(), 400),
        ("Content-Length: 1\t\r\n".to_owned(), 400),
        ("Content-Length: 1, 1\r\n".to_owned(), 400),
        ("Content-Length: \r\n".to_owned(), 400),
        ("Content-Length: 1\r\nContent-Length: 1\r\n".to_owned(), 400),
        (
            "Content-Length: 0\r\nTransfer-Encoding: chunked\r\n".to_owned(),
            400,
        ),
        ("Transfer-Encoding: identity\r\n".to_owned(), 400),
        ("Content-Length: 1048577\r\n".to_owned(), 413),
        (format!("Content-Length: {}\r\n", "9".repeat(5000)), 413),
    ];
    for (headers, status) in cases {
        assert_eq!(
            server.raw("PUT", &key(1), AUTH, &headers, b"").await.status,
            status,
            "{headers}"
        );
    }
    assert_eq!(
        server
            .raw("PUT", &key(1), AUTH, "Content-Length: 0000\r\n", b"")
            .await
            .status,
        204
    );
    for (auth, length, status) in [("wrong", 1_000_000, 401), (AUTH, 999_999_999, 413)] {
        let mut socket = TcpStream::connect(server.address).await.unwrap();
        socket
            .write_all(
                wire(
                    "PUT",
                    &key(1),
                    auth,
                    &format!("Expect: 100-continue\r\nContent-Length: {length}\r\n"),
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        // Do not half-close: rejection must not wait for any upload bytes.
        assert_eq!(response(&mut socket).await.status, status);
    }
}

#[tokio::test]
async fn expect_continue_and_large_streams() {
    let root = tempfile::tempdir().unwrap();
    let server = Server::start(root.path(), 1024 * 1024).await;
    let body = (0u8..=255).cycle().take(256_000).collect::<Vec<_>>();
    let mut socket = TcpStream::connect(server.address).await.unwrap();
    socket
        .write_all(
            wire(
                "PUT",
                &key(1),
                AUTH,
                &format!("Expect: 100-continue\r\nContent-Length: {}\r\n", body.len()),
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let mut interim = [0; 25];
    socket.read_exact(&mut interim).await.unwrap();
    assert_eq!(&interim, b"HTTP/1.1 100 Continue\r\n\r\n");
    socket.write_all(&body).await.unwrap();
    assert_eq!(response(&mut socket).await.status, 204);
    for _ in 0..2 {
        assert_eq!(server.request("GET", &key(1), b"").await.body, body);
        assert_eq!(server.request("PUT", &key(1), &body).await.status, 204);
    }
    let path = key(1);
    let requests = (0..8).map(|_| server.request("PUT", &path, &body));
    for result in futures::future::join_all(requests).await {
        assert_eq!(result.status, 204);
    }
}
