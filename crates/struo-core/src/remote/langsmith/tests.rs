use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::json;

use super::*;
mod known_identity;
mod lifecycle;
mod recovery;
mod workflow;
use crate::remote::{
    lifecycle::{Lookup, Provider, MARKER_KEY},
    test_server::{self, Request, Response},
};

const ID: &str = "7fa38660-2016-4ac4-995b-b802e4b2d332";
const WORKSPACE: &str = "f675ec98-10dd-46a5-bc7d-6b2bbc91d946";

fn client(url: String) -> Client {
    Client::new(Config {
        url,
        api_key: "test-key".into(),
        workspace: WORKSPACE.into(),
        dataplane_origins: vec![],
    })
    .unwrap()
}

fn sandbox(url: &str) -> client::Sandbox {
    serde_json::from_value(json!({"id": ID, "name": "struo-test", "status": "ready", "dataplane_url": format!("{url}/api/v2/sandboxes/{ID}")})).unwrap()
}

#[allow(clippy::needless_pass_by_value)]
fn event(name: &str, body: serde_json::Value) -> String {
    format!("event: {name}\r\ndata: {body}\r\n\r\n")
}

fn sse(request: &Request, tail: &str) -> Response {
    let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
    Response {
        content_type: "text/event-stream",
        body: format!(
            "{}{}",
            event("started", json!({"command_id":body["command_id"]})),
            tail
        )
        .into_bytes(),
        ..Response::empty(200)
    }
}

#[tokio::test]
async fn resumes_binary_output_with_committed_offsets_without_restarting() {
    let tail = format!(
        "{}{}{}",
        event(
            "stdout",
            json!({"offset":0,"data":STANDARD.encode(b"out\0\xff")})
        ),
        event(
            "stderr",
            json!({"offset":0,"data":STANDARD.encode(b"err\xfe")})
        ),
        event("ack_required", json!({"stdout_offset":5,"stderr_offset":4}))
    );
    let (url, server) = test_server::spawn_with_handler(2, move |request| {
        if request.path.ends_with("/start") {
            sse(request, &tail)
        } else {
            let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
            assert_eq!(body["stdout_offset"], 5);
            assert_eq!(body["stderr_offset"], 4);
            sse(
                request,
                &format!(
                    "{}{}",
                    event("stdout", json!({"offset":5,"data":STANDARD.encode(b"end")})),
                    event("exit", json!({"exit_code":7}))
                ),
            )
        }
    })
    .await;
    let workspace = tempfile::tempdir().unwrap();
    let capture = client(url.clone())
        .execute(&sandbox(&url), &["vitest".into()], "/tmp", workspace.path())
        .await
        .unwrap();
    assert_eq!(capture.exit_code, 7);
    assert_eq!(
        std::fs::read(capture.stdout.path()).unwrap(),
        b"out\0\xffend"
    );
    assert_eq!(std::fs::read(capture.stderr.path()).unwrap(), b"err\xfe");
    let requests = test_server::assert_ready(server).await.unwrap();
    assert!(requests[1].path.ends_with("/resume"));
    for request in requests {
        assert_eq!(request.headers["x-tenant-id"], WORKSPACE);
        assert_eq!(request.headers["x-api-key"], "test-key");
    }
}

#[tokio::test]
async fn rejects_gaps_invalid_base64_missing_exit_and_shared_capture_overflow() {
    for tail in [
        event("stdout", json!({"offset":1,"data":"YQ=="})),
        event("stdout", json!({"offset":0,"data":"%%%"})),
        event("exit", json!({})),
        format!(
            "{}{}",
            event("stdout", json!({"offset":0,"data":"YWI="})),
            event("stderr", json!({"offset":0,"data":"Y2Q="}))
        ),
        format!(
            "{}{}",
            event("stdout", json!({"offset":0,"data":"YQ=="})),
            event("stream_end", json!({"stream":"stdout","offset":2}))
        ),
    ] {
        let (url, server) =
            test_server::spawn_with_handler(1, move |request| sse(request, &tail)).await;
        let mut client = client(url.clone());
        client.limits.max_capture_bytes = 3;
        let workspace = tempfile::tempdir().unwrap();
        assert!(client
            .execute(&sandbox(&url), &["vitest".into()], "/tmp", workspace.path())
            .await
            .is_err());
        test_server::assert_ready(server).await.unwrap();
    }
}

#[tokio::test]
async fn uncertain_create_lookup_is_retained_and_pagination_verifies_markers() {
    let (url, server) = test_server::spawn(vec![
        Response::json(r#"{"items":[],"next_cursor":null}"#),
        Response::json(r#"{"items":[],"next_cursor":"opaque+/="}"#),
        Response::json(&json!({"items":[{"id":ID,"name":"test","status":"ready","labels":{MARKER_KEY:"nonce"}}],"next_cursor":null}).to_string()),
    ]).await;
    let client = client(url);
    assert_eq!(client.find("nonce").await.unwrap(), Lookup::Inconclusive);
    assert_eq!(
        client.find("nonce").await.unwrap(),
        Lookup::Found(vec![ID.into()])
    );
    let requests = test_server::assert_ready(server).await.unwrap();
    assert!(requests[2].path.contains("cursor=opaque%2B%2F%3D"));
    assert!(requests
        .iter()
        .all(|r| r.path.contains("created_by=me")
            && r.path.contains("label=struo-operation%3Dnonce")));
}

#[tokio::test]
async fn deletion_requires_confirmed_uuid_absence() {
    let present = json!({"items":[{"id":ID,"name":"test","status":"deleting"}],"next_cursor":null});
    let (url, server) = test_server::spawn(vec![
        Response::empty(204),
        Response::json(&present.to_string()),
        Response::empty(204),
        Response::json(r#"{"items":[]}"#),
        Response::empty(404),
        Response::json(r#"{"items":[],"next_cursor":null}"#),
    ])
    .await;
    let client = client(url);
    assert!(client.delete(ID).await.is_err());
    assert!(client.delete(ID).await.is_err());
    client.delete(ID).await.unwrap();
    let requests = test_server::assert_ready(server).await.unwrap();
    assert_eq!(requests[0].path, format!("/api/v2/sandboxes/boxes/{ID}"));
}

#[test]
fn credentials_only_go_to_explicit_authenticated_origins() {
    let mut config = Config {
        url: "https://api.example.com".into(),
        api_key: "test-key".into(),
        workspace: WORKSPACE.into(),
        dataplane_origins: vec!["https://runtime.example.com".into()],
    };
    let client = Client::new(config.clone()).unwrap();
    for url in ["https://runtime.example.com", "https://api.example.com"] {
        assert!(client
            .runtime(&sandbox(url), reqwest::Method::POST, "/upload")
            .is_ok());
    }
    for url in [
        "https://runtime.example.com.evil.test",
        "http://runtime.example.com",
        "https://user@runtime.example.com",
        "https://runtime.example.com:8443",
        "https://evil.test",
    ] {
        assert!(client
            .runtime(&sandbox(url), reqwest::Method::POST, "/upload")
            .is_err());
    }
    config.dataplane_origins = vec!["https://runtime.example.com/path".into()];
    assert!(Client::new(config).is_err());
}
