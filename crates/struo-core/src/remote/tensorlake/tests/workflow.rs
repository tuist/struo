use std::io::Read;

use super::*;
use crate::remote::lifecycle::tests::records;

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn stages_dependencies_preserves_binary_capture_and_restores_only_accepted_outputs() {
    for exit in [0, 7, 1] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let origin = url.clone();
        let server = tokio::spawn(async move {
            let mut requests = Vec::new();
            let mut name = String::new();
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let request = test_server::read_request(&mut stream).await;
                assert_eq!(request.headers["authorization"], "Bearer project-key");
                let response = match (request.method.as_str(), request.path.as_str()) {
                    ("POST", "/sandboxes") => {
                        let body: serde_json::Value =
                            serde_json::from_slice(&request.body).unwrap();
                        name = body["name"].as_str().unwrap().into();
                        Response {
                            status: 202,
                            ..Response::json(r#"{"sandbox_id":"sb-one","state":"pending"}"#)
                        }
                    }
                    ("GET", "/sandboxes/sb-one") => info(&name, "running", &origin),
                    ("PUT", path) if path.starts_with("/api/v1/files?") => Response::empty(204),
                    ("POST", "/api/v1/processes") => {
                        let body: serde_json::Value =
                            serde_json::from_slice(&request.body).unwrap();
                        exited(if body["command"] == "sh" { exit } else { 0 })
                    }
                    ("GET", path) if path.contains("struo-stdout") => Response {
                        body: b"stdout\0\xff".to_vec(),
                        ..Response::empty(200)
                    },
                    ("GET", path) if path.contains("struo-stderr") => Response {
                        body: b"stderr\xfe".to_vec(),
                        ..Response::empty(200)
                    },
                    ("GET", path) if path.contains("struo-outputs") => {
                        Response::tar_file("./reports/result", b"passed\0\xff")
                    }
                    ("DELETE", "/sandboxes/sb-one") => Response::empty(204),
                    _ => panic!("unexpected {} {}", request.method, request.path),
                };
                let deleted = request.method == "DELETE";
                requests.push(request);
                test_server::write_response(&mut stream, &response).await;
                if deleted {
                    let (mut stream, _) = listener.accept().await.unwrap();
                    requests.push(test_server::read_request(&mut stream).await);
                    test_server::write_response(&mut stream, &info(&name, "terminated", &origin))
                        .await;
                    return requests;
                }
            }
        });
        let workspace = workspace();
        let result = test_server::assert_ready(run(
            client(&url),
            workspace.path(),
            None,
            None,
            &[WorkspacePath::try_from("reports").unwrap()],
        ))
        .await
        .unwrap();
        let requests = test_server::assert_ready(server).await.unwrap();
        let cache = CacheProvider::open_local(workspace.path().join("cache"));
        assert_eq!(result.exit_code, exit);
        assert_eq!(
            cache.get_blob(&result.stdout.unwrap()).await.unwrap(),
            b"stdout\0\xff"
        );
        assert_eq!(
            cache.get_blob(&result.stderr.unwrap()).await.unwrap(),
            b"stderr\xfe"
        );
        assert_eq!(workspace.path().join("reports/result").exists(), exit != 1);
        if exit != 1 {
            assert_eq!(
                std::fs::read(workspace.path().join("reports/result")).unwrap(),
                b"passed\0\xff"
            );
        }
        let create: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(create["image"], "node-tests-v1");
        assert_eq!(create["wait"], false);
        assert_eq!(create["auto_resume"], false);
        assert_eq!(create["allow_unauthenticated_access"], false);
        assert_eq!(create["exposed_ports"], serde_json::json!([]));
        assert!(create.get("template_id").is_none());
        for request in requests
            .iter()
            .filter(|request| request.path.starts_with("/api/"))
        {
            assert_eq!(request.headers["x-tensorlake-sandbox-id"], "sb-one");
        }
        let upload = requests
            .iter()
            .find(|request| request.method == "PUT")
            .unwrap();
        let mut archive = tar::Archive::new(upload.body.as_slice());
        let mut found = false;
        for entry in archive.entries().unwrap() {
            let mut entry = entry.unwrap();
            let path = entry.path().unwrap().to_string_lossy().into_owned();
            assert!(!path.contains("undeclared"));
            if path.ends_with("runner.mjs") {
                let mut bytes = Vec::new();
                entry.read_to_end(&mut bytes).unwrap();
                assert_eq!(bytes, b"dependency\0\xff");
                found = true;
            }
        }
        assert!(found);
        let execute: serde_json::Value = requests
            .iter()
            .filter(|request| request.method == "POST" && request.path == "/api/v1/processes")
            .map(|request| serde_json::from_slice::<serde_json::Value>(&request.body).unwrap())
            .find(|body| body["command"] == "sh")
            .unwrap();
        assert_eq!(
            execute["working_dir"],
            "/home/tl-user/struo-workspace/tests"
        );
        assert_eq!(execute["env"], serde_json::json!({}));
        let script = execute["args"][1].as_str().unwrap();
        assert!(script.contains("'IFS=literal-ifs'"));
        assert!(script.contains("'PWD=selected-pwd'"));
        assert!(records(workspace.path()).is_empty());
    }
}

#[tokio::test]
async fn process_polling_requires_a_real_exit_and_matching_pid() {
    for status in [
        r#"{"pid":42,"status":"exited"}"#,
        r#"{"pid":43,"status":"exited","exit_code":0}"#,
        r#"{"pid":42,"status":"oom_killed","exit_code":0}"#,
    ] {
        let (url, server) = test_server::spawn(vec![
            Response::json(r#"{"pid":42,"status":"running","exit_code":0}"#),
            Response::json(status),
        ])
        .await;
        let client = client(&url);
        assert!(client
            .process(
                &sandbox(),
                &client.guest_url(&url).unwrap(),
                &["node".into()],
                None
            )
            .await
            .is_err());
        let requests = test_server::assert_ready(server).await.unwrap();
        assert_eq!(requests[1].path, "/api/v1/processes/42");
        let start: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert!(start.get("working_dir").is_none());
        assert_eq!(start["stdin_mode"], "closed");
        assert_eq!(start["stdout_mode"], "discard");
        assert_eq!(start["stderr_mode"], "discard");
    }
}

#[tokio::test]
async fn raw_transfers_enforce_capture_bounds_and_reject_partial_http_status() {
    for (status, limit) in [(200, 3), (206, 100)] {
        let (url, server) = test_server::spawn(vec![Response {
            body: b"four".to_vec(),
            ..Response::empty(status)
        }])
        .await;
        let mut client = client(&url);
        client.limits.max_archive_bytes = limit;
        let directory = tempfile::tempdir().unwrap();
        assert!(client
            .download(
                &sandbox(),
                &client.guest_url(&url).unwrap(),
                "/tmp/output",
                &directory.path().join("file"),
                None,
                1
            )
            .await
            .is_err());
        test_server::assert_ready(server).await.unwrap();
    }
}

#[test]
fn returned_guest_urls_cannot_exfiltrate_credentials() {
    let client = Client::new(
        "https://api.tensorlake.ai",
        "secret".into(),
        "ns".into(),
        None,
    )
    .unwrap();
    assert!(client
        .guest_url("https://sb-one.sandbox.tensorlake.ai/path")
        .is_ok());
    for url in [
        "http://sb-one.sandbox.tensorlake.ai",
        "https://sandbox.tensorlake.ai.evil.test",
        "https://evil.test",
        "https://user@sb-one.sandbox.tensorlake.ai",
        "https://sb-one.sandbox.tensorlake.ai?token=x",
        "https://sb-one.sandbox.tensorlake.ai:444",
    ] {
        assert!(client.guest_url(url).is_err(), "{url}");
    }
}
