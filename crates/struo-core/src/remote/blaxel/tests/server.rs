use std::collections::BTreeMap;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::remote::test_server::Response;

pub(super) struct Request {
    pub method: String,
    pub path: String,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

pub(super) async fn spawn(
    responses: Vec<Response>,
) -> (String, tokio::task::JoinHandle<Vec<Request>>) {
    start(responses, None, None).await
}

#[cfg(unix)]
pub(super) async fn spawn_executing(
    responses: Vec<Response>,
) -> (String, tokio::task::JoinHandle<Vec<Request>>) {
    start(responses, None, Some(tempfile::tempdir().unwrap())).await
}

pub(super) struct Hold {
    pub arrived: tokio::sync::oneshot::Receiver<()>,
    pub release: tokio::sync::oneshot::Sender<()>,
}

pub(super) async fn spawn_held(
    responses: Vec<Response>,
    index: usize,
) -> (String, tokio::task::JoinHandle<Vec<Request>>, Hold) {
    let (arrived_tx, arrived) = tokio::sync::oneshot::channel();
    let (release, release_rx) = tokio::sync::oneshot::channel();
    let (url, task) = start(responses, Some((index, arrived_tx, release_rx)), None).await;
    (url, task, Hold { arrived, release })
}

type Held = (
    usize,
    tokio::sync::oneshot::Sender<()>,
    tokio::sync::oneshot::Receiver<()>,
);

async fn start(
    responses: Vec<Response>,
    mut held: Option<Held>,
    guest: Option<tempfile::TempDir>,
) -> (String, tokio::task::JoinHandle<Vec<Request>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let mut requests = Vec::new();
        for (index, mut response) in responses.into_iter().enumerate() {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            let end = loop {
                let mut chunk = [0; 8192];
                let n = stream.read(&mut chunk).await.unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&chunk[..n]);
                if let Some(end) = bytes.windows(4).position(|s| s == b"\r\n\r\n") {
                    break end + 4;
                }
            };
            let request_headers = String::from_utf8_lossy(&bytes[..end]);
            let mut lines = request_headers.lines();
            let first = lines.next().unwrap().split_whitespace().collect::<Vec<_>>();
            let method = first[0].to_string();
            let path = first[1].to_string();
            let headers: BTreeMap<String, String> = lines
                .filter_map(|s| s.split_once(':'))
                .map(|(k, v)| (k.to_ascii_lowercase(), v.trim().into()))
                .collect();
            let length = headers
                .get("content-length")
                .map_or(0, |s| s.parse::<usize>().unwrap());
            while bytes.len() < end + length {
                let mut chunk = [0; 8192];
                let n = stream.read(&mut chunk).await.unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&chunk[..n]);
            }
            let body = bytes[end..end + length].to_vec();
            if response.body == b"CREATE" {
                let mut created: serde_json::Value = serde_json::from_slice(&body).unwrap();
                created["status"] = "DEPLOYED".into();
                response.body = serde_json::to_vec(&created).unwrap();
            }
            if response.body == b"EXECUTE" {
                let guest = guest.as_ref().unwrap();
                let execute: serde_json::Value = serde_json::from_slice(&body).unwrap();
                let command = execute["command"].as_str().unwrap().replace(
                    "/tmp/struo-output-",
                    &format!("{}/struo-output-", guest.path().display()),
                );
                let mut child = std::process::Command::new("sh");
                child.args(["-c", &command]).current_dir(guest.path());
                for (key, value) in execute["env"].as_object().unwrap() {
                    child.env(key, value.as_str().unwrap());
                }
                child.envs([
                    ("PWD", "/provider/workdir"),
                    ("HOME", "/provider/home"),
                    ("USER", "provider"),
                    ("LOGNAME", "provider"),
                ]);
                let output = tokio::task::spawn_blocking(move || child.output().unwrap())
                    .await
                    .unwrap();
                assert!(output.stdout.is_empty(), "uncaptured stdout: {output:?}");
                assert!(output.stderr.is_empty(), "uncaptured stderr: {output:?}");
                response = super::completed(output.status.code().unwrap());
            }
            if response.body == b"CAPTURE" {
                let name = path.rsplit('/').next().unwrap().replace("%2F", "/");
                let name = name.strip_prefix("/tmp/").unwrap();
                response = super::bytes(
                    &std::fs::read(guest.as_ref().unwrap().path().join(name)).unwrap(),
                );
            }
            requests.push(Request {
                method,
                path,
                headers,
                body,
            });
            if held.as_ref().is_some_and(|(target, _, _)| *target == index) {
                let (_, arrived, release) = held.take().unwrap();
                let _ = arrived.send(());
                let _ = release.await;
            }
            let response_headers = format!("HTTP/1.1 {} Response\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", response.status, response.content_type, response.body.len());
            let _ = stream.write_all(response_headers.as_bytes()).await;
            let _ = stream.write_all(&response.body).await;
            let _ = stream.shutdown().await;
        }
        requests
    });
    (format!("http://{address}"), task)
}
