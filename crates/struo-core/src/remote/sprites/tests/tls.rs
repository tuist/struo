use std::sync::Arc;

use futures::SinkExt;
use tokio_rustls::{rustls, TlsAcceptor};
use tokio_tungstenite::tungstenite::Message;

use super::super::*;

#[tokio::test]
async fn exec_upgrades_over_http1_when_tls_server_also_offers_http2() {
    let certificate = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let mut tls = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::aws_lc_rs::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(
        vec![certificate.cert.der().clone()],
        rustls::pki_types::PrivatePkcs8KeyDer::from(certificate.signing_key.serialize_der()).into(),
    )
    .unwrap();
    tls.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!(
        "https://localhost:{}",
        listener.local_addr().unwrap().port()
    );
    let mut client = Client::new(&endpoint, "test-token".into()).unwrap();
    client.http = Client::http_builder()
        .tls_certs_only([reqwest::Certificate::from_der(certificate.cert.der()).unwrap()])
        .build()
        .unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let stream = TlsAcceptor::from(Arc::new(tls))
            .accept(stream)
            .await
            .unwrap();
        assert_eq!(stream.get_ref().1.alpn_protocol(), Some(&b"http/1.1"[..]));
        let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
        socket
            .send(Message::Binary(vec![1, b'o', 0, 255].into()))
            .await
            .unwrap();
        socket
            .send(Message::Binary(vec![2, b'e'].into()))
            .await
            .unwrap();
        socket
            .send(Message::Binary(vec![3, 7].into()))
            .await
            .unwrap();
    });
    let sprite: Sprite =
        serde_json::from_value(serde_json::json!({"name": "struo-tls", "id": "tls-id"})).unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let cache = CacheProvider::open_local(workspace.path().join("cache"));
    let result = client
        .execute(
            &sprite,
            &["node".into()],
            None,
            &BTreeMap::new(),
            &cache,
            false,
            None,
        )
        .await;
    server.await.unwrap();
    let result = result.unwrap();
    assert_eq!(result.exit_code, 7);
    assert_eq!(
        cache.get_blob(&result.stdout.unwrap()).await.unwrap(),
        b"o\0\xff"
    );
    assert_eq!(cache.get_blob(&result.stderr.unwrap()).await.unwrap(), b"e");
}
