mod limits;
mod protocol;
mod storage;

use std::net::SocketAddr;
use std::path::Path;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

use super::*;

const AUTH: &str = "Bearer test-secret";

struct Server {
    address: SocketAddr,
    task: JoinHandle<io::Result<()>>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Server {
    async fn start(path: &Path, capacity: u64) -> Self {
        Self::budgeted(path, capacity, TransferBudget::default()).await
    }

    async fn budgeted(path: &Path, capacity: u64, budget: TransferBudget) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let path = path.to_owned();
        let task = tokio::spawn(async move {
            serve(listener, path, "test-secret".to_owned(), capacity, budget).await
        });
        let server = Self { address, task };
        // An actual request is also a startup barrier for cleanup and quota scans.
        assert_eq!(server.request("HEAD", &key(999), b"").await.status, 404);
        server
    }

    async fn request(&self, method: &str, path: &str, body: &[u8]) -> Response {
        self.raw(
            method,
            path,
            AUTH,
            &format!("Content-Length: {}\r\n", body.len()),
            body,
        )
        .await
    }

    async fn raw(
        &self,
        method: &str,
        path: &str,
        auth: &str,
        headers: &str,
        body: &[u8],
    ) -> Response {
        let mut socket = TcpStream::connect(self.address).await.unwrap();
        socket
            .write_all(wire(method, path, auth, headers).as_bytes())
            .await
            .unwrap();
        socket.write_all(body).await.unwrap();
        socket.shutdown().await.unwrap();
        response(&mut socket).await
    }

    async fn stop(self) {
        self.task.abort();
        while !self.task.is_finished() {
            tokio::task::yield_now().await;
        }
        // JoinSet cancellation schedules the children for destruction.
        tokio::task::yield_now().await;
    }
}

fn key(number: u64) -> String {
    format!("/v1.cas.{number:064x}")
}

fn wire(method: &str, path: &str, auth: &str, headers: &str) -> String {
    let auth = if auth.is_empty() {
        String::new()
    } else {
        format!("Authorization: {auth}\r\n")
    };
    format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n{auth}{headers}\r\n"
    )
}

struct Response {
    status: u16,
    length: u64,
    body: Vec<u8>,
}

async fn response(socket: &mut TcpStream) -> Response {
    let mut bytes = Vec::new();
    timeout(Duration::from_secs(5), socket.read_to_end(&mut bytes))
        .await
        .unwrap()
        .unwrap();
    let mut headers = [httparse::EMPTY_HEADER; 20];
    let mut response = httparse::Response::new(&mut headers);
    let httparse::Status::Complete(start) = response.parse(&bytes).unwrap() else {
        panic!("incomplete response");
    };
    let value = |name: &str| {
        response
            .headers
            .iter()
            .find(|header| header.name.eq_ignore_ascii_case(name))
            .unwrap()
            .value
    };
    assert_eq!(value("connection"), b"close");
    Response {
        status: response.code.unwrap(),
        length: std::str::from_utf8(value("content-length"))
            .unwrap()
            .parse()
            .unwrap(),
        body: bytes[start..].to_vec(),
    }
}
