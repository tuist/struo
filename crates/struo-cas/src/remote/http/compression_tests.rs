use axum::body::Body;
use axum::extract::Request;
use axum::response::Response;
use futures::StreamExt;
use tokio::io::AsyncReadExt;

use super::*;
use crate::remote::{blob_key, RemoteAccess};
use crate::{CacheProvider, Digest};

async fn compressed_server(
    bytes: Vec<u8>,
    raw_size: u64,
    size_header: Option<String>,
    encoding: &'static str,
) -> (String, tokio::task::JoinHandle<()>) {
    let app = axum::Router::new().fallback(move |request: Request| {
        let bytes = bytes.clone();
        let size_header = size_header.clone();
        async move {
            if request.method() == Method::HEAD {
                return Response::builder()
                    .header(CONTENT_LENGTH, raw_size)
                    .body(Body::empty())
                    .unwrap();
            }
            assert_eq!(request.headers()[ACCEPT_ENCODING], "zstd");
            let mut response = Response::builder().header(CONTENT_ENCODING, encoding);
            if let Some(size) = size_header {
                response = response.header("x-struo-uncompressed-length", size);
            }
            response.body(Body::from(bytes)).unwrap()
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, task)
}

#[tokio::test]
async fn generic_compressed_servers_can_supply_the_raw_size_through_head() {
    let raw = b"generic HTTP server".repeat(10_000);
    let compressed = zstd::stream::encode_all(raw.as_slice(), 3).unwrap();
    let (url, server) = compressed_server(compressed, raw.len() as u64, None, "ZsTd").await;
    let remote = HttpCacheRemote::new(&url, None, HttpCacheAuth::None).unwrap();
    let mut body = remote.get("cas/value").await.unwrap().unwrap();
    assert_eq!(body.size, raw.len() as u64);
    let mut restored = Vec::new();
    body.reader.read_to_end(&mut restored).await.unwrap();
    assert_eq!(restored, raw);
    server.abort();
}

#[tokio::test]
async fn compressed_downloads_reject_bad_frames_sizes_and_digests() {
    let raw = b"verified content".repeat(10_000);
    let digest = Digest::of_bytes(&raw);
    let compressed = zstd::stream::encode_all(raw.as_slice(), 3).unwrap();
    let mut trailing = compressed.clone();
    trailing.extend_from_slice(b"invalid trailing frame");
    let mut different = raw.clone();
    different[0] ^= 1;
    let mut large_window = zstd::stream::write::Encoder::new(Vec::new(), 3).unwrap();
    large_window.window_log(24).unwrap();
    std::io::Write::write_all(&mut large_window, &raw).unwrap();
    let cases = [
        (
            compressed[..compressed.len() - 1].to_vec(),
            raw.len() as u64,
        ),
        (trailing, raw.len() as u64),
        (compressed.clone(), raw.len() as u64 - 1),
        (compressed, raw.len() as u64 + 1),
        (
            zstd::stream::encode_all(different.as_slice(), 3).unwrap(),
            raw.len() as u64,
        ),
        (large_window.finish().unwrap(), raw.len() as u64),
    ];
    for (case, (bytes, size)) in cases.into_iter().enumerate() {
        let (url, server) = compressed_server(bytes, size, Some(size.to_string()), "zstd").await;
        let root = tempfile::tempdir().unwrap();
        let remote =
            std::sync::Arc::new(HttpCacheRemote::new(&url, None, HttpCacheAuth::None).unwrap());
        let cache = CacheProvider::remote(root.path(), remote, RemoteAccess::ReadOnly);
        let output = root.path().join("restored");
        let error = cache.copy_blob_to_file(&digest, &output).await.unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Corrupt, "case {case}: {error}");
        assert!(!output.exists());
        server.abort();
    }
}

#[tokio::test]
async fn compressed_response_headers_must_describe_a_supported_representation() {
    for (header, encoding) in [(Some("invalid".into()), "zstd"), (Some("1".into()), "gzip")] {
        let (url, server) = compressed_server(vec![0], 1, header, encoding).await;
        let remote = HttpCacheRemote::new(&url, None, HttpCacheAuth::None).unwrap();
        assert_eq!(
            remote
                .get(&blob_key(&Digest::of_bytes(b"x")))
                .await
                .unwrap_err()
                .kind(),
            ErrorKind::Protocol
        );
        server.abort();
    }
}

#[tokio::test]
async fn interrupted_compressed_transfers_remain_outages_not_corruption() {
    let raw = b"network failure".repeat(10_000);
    let size = raw.len();
    let digest = Digest::of_bytes(&raw);
    let app = axum::Router::new().fallback(move |request: Request| async move {
        if request.method() == Method::HEAD {
            return Response::builder()
                .header(CONTENT_LENGTH, size)
                .body(Body::empty())
                .unwrap();
        }
        let stream = futures::stream::once(async { Ok(Bytes::from_static(b"\x28\xb5\x2f\xfd")) })
            .chain(futures::stream::once(async {
                tokio::time::sleep(Duration::from_millis(20)).await;
                Err(std::io::Error::other("connection interrupted"))
            }));
        Response::builder()
            .header(CONTENT_ENCODING, "zstd")
            .header("x-struo-uncompressed-length", size)
            .body(Body::from_stream(stream))
            .unwrap()
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let remote =
        std::sync::Arc::new(HttpCacheRemote::new(&url, None, HttpCacheAuth::None).unwrap());
    let mut body = remote.get(&blob_key(&digest)).await.unwrap().unwrap();
    let error = body.reader.read_to_end(&mut Vec::new()).await.unwrap_err();
    assert!(matches!(error.get_ref(), Some(source) if source.is::<reqwest::Error>()));
    let root = tempfile::tempdir().unwrap();
    let cache = CacheProvider::remote(root.path(), remote, RemoteAccess::ReadOnly);
    let error = cache
        .copy_blob_to_file(&digest, &root.path().join("out"))
        .await
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Unavailable, "{error}");
    server.abort();
}
