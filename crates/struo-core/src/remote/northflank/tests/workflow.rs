use std::io::Read;

use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::*;
use crate::remote::lifecycle::tests::records;
use crate::{Executor, ResourceRequest, WorkspacePath};

async fn http(stream: &mut tokio::net::TcpStream) -> (String, Value) {
    let mut bytes = Vec::new();
    while !bytes.ends_with(b"\r\n\r\n") {
        bytes.push(stream.read_u8().await.unwrap());
    }
    let headers = String::from_utf8(bytes).unwrap();
    assert!(headers
        .to_ascii_lowercase()
        .contains("authorization: bearer test-token"));
    let size = headers
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .map(|value| value.trim().parse::<usize>().unwrap())
        })
        .unwrap_or(0);
    let mut body = vec![0; size];
    stream.read_exact(&mut body).await.unwrap();
    (
        headers.lines().next().unwrap().to_string(),
        serde_json::from_slice(&body).unwrap_or(Value::Null),
    )
}

async fn reply(stream: &mut tokio::net::TcpStream, code: u16, value: Value) {
    let body = value.to_string();
    let head = format!("HTTP/1.1 {code} Response\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
    stream.write_all(head.as_bytes()).await.unwrap();
    stream.write_all(body.as_bytes()).await.unwrap();
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn declared_dependencies_outputs_capture_and_cleanup_use_the_real_adapter() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        assert!(http(&mut stream).await.0.starts_with("GET /v1/plans "));
        reply(
            &mut stream,
            200,
            json!({"data":{"plans":[{"id":"test-plan","cpuResource":2,"ramResource":2048}]}}),
        )
        .await;
        let (mut stream, _) = listener.accept().await.unwrap();
        let (request, create) = http(&mut stream).await;
        assert!(request.starts_with("POST /v1/projects/test-project/services/deployment "));
        let id = create["name"].as_str().unwrap().to_string();
        reply(&mut stream, 200, json!({"data":{"id":id,"description":create["description"],"status":{"deployment":{"status":"COMPLETED"}}}})).await;
        for index in 0..4 {
            let (stream, _) = listener.accept().await.unwrap();
            let (mut socket, init) = super::websocket::accept(stream).await;
            let argv = &init["data"]["context"]["command"];
            let input = super::websocket::input(&mut socket).await;
            match index {
                0 => assert_eq!(argv, &json!(["mkdir", "-p", "/workspace"])),
                1 => {
                    let mut tar = tar::Archive::new(&input[..]);
                    let mut files = BTreeMap::new();
                    for entry in tar.entries().unwrap() {
                        let mut entry = entry.unwrap();
                        let path = entry.path().unwrap().to_string_lossy().into_owned();
                        let mut bytes = Vec::new();
                        entry.read_to_end(&mut bytes).unwrap();
                        files.insert(path, bytes);
                    }
                    assert_eq!(files["node_modules/dependency/index.js"], b"binary\0\xff");
                    assert_eq!(files["tests/run.sh"], b"printf test");
                }
                2 => {
                    assert_eq!(argv[0], "env");
                    assert!(argv
                        .as_array()
                        .unwrap()
                        .contains(&json!("TEST_VALUE=spaces ' and $ literal")));
                    assert_eq!(
                        argv.as_array().unwrap().last().unwrap(),
                        "cd '/workspace/tests' && exec 'sh' 'run.sh' 'argument with spaces'"
                    );
                }
                3 => assert_eq!(
                    argv,
                    &json!(["tar", "-cf", "-", "-C", "/workspace", "./reports"])
                ),
                _ => unreachable!(),
            }
            let output = if index == 3 {
                Response::tar_file("./reports/result.bin", b"passed\0\xff").body
            } else if index == 2 {
                b"stdout\xff".to_vec()
            } else {
                Vec::new()
            };
            super::websocket::finish(
                &mut socket,
                if index == 2 { 7 } else { 0 },
                &output,
                if index == 2 { b"stderr" } else { b"" },
            )
            .await;
        }
        let (mut stream, _) = listener.accept().await.unwrap();
        assert!(http(&mut stream)
            .await
            .0
            .starts_with(&format!("DELETE /v1/projects/test-project/services/{id} ")));
        reply(&mut stream, 200, Value::Null).await;
        let (mut stream, _) = listener.accept().await.unwrap();
        assert!(http(&mut stream)
            .await
            .0
            .starts_with(&format!("GET /v1/projects/test-project/services/{id} ")));
        reply(&mut stream, 404, Value::Null).await;
    });
    let workspace = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(workspace.path().join("node_modules/dependency")).unwrap();
    std::fs::create_dir_all(workspace.path().join("tests")).unwrap();
    std::fs::write(
        workspace.path().join("node_modules/dependency/index.js"),
        b"binary\0\xff",
    )
    .unwrap();
    std::fs::write(workspace.path().join("tests/run.sh"), b"printf test").unwrap();
    let cache = CacheProvider::open_local(workspace.path().join("cache"));
    let mut remote = RemoteExecution::provider("northflank");
    remote.environment = Some("node:22".into());
    let argv = vec!["sh".into(), "run.sh".into(), "argument with spaces".into()];
    let env = BTreeMap::from([("TEST_VALUE".into(), "spaces ' and $ literal".into())]);
    let inputs = [
        WorkspacePath::try_from("node_modules").unwrap(),
        WorkspacePath::try_from("tests").unwrap(),
    ];
    let outputs = [WorkspacePath::try_from("reports").unwrap()];
    let cwd = WorkspacePath::try_from("tests").unwrap();
    let resources = ResourceRequest::default();
    let quota = test_server::single_remote_quota();
    let lease = quota.acquire(Executor::Northflank, &resources).await;
    let command = PreparedCommand {
        lease: Some(lease),
        argv: &argv,
        env: &env,
        cwd: Some(&cwd),
        inputs: &inputs,
        outputs: &outputs,
        resources: &resources,
        redirect: crate::local::Redirect::default(),
        timeout_ms: Some(10_000),
        success_exit_codes: &[0, 7],
    };
    let result = test_server::assert_ready(execute_with_client(
        &remote,
        command,
        workspace.path(),
        &cache,
        false,
        client(&url),
    ))
    .await
    .unwrap();
    assert_eq!(result.exit_code, 7);
    assert_eq!(
        result.stdout,
        Some(struo_cas::Digest::from_bytes(
            *blake3::hash(b"stdout\xff").as_bytes()
        ))
    );
    assert_eq!(
        result.stderr,
        Some(struo_cas::Digest::from_bytes(
            *blake3::hash(b"stderr").as_bytes()
        ))
    );
    assert_eq!(
        std::fs::read(workspace.path().join("reports/result.bin")).unwrap(),
        b"passed\0\xff"
    );
    assert!(records(workspace.path()).is_empty());
    test_server::assert_ready(quota.acquire(Executor::Northflank, &resources)).await;
    test_server::assert_ready(server).await.unwrap();
}
