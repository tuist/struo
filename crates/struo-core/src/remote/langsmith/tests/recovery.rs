use std::time::Duration;

use tokio::io::AsyncWriteExt;

use super::*;

#[derive(Clone, Copy)]
enum Interruption {
    LostHeaders,
    StalledHeaders,
    StalledBody,
    Shutdown,
    Unavailable,
}

#[tokio::test]
async fn recoverable_interruptions_resume_same_command_at_committed_offsets() {
    for interruption in [
        Interruption::LostHeaders,
        Interruption::StalledHeaders,
        Interruption::StalledBody,
        Interruption::Shutdown,
        Interruption::Unavailable,
    ] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut first, _) = listener.accept().await.unwrap();
            let request = test_server::read_request(&mut first).await;
            assert!(request.path.ends_with("/start"));
            let start: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
            let output = matches!(
                interruption,
                Interruption::StalledBody | Interruption::Shutdown
            );
            match interruption {
                Interruption::LostHeaders => {
                    drop(first);
                }
                Interruption::StalledHeaders => {}
                Interruption::StalledBody => {
                    let response = sse(
                        &request,
                        &event("stdout", json!({"offset":0,"data":"YWJj"})),
                    );
                    first.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n", response.body.len() + 100).as_bytes()).await.unwrap();
                    first.write_all(&response.body).await.unwrap();
                }
                Interruption::Shutdown => {
                    test_server::write_response(
                        &mut first,
                        &sse(
                            &request,
                            &format!(
                                "{}{}",
                                event("stdout", json!({"offset":0,"data":"YWJj"})),
                                event(
                                    "error",
                                    json!({"error_type":"ServerShuttingDown","error":"shutdown"})
                                )
                            ),
                        ),
                    )
                    .await;
                }
                Interruption::Unavailable => {
                    test_server::write_response(&mut first, &Response::empty(503)).await;
                }
            }
            let (mut resumed, _) = listener.accept().await.unwrap();
            let request = test_server::read_request(&mut resumed).await;
            assert!(request.path.ends_with("/resume"));
            let resume: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
            assert_eq!(resume["command_id"], start["command_id"]);
            assert_eq!(resume["stdout_offset"], if output { 3 } else { 0 });
            assert_eq!(resume["stderr_offset"], 0);
            assert!(resume.get("command").is_none());
            test_server::write_response(
                &mut resumed,
                &sse(
                    &request,
                    &format!(
                        "{}{}",
                        event(
                            "stdout",
                            json!({"offset":if output { 3 } else { 0 },"data":"ZGVm"})
                        ),
                        event("exit", json!({"exit_code":7}))
                    ),
                ),
            )
            .await;
            output
        });
        let mut client = client(url.clone());
        client.limits.stall_timeout = Duration::from_millis(100);
        let workspace = tempfile::tempdir().unwrap();
        let result = client
            .execute(&sandbox(&url), &["tool".into()], "/tmp", workspace.path())
            .await;
        let output = test_server::assert_ready(server).await.unwrap();
        let capture = result.unwrap();
        assert_eq!(capture.exit_code, 7);
        assert_eq!(
            std::fs::read(capture.stdout.path()).unwrap(),
            if output {
                b"abcdef".as_slice()
            } else {
                b"def".as_slice()
            }
        );
    }
}

#[tokio::test]
async fn recoverable_errors_are_bounded_and_permanent_errors_do_not_resume() {
    for (status, error_type, attempts) in [
        (503, "", 4),
        (200, "ServerShuttingDown", 4),
        (401, "", 1),
        (404, "", 1),
        (200, "CommandNotFound", 1),
    ] {
        let (url, server) = test_server::spawn_with_handler(attempts, move |request| {
            if status == 200 {
                sse(request, &event("error", json!({"error_type":error_type})))
            } else {
                Response::empty(status)
            }
        })
        .await;
        let workspace = tempfile::tempdir().unwrap();
        assert!(client(url.clone())
            .execute(&sandbox(&url), &["tool".into()], "/tmp", workspace.path())
            .await
            .is_err());
        let requests = test_server::assert_ready(server).await.unwrap();
        let initial: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert!(requests[0].path.ends_with("/start"));
        for request in &requests[1..] {
            assert!(request.path.ends_with("/resume"));
            let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
            assert_eq!(body["command_id"], initial["command_id"]);
        }
    }
}
