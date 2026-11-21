use base64::prelude::*;
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

use super::*;

pub(super) async fn accept(
    stream: tokio::net::TcpStream,
) -> (
    tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    Value,
) {
    let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
    let init = socket.next().await.unwrap().unwrap();
    let init: Value = serde_json::from_slice(&init.into_data()).unwrap();
    assert_eq!(init["data"]["auth"]["apiToken"], "test-token");
    assert_eq!(init["data"]["context"]["encoding"], "base64");
    assert_eq!(init["data"]["context"]["tty"], false);
    send(
        &mut socket,
        json!({"type":"init", "data":{"auth":"successful"}}),
    )
    .await;
    (socket, init)
}

pub(super) async fn send(
    socket: &mut tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    value: Value,
) {
    socket
        .send(Message::Text(value.to_string().into()))
        .await
        .unwrap();
}

pub(super) async fn input(
    socket: &mut tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    loop {
        let frame = socket.next().await.unwrap().unwrap();
        let frame: Value = serde_json::from_slice(&frame.into_data()).unwrap();
        if frame["type"] == "stdInEnd" {
            return bytes;
        }
        assert_eq!(frame["type"], "payload");
        bytes.extend(
            BASE64_STANDARD
                .decode(frame["data"].as_str().unwrap())
                .unwrap(),
        );
    }
}

pub(super) async fn finish(
    socket: &mut tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    code: i32,
    stdout: &[u8],
    stderr: &[u8],
) {
    send(socket, json!({"type":"completion", "data":{"exitCode":code,"status":if code == 0 {"Success"} else {"Failure"}}})).await;
    // Output after completion is legal and must not be truncated.
    send(
        socket,
        json!({"type":"stdOut", "data":BASE64_STANDARD.encode(stdout)}),
    )
    .await;
    send(
        socket,
        json!({"type":"stdErr", "data":BASE64_STANDARD.encode(stderr)}),
    )
    .await;
    socket.close(None).await.unwrap();
}

#[tokio::test]
async fn binary_upload_and_separate_capture_survive_completion_before_output() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let payload: Vec<u8> = (0..=u8::MAX).cycle().take(100_003).collect();
    let expected = payload.clone();
    let task = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let (mut socket, _) = accept(stream).await;
        assert_eq!(input(&mut socket).await, expected);
        finish(&mut socket, 17, b"out\0\xff", b"err\x80").await;
    });
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("input");
    let stdout = temp.path().join("stdout");
    let stderr = temp.path().join("stderr");
    std::fs::write(&source, payload).unwrap();
    let code = client(&url)
        .exec(
            "owned",
            &["cat".into()],
            Some(&source),
            &stdout,
            &stderr,
            10,
            None,
        )
        .await
        .unwrap();
    assert_eq!(code, 17);
    assert_eq!(std::fs::read(stdout).unwrap(), b"out\0\xff");
    assert_eq!(std::fs::read(stderr).unwrap(), b"err\x80");
    task.await.unwrap();
}

#[tokio::test]
async fn capture_limit_is_shared_and_premature_close_is_not_success() {
    for premature in [false, true] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (mut socket, _) = accept(stream).await;
            input(&mut socket).await;
            if premature {
                socket.close(None).await.unwrap();
            } else {
                finish(&mut socket, 0, b"123", b"456").await;
            }
        });
        let temp = tempfile::tempdir().unwrap();
        let result = client(&url)
            .exec(
                "owned",
                &["true".into()],
                None,
                &temp.path().join("out"),
                &temp.path().join("err"),
                5,
                None,
            )
            .await;
        if premature {
            assert!(result
                .unwrap_err()
                .to_string()
                .contains("before command completion"));
        } else {
            assert!(matches!(
                result,
                Err(crate::Error::ExecutorResourceLimit { .. })
            ));
        }
        task.await.unwrap();
    }
}
