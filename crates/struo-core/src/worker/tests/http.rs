use std::time::Duration;

use struo_cas::Digest;
use tokio::io::AsyncWriteExt;

use super::{action, executor, Response};

const TOKEN: &str = "0123456789abcdef0123456789abcdef";

struct Service {
    root: tempfile::TempDir,
    url: String,
    task: tokio::task::JoinHandle<crate::Result<()>>,
    client: reqwest::Client,
}

impl Drop for Service {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Service {
    async fn start(capacity: u64) -> Option<Self> {
        let root = tempfile::tempdir().unwrap();
        let executor = executor(root.path());
        if let Err(error) = executor.probe(root.path()).await {
            eprintln!("ISOLATION NOT EXERCISED: {error}");
            return None;
        }
        let mut options = executor.options;
        options.capacity_bytes = capacity;
        std::fs::create_dir(root.path().join("action-stale")).unwrap();
        std::fs::write(root.path().join("action-stale/secret"), "old").unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(super::super::serve(
            listener,
            root.path().to_path_buf(),
            TOKEN.into(),
            options,
        ));
        let service = Self {
            root,
            url,
            task,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap(),
        };
        let health = service
            .client
            .get(format!("{}/v1/health", service.url))
            .bearer_auth(TOKEN)
            .send()
            .await
            .unwrap();
        assert_eq!(health.status(), 200);
        assert!(!service.root.path().join("action-stale").exists());
        Some(service)
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.client
            .request(method, format!("{}{path}", self.url))
            .bearer_auth(TOKEN)
    }
}

#[tokio::test]
async fn authenticated_cas_endpoints_batch_missing_integrity_reuse_and_capacity() {
    let Some(service) = Service::start(4096).await else {
        return;
    };
    for path in ["/v1/health", "/v1/actions", "/v1/cas/missing"] {
        assert_eq!(
            service
                .client
                .get(format!("{}{path}", service.url))
                .send()
                .await
                .unwrap()
                .status(),
            401
        );
    }
    let digest = Digest::of_bytes(b"hello");
    let path = format!("/v1/cas/{}", digest.to_hex());
    assert_eq!(
        service
            .request(reqwest::Method::HEAD, &path)
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    let missing: Vec<Digest> = service
        .request(reqwest::Method::POST, "/v1/cas/missing")
        .json(&vec![digest])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(missing, vec![digest]);
    assert_eq!(
        service
            .request(reqwest::Method::PUT, &path)
            .body("wrong")
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    for _ in 0..2 {
        assert_eq!(
            service
                .request(reqwest::Method::PUT, &path)
                .body("hello")
                .send()
                .await
                .unwrap()
                .status(),
            200
        );
    }
    let missing: Vec<Digest> = service
        .request(reqwest::Method::POST, "/v1/cas/missing")
        .json(&vec![digest])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(missing.is_empty());
    assert_eq!(
        service
            .request(reqwest::Method::GET, &path)
            .send()
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap()
            .as_ref(),
        b"hello"
    );
    let too_large = Digest::of_bytes(&[1; 4097]);
    assert_eq!(
        service
            .request(
                reqwest::Method::PUT,
                &format!("/v1/cas/{}", too_large.to_hex())
            )
            .body(vec![1; 4097])
            .send()
            .await
            .unwrap()
            .status(),
        507
    );
}

#[tokio::test]
async fn concurrent_posts_are_serialized_and_disconnect_cleans_up() {
    let Some(service) = Service::start(1024 * 1024).await else {
        return;
    };
    let first = service
        .request(reqwest::Method::POST, "/v1/actions")
        .json(&action("touch first; sleep 0.15; echo first"))
        .send();
    let second = service
        .request(reqwest::Method::POST, "/v1/actions")
        .json(&action("test ! -e first; echo second"))
        .send();
    let started = std::time::Instant::now();
    let (first, second) = tokio::join!(first, second);
    assert!(started.elapsed() >= Duration::from_millis(150));
    for response in [first.unwrap(), second.unwrap()] {
        assert_eq!(response.status(), 200);
        assert_eq!(
            response.json::<Response>().await.unwrap().result.exit_code,
            0
        );
    }
    let body = serde_json::to_vec(&action("sleep 60 & wait")).unwrap();
    let address = service.url.trim_start_matches("http://");
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    stream.write_all(format!("POST /v1/actions HTTP/1.1\r\nAuthorization: Bearer {TOKEN}\r\nContent-Length: {}\r\n\r\n", body.len()).as_bytes()).await.unwrap();
    stream.write_all(&body).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    drop(stream);
    assert_eq!(
        service
            .request(reqwest::Method::GET, "/v1/health")
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    assert!(!std::fs::read_dir(service.root.path())
        .unwrap()
        .any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("action-")));
}
