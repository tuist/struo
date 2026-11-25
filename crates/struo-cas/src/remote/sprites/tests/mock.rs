use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use axum::body::{to_bytes, Body};
use axum::extract::{Request, State};
use axum::response::Response;
use serde_json::{json, Value};
use tempfile::TempDir;

use super::super::{PORT, ROOT};

pub(super) struct Provider {
    pub guest: TempDir,
    pub calls: Vec<(String, String)>,
    pub sprite: Option<Value>,
    pub definition: Option<Value>,
    pub process: Option<Child>,
    pub create_status: u16,
    pub lookup_status: Option<u16>,
    pub port: u16,
}

impl Provider {
    fn file(&self, remote: &str) -> PathBuf {
        self.guest
            .path()
            .join(remote.strip_prefix(ROOT).unwrap().trim_start_matches('/'))
    }

    pub fn stop(&mut self) {
        if let Some(mut process) = self.process.take() {
            let _ = process.kill();
            let _ = process.wait();
        }
    }

    fn start(&mut self) {
        if self.process.is_some() || self.definition.is_none() {
            return;
        }
        let args = self.definition.as_ref().unwrap()["args"]
            .as_array()
            .unwrap();
        assert_eq!(args[3], PORT.to_string());
        self.process = Some(
            Command::new("python3")
                .arg(self.file(args[0].as_str().unwrap()))
                .arg(self.file(args[1].as_str().unwrap()))
                .arg(args[2].as_str().unwrap())
                .arg(self.port.to_string())
                .arg(self.file(args[4].as_str().unwrap()))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap(),
        );
    }

    pub fn writes(&self) -> usize {
        self.calls
            .iter()
            .filter(|(method, _)| method != "GET")
            .count()
    }
}

impl Drop for Provider {
    fn drop(&mut self) {
        self.stop();
    }
}

pub(super) struct Fixture {
    pub endpoint: String,
    pub state: Arc<Mutex<Provider>>,
    task: tokio::task::JoinHandle<()>,
}

impl Fixture {
    pub async fn new() -> Self {
        let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = socket.local_addr().unwrap().port();
        drop(socket);
        let state = Arc::new(Mutex::new(Provider {
            guest: TempDir::new().unwrap(),
            calls: Vec::new(),
            sprite: None,
            definition: None,
            process: None,
            create_status: 200,
            lookup_status: None,
            port,
        }));
        let app = axum::Router::new()
            .fallback(handle)
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self {
            endpoint,
            state,
            task,
        }
    }

    pub fn remote(&self, root: &Path) -> super::super::SpritesCacheRemote {
        super::super::SpritesCacheRemote::new(
            root,
            4 * 1024 * 1024,
            &self.endpoint,
            "api-secret-not-a-cache-token".into(),
        )
        .unwrap()
    }

    pub fn cache_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.state.lock().unwrap().port)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
        self.state.lock().unwrap().stop();
    }
}

async fn handle(State(state): State<Arc<Mutex<Provider>>>, request: Request) -> Response {
    assert_eq!(
        request.headers()["authorization"],
        "Bearer api-secret-not-a-cache-token"
    );
    let method = request.method().as_str().to_string();
    let uri = request.uri().clone();
    let body = to_bytes(request.into_body(), 1024 * 1024).await.unwrap();
    let mut state = state.lock().unwrap();
    state.calls.push((method.clone(), uri.path().into()));
    let parts: Vec<_> = uri.path().trim_start_matches('/').split('/').collect();
    match (method.as_str(), parts.as_slice()) {
        ("POST", ["v1", "sprites"]) => {
            assert!(state.sprite.is_none(), "must not create a second cache");
            let input: Value = serde_json::from_slice(&body).unwrap();
            state.sprite = Some(json!({"id": "original-id", "name": input["name"],
                "url": format!("http://127.0.0.1:{}", state.port), "url_settings": input["url_settings"]}));
            response(state.create_status, state.sprite.as_ref().unwrap())
        }
        ("GET", ["v1", "sprites", _]) => {
            if let Some(status) = state.lookup_status {
                return response(status, &json!({}));
            }
            state.start();
            response(
                if state.sprite.is_some() { 200 } else { 404 },
                &json!(state.sprite),
            )
        }
        ("PUT", ["v1", "sprites", _]) => {
            let input: Value = serde_json::from_slice(&body).unwrap();
            state.sprite.as_mut().unwrap()["url_settings"] = input["url_settings"].clone();
            response(200, state.sprite.as_ref().unwrap())
        }
        ("PUT", ["v1", "sprites", _, "fs", "write"]) => {
            let url = reqwest::Url::parse(&format!("http://localhost{uri}")).unwrap();
            let query: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
            assert_eq!(query["mode"], "0600");
            assert_eq!(query["mkdir"], "true");
            let path = state.file(&query["path"]);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
            response(200, &json!({}))
        }
        ("POST", ["v1", "sprites", _, "fs", "rename"]) => {
            let input: Value = serde_json::from_slice(&body).unwrap();
            std::fs::rename(
                state.file(input["source"].as_str().unwrap()),
                state.file(input["dest"].as_str().unwrap()),
            )
            .unwrap();
            response(200, &json!({}))
        }
        ("GET", ["v1", "sprites", _, "services", "struo-cache"]) => response(
            if state.definition.is_some() { 200 } else { 404 },
            &json!(state.definition),
        ),
        ("PUT", ["v1", "sprites", _, "services", "struo-cache"]) => {
            let definition: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(definition["cmd"], "python3");
            assert_eq!(definition["http_port"], PORT);
            state.definition = Some(definition);
            state.start();
            response(200, &json!({"type": "complete"}))
        }
        ("POST", ["v1", "sprites", _, "services", "struo-cache", "restart"]) => {
            state.stop();
            state.start();
            response(200, &json!({"type": "complete"}))
        }
        _ => panic!("unexpected {method} {uri}"),
    }
}

fn response(status: u16, value: &Value) -> Response {
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(value).unwrap()))
        .unwrap()
}
