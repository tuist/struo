use std::sync::{Arc, Mutex};

use futures::SinkExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::tungstenite::Message;

use super::*;
use crate::remote::lifecycle::Provider;
use crate::{Executor, ResourceRequest, WorkspacePath};

mod tls;

#[derive(Default)]
struct State {
    name: String,
    requests: Vec<(String, String)>,
    uploaded: Vec<u8>,
    part: Vec<u8>,
    commands: Vec<Vec<String>>,
    hold_action: Option<(Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>)>,
    hold_delete: Option<(Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>)>,
}

#[allow(clippy::too_many_lines)]
async fn mock(
    exit: Option<u8>,
    fail_upload: bool,
) -> (Client, Arc<Mutex<State>>, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let state = Arc::new(Mutex::new(State::default()));
    let server_state = state.clone();
    let task = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut peek = vec![0; 65536];
            let count = loop {
                let count = socket.peek(&mut peek).await.unwrap();
                if peek[..count].windows(4).any(|s| s == b"\r\n\r\n") {
                    break count;
                }
                tokio::task::yield_now().await;
            };
            let headers = String::from_utf8_lossy(&peek[..count]);
            let first = headers.lines().next().unwrap();
            let mut words = first.split_whitespace();
            let method = words.next().unwrap().to_string();
            let path = words.next().unwrap().to_string();
            assert!(headers
                .to_lowercase()
                .contains("authorization: bearer test-token"));
            server_state
                .lock()
                .unwrap()
                .requests
                .push((method.clone(), path.clone()));
            if headers.to_lowercase().contains("upgrade: websocket") {
                let url = reqwest::Url::parse(&format!("http://localhost{path}")).unwrap();
                let args: Vec<String> = url
                    .query_pairs()
                    .filter(|(k, _)| k == "cmd")
                    .map(|(_, v)| v.into_owned())
                    .collect();
                let is_action = args[0] == "node";
                if args[0] == "dd" {
                    let mut state = server_state.lock().unwrap();
                    if !args.contains(&"oflag=append".into())
                        || !args.contains(&"conv=notrunc".into())
                    {
                        state.uploaded.clear();
                    }
                    let part = state.part.clone();
                    state.uploaded.extend(part);
                }
                server_state.lock().unwrap().commands.push(args);
                let mut ws = tokio_tungstenite::accept_async(socket).await.unwrap();
                if is_action {
                    let hold = server_state.lock().unwrap().hold_action.clone();
                    if let Some((arrived, release)) = hold {
                        arrived.notify_one();
                        release.notified().await;
                        continue;
                    }
                    assert!(url
                        .query_pairs()
                        .any(|(k, v)| k == "dir" && v == GUEST_ROOT));
                    ws.send(Message::Binary(vec![1, b'o', 0, 255].into()))
                        .await
                        .unwrap();
                    ws.send(Message::Binary(vec![2, b'e', b'r', b'r'].into()))
                        .await
                        .unwrap();
                }
                let code = if is_action { exit } else { Some(0) };
                if let Some(code) = code {
                    ws.send(Message::Binary(vec![3, code].into()))
                        .await
                        .unwrap();
                }
                let _ = ws.close(None).await;
                continue;
            }
            let mut header = Vec::new();
            while !header.ends_with(b"\r\n\r\n") {
                header.push(socket.read_u8().await.unwrap());
            }
            let headers = String::from_utf8(header).unwrap();
            let length = headers
                .lines()
                .find_map(|line| {
                    line.to_lowercase()
                        .strip_prefix("content-length: ")
                        .map(|v| v.parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            let mut body = vec![0; length];
            socket.read_exact(&mut body).await.unwrap();
            let mut status = 200;
            let response = match method.as_str() {
                "POST" => {
                    assert_eq!(path, "/v1/sprites");
                    let create: serde_json::Value = serde_json::from_slice(&body).unwrap();
                    let name = create["name"].as_str().unwrap().to_string();
                    assert!(name.starts_with("struo-"));
                    assert_eq!(create["url_settings"]["auth"], "sprite");
                    server_state.lock().unwrap().name = name.clone();
                    serde_json::to_vec(&serde_json::json!({"name": name, "id": "original-id"}))
                        .unwrap()
                }
                "PUT" => {
                    assert!(path.contains(
                        "/fs/write?path=%2Ftmp%2Fstruo-input-part&workingDir=%2F&mode=0600"
                    ));
                    assert!(body.len() <= 16 * 1024 * 1024);
                    server_state.lock().unwrap().part = body;
                    if fail_upload {
                        status = 400;
                    }
                    b"{}".to_vec()
                }
                "GET" => {
                    if !path.contains("/fs/read?") {
                        let name = server_state.lock().unwrap().name.clone();
                        let response = serde_json::to_vec(
                            &serde_json::json!({"name": name, "id": "original-id"}),
                        )
                        .unwrap();
                        socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", response.len()).as_bytes()).await.unwrap();
                        socket.write_all(&response).await.unwrap();
                        continue;
                    }
                    assert!(path.contains("/fs/read?"));
                    #[cfg(target_os = "linux")]
                    {
                        use std::os::unix::fs::symlink;
                        let workspace = tempfile::tempdir().unwrap();
                        let reports = workspace.path().join("reports");
                        std::fs::create_dir(&reports).unwrap();
                        std::fs::write(reports.join("result.bin"), b"passed\0\xff").unwrap();
                        std::fs::hard_link(reports.join("result.bin"), reports.join("hard-link"))
                            .unwrap();
                        symlink("result.bin", reports.join("symlink")).unwrap();
                        let archive_path = workspace.path().join("outputs.tar");
                        let commands = server_state.lock().unwrap().commands.clone();
                        let invocation = commands.last().unwrap();
                        assert_eq!(invocation[0], "tar");
                        let args = invocation[1..].iter().map(|arg| match arg.as_str() {
                            GUEST_ROOT => workspace.path().to_str().unwrap(),
                            OUTPUT_ARCHIVE => archive_path.to_str().unwrap(),
                            _ => arg.as_str(),
                        });
                        let output = std::process::Command::new("tar")
                            .args(args)
                            .output()
                            .unwrap();
                        assert!(output.status.success(), "{output:?}");
                        let response = std::fs::read(archive_path).unwrap();
                        socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", response.len()).as_bytes()).await.unwrap();
                        socket.write_all(&response).await.unwrap();
                        continue;
                    }
                    #[cfg(not(target_os = "linux"))]
                    {
                        let mut archive = tar::Builder::new(Vec::new());
                        let bytes = b"passed\0\xff";
                        let mut header = tar::Header::new_gnu();
                        header.set_size(bytes.len() as u64);
                        header.set_mode(0o644);
                        header.set_cksum();
                        archive
                            .append_data(&mut header, "reports/result.bin", &bytes[..])
                            .unwrap();
                        archive.into_inner().unwrap()
                    }
                }
                "DELETE" => {
                    assert_eq!(
                        path,
                        format!("/v1/sprites/{}", server_state.lock().unwrap().name)
                    );
                    let hold = server_state.lock().unwrap().hold_delete.clone();
                    if let Some((arrived, release)) = hold {
                        arrived.notify_one();
                        release.notified().await;
                    }
                    status = 204;
                    Vec::new()
                }
                _ => panic!("unexpected request"),
            };
            socket
                .write_all(
                    format!(
                        "HTTP/1.1 {status} OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        response.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            socket.write_all(&response).await.unwrap();
            if method == "DELETE" {
                break;
            }
        }
    });
    (Client::new(&url, "test-token".into()).unwrap(), state, task)
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn transfers_declared_dependencies_and_outputs_and_preserves_streams_and_exit() {
    for exit in [0, 7, 9] {
        let (client, state, server) = mock(Some(exit), false).await;
        let workspace = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(workspace.path().join("node_modules/lib")).unwrap();
        std::fs::write(
            workspace.path().join("node_modules/lib/index.js"),
            b"module.exports=42",
        )
        .unwrap();
        if exit == 0 {
            use std::io::Write;
            let mut file =
                std::fs::File::create(workspace.path().join("node_modules/large.bin")).unwrap();
            let bytes = vec![0xa5; 64 * 1024];
            for _ in 0..256 {
                file.write_all(&bytes).unwrap();
            }
            file.write_all(b"asymmetric-tail\0\xff").unwrap();
        }
        let cache = CacheProvider::open_local(workspace.path().join("cache"));
        let argv = vec!["node".into(), "test script.js".into()];
        let inputs = vec![WorkspacePath::try_from("node_modules").unwrap()];
        let outputs = vec![WorkspacePath::try_from("reports").unwrap()];
        let env = BTreeMap::from([("TEST_VALUE".into(), "a b&c".into())]);
        let resources = ResourceRequest::default();
        let result = execute_with_client(
            PreparedCommand {
                argv: &argv,
                env: &env,
                inputs: &inputs,
                outputs: &outputs,
                cwd: None,
                resources: &resources,
                redirect: crate::local::Redirect::default(),
                timeout_ms: Some(10000),
                success_exit_codes: &[0, 7],
                lease: None,
            },
            workspace.path(),
            &cache,
            false,
            client,
        )
        .await
        .unwrap();
        server.await.unwrap();
        assert_eq!(result.exit_code, i32::from(exit));
        assert_eq!(
            cache.get_blob(&result.stdout.unwrap()).await.unwrap(),
            b"o\0\xff"
        );
        assert_eq!(
            cache.get_blob(&result.stderr.unwrap()).await.unwrap(),
            b"err"
        );
        if exit == 9 {
            assert!(!workspace.path().join("reports").exists());
        } else {
            assert_eq!(
                std::fs::read(workspace.path().join("reports/result.bin")).unwrap(),
                b"passed\0\xff"
            );
            #[cfg(target_os = "linux")]
            {
                use std::os::unix::fs::MetadataExt;
                let reports = workspace.path().join("reports");
                assert_eq!(
                    std::fs::read(reports.join("hard-link")).unwrap(),
                    b"passed\0\xff"
                );
                assert_eq!(
                    std::fs::read(reports.join("symlink")).unwrap(),
                    b"passed\0\xff"
                );
                assert_eq!(
                    std::fs::read_link(reports.join("symlink")).unwrap(),
                    Path::new("result.bin")
                );
                assert_ne!(
                    std::fs::metadata(reports.join("result.bin")).unwrap().ino(),
                    std::fs::metadata(reports.join("hard-link")).unwrap().ino()
                );
            }
        }
        let state = state.lock().unwrap();
        let mut archive = tar::Archive::new(&state.uploaded[..]);
        let mut found_library = false;
        let mut found_large = false;
        for entry in archive.entries().unwrap() {
            use std::io::Read;
            let mut entry = entry.unwrap();
            if entry.path().unwrap().ends_with("node_modules/lib/index.js") {
                let mut bytes = Vec::new();
                entry.read_to_end(&mut bytes).unwrap();
                assert_eq!(bytes, b"module.exports=42");
                found_library = true;
            } else if entry.path().unwrap().ends_with("node_modules/large.bin") {
                let mut bytes = Vec::new();
                entry.read_to_end(&mut bytes).unwrap();
                assert_eq!(
                    bytes.len(),
                    16 * 1024 * 1024 + b"asymmetric-tail\0\xff".len()
                );
                assert!(bytes[..16 * 1024 * 1024].iter().all(|byte| *byte == 0xa5));
                assert_eq!(&bytes[16 * 1024 * 1024..], b"asymmetric-tail\0\xff");
                found_large = true;
            }
        }
        assert!(found_library);
        assert_eq!(found_large, exit == 0);
        assert!(state.commands.contains(&argv));
        if exit == 0 {
            assert_eq!(
                state
                    .requests
                    .iter()
                    .filter(|(method, _)| method == "PUT")
                    .count(),
                2
            );
        }
        assert_eq!(state.requests.last().unwrap().0, "DELETE");
        assert!(super::super::lifecycle::tests::records(workspace.path()).is_empty());
    }
}

#[tokio::test]
async fn transfer_failure_and_missing_exit_still_delete_owned_sprite() {
    for (exit, upload_failure) in [(Some(0), true), (None, false)] {
        let (client, state, server) = mock(exit, upload_failure).await;
        let workspace = tempfile::tempdir().unwrap();
        let cache = CacheProvider::open_local(workspace.path().join("cache"));
        let argv = vec!["node".into()];
        let resources = ResourceRequest::default();
        let error = execute_with_client(
            PreparedCommand {
                argv: &argv,
                env: &BTreeMap::new(),
                inputs: &[],
                outputs: &[],
                cwd: None,
                resources: &resources,
                redirect: crate::local::Redirect::default(),
                timeout_ms: Some(10000),
                success_exit_codes: &[0],
                lease: None,
            },
            workspace.path(),
            &cache,
            false,
            client,
        )
        .await
        .unwrap_err();
        server.await.unwrap();
        if upload_failure {
            assert!(error.to_string().contains("400"));
        } else {
            assert!(error.to_string().contains("without an exit code"));
        }
        assert_eq!(state.lock().unwrap().requests.last().unwrap().0, "DELETE");
    }
}

#[tokio::test]
async fn recovery_looks_up_exact_journaled_nonce_and_deletes_not_hibernates() {
    use crate::remote::lifecycle::tests::{intent, records, rescope};
    use crate::remote::test_server::{self, Response};
    let workspace = tempfile::tempdir().unwrap();
    let journal = Journal::new(workspace.path());
    let nonce = intent(&journal);
    let identity = serde_json::to_string(&(format!("struo-{nonce}"), "original-id")).unwrap();
    crate::remote::lifecycle::tests::mark_created(workspace.path(), &identity);
    let (url, server) = test_server::spawn(vec![
        Response::json(&format!(r#"{{"name":"struo-{nonce}","id":"original-id"}}"#)),
        Response::json(&format!(r#"{{"name":"struo-{nonce}","id":"original-id"}}"#)),
        Response::empty(204),
    ])
    .await;
    let client = Client::new(&url, "test-token".into()).unwrap();
    rescope(
        workspace.path(),
        "sprites",
        &client.scope(),
        &client.credential(),
    );
    let report = lifecycle::reconcile(&journal, &client, Budget::default()).await;
    assert_eq!(report.deleted, 1);
    let requests = server.await.unwrap();
    assert_eq!(requests[0].path, format!("/v1/sprites/struo-{nonce}"));
    assert_eq!(requests[2].method, "DELETE");
    assert!(records(workspace.path()).is_empty());
}

#[test]
fn sprites_admission_and_unsupported_environment_fail_closed() {
    let mut binding = RemoteExecution::provider("sprites");
    assert_eq!(Executor::from_binding(&binding).unwrap(), Executor::Sprites);
    assert_eq!(
        Executor::Sprites.resource_domain(),
        crate::ResourceDomain::Remote
    );
    binding.environment = Some("custom-image".into());
    assert!(Executor::from_binding(&binding).is_err());
}

#[tokio::test]
async fn cancelled_execution_holds_admission_until_delete_finishes() {
    use crate::remote::test_server::{assert_pending, assert_ready, single_remote_quota};
    let (client, state, server) = mock(Some(0), false).await;
    let arrived = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let delete_arrived = Arc::new(tokio::sync::Notify::new());
    let delete_release = Arc::new(tokio::sync::Notify::new());
    state.lock().unwrap().hold_action = Some((arrived.clone(), release.clone()));
    state.lock().unwrap().hold_delete = Some((delete_arrived.clone(), delete_release.clone()));
    let resources = single_remote_quota();
    let lease = resources
        .acquire(Executor::Sprites, &ResourceRequest::default())
        .await;
    let workspace = tempfile::tempdir().unwrap();
    let cache = CacheProvider::open_local(workspace.path().join("cache"));
    let argv = vec!["node".into()];
    let env = BTreeMap::new();
    let request = ResourceRequest::default();
    let mut execution = Box::pin(execute_with_client(
        PreparedCommand {
            argv: &argv,
            env: &env,
            inputs: &[],
            outputs: &[],
            cwd: None,
            resources: &request,
            redirect: crate::local::Redirect::default(),
            timeout_ms: None,
            success_exit_codes: &[0],
            lease: Some(lease),
        },
        workspace.path(),
        &cache,
        false,
        client,
    ));
    assert_ready(async {
        tokio::select! {
            _ = &mut execution => panic!("action finished before cancellation"),
            () = arrived.notified() => {},
        }
    })
    .await;
    drop(execution);
    assert_pending(resources.acquire(Executor::Sprites, &request)).await;
    release.notify_one();
    assert_ready(delete_arrived.notified()).await;
    assert_pending(resources.acquire(Executor::Sprites, &request)).await;
    delete_release.notify_one();
    assert_ready(server).await.unwrap();
    assert_ready(resources.acquire(Executor::Sprites, &request)).await;
    assert!(super::super::lifecycle::tests::records(workspace.path()).is_empty());
}

#[tokio::test]
async fn recovery_does_not_delete_an_unmatched_sprite() {
    use crate::remote::lifecycle::tests::{intent, records, rescope};
    use crate::remote::test_server::{self, Response};
    let workspace = tempfile::tempdir().unwrap();
    let journal = Journal::new(workspace.path());
    intent(&journal);
    let (url, server) = test_server::spawn(vec![Response::json(
        r#"{"name":"foreign-sprite","id":"foreign-id"}"#,
    )])
    .await;
    let client = Client::new(&url, "test-token".into()).unwrap();
    rescope(
        workspace.path(),
        "sprites",
        &client.scope(),
        &client.credential(),
    );
    let report = lifecycle::reconcile(&journal, &client, Budget::default()).await;
    assert_eq!(report.deleted, 0);
    assert_eq!(report.retained, 1);
    assert_eq!(server.await.unwrap().len(), 1);
    assert_eq!(records(workspace.path()).len(), 1);
}

#[tokio::test]
async fn recovery_retains_replacements_and_ambiguous_intents() {
    use crate::remote::lifecycle::tests::{intent, mark_created, records, rescope};
    use crate::remote::test_server::{self, Response};
    for known in [false, true] {
        let workspace = tempfile::tempdir().unwrap();
        let journal = Journal::new(workspace.path());
        let nonce = intent(&journal);
        let name = format!("struo-{nonce}");
        let identity = serde_json::to_string(&(&name, "original-id")).unwrap();
        if known {
            mark_created(workspace.path(), &identity);
        }
        let returned_id = if known {
            "replacement-id"
        } else {
            "original-id"
        };
        let (url, server) = test_server::spawn(vec![Response::json(&format!(
            r#"{{"name":"{name}","id":"{returned_id}"}}"#
        ))])
        .await;
        let client = Client::new(&url, "test-token".into()).unwrap();
        rescope(
            workspace.path(),
            "sprites",
            &client.scope(),
            &client.credential(),
        );
        let report = lifecycle::reconcile(&journal, &client, Budget::default()).await;
        assert_eq!(report.retained, 1);
        assert_eq!(report.deleted, 0);
        assert_eq!(server.await.unwrap().len(), 1);
        assert_eq!(
            records(workspace.path())[0]["sandbox_ids"],
            if known {
                serde_json::json!([identity])
            } else {
                serde_json::json!([])
            }
        );
    }
}

#[tokio::test]
async fn changed_credentials_cannot_claim_a_recorded_sprite() {
    let client = Client::new("http://127.0.0.1:1", "new-token".into()).unwrap();
    assert_eq!(
        client
            .find_owned("nonce", &["identity".into()], "old-credential")
            .await
            .unwrap(),
        lifecycle::Lookup::Inconclusive
    );
}

#[tokio::test]
async fn direct_cleanup_refuses_a_same_name_replacement() {
    use crate::remote::test_server::{self, Response};
    let (url, server) = test_server::spawn(vec![Response::json(
        r#"{"name":"struo-test","id":"replacement-id"}"#,
    )])
    .await;
    let client = Client::new(&url, "test-token".into()).unwrap();
    let identity = serde_json::to_string(&("struo-test", "original-id")).unwrap();
    assert!(client
        .delete(&identity)
        .await
        .unwrap_err()
        .to_string()
        .contains("identity changed"));
    assert_eq!(server.await.unwrap().len(), 1);
}
