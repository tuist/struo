use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

use struo_cas::{ActionResult, CacheProvider};

use super::*;
use crate::remote::lifecycle::tests::records;
use crate::{ExecutionPermit, Executor, ResourceRequest};

async fn run(
    client: Client,
    workspace: &Path,
    lease: Option<ExecutionPermit>,
) -> Result<ActionResult> {
    let argv = [
        "node".into(),
        "node_modules/vitest/vitest.mjs".into(),
        "run".into(),
    ];
    run_argv(client, workspace, lease, &argv).await
}

async fn run_argv(
    client: Client,
    workspace: &Path,
    lease: Option<ExecutionPermit>,
    argv: &[String],
) -> Result<ActionResult> {
    let cache = CacheProvider::open_local(workspace.join("cache"));
    let env = BTreeMap::from([
        ("IFS".into(), "literal \t ' $HOME".into()),
        ("PWD".into(), "selected-pwd".into()),
        ("not-a-shell-identifier".into(), "literal=value".into()),
    ]);
    let inputs = [WorkspacePath::try_from("node_modules").unwrap()];
    let outputs = [WorkspacePath::try_from("reports").unwrap()];
    let resources = ResourceRequest::default();
    let mut remote = RemoteExecution::provider("langsmith");
    remote.environment = Some("node-tests:v1".into());
    Box::pin(execute_with_client(
        &remote,
        PreparedCommand {
            lease,
            argv,
            env: &env,
            cwd: None,
            inputs: &inputs,
            outputs: &outputs,
            resources: &resources,
            redirect: crate::local::Redirect::default(),
            timeout_ms: Some(10_000),
            success_exit_codes: &[0, 7],
        },
        workspace,
        &cache,
        false,
        client,
    ))
    .await
}

fn workspace() -> tempfile::TempDir {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::create_dir(workspace.path().join("node_modules")).unwrap();
    std::fs::write(
        workspace.path().join("node_modules/input"),
        b"dependency\0\xff",
    )
    .unwrap();
    std::fs::write(workspace.path().join(".gitignore"), "node_modules\n").unwrap();
    std::fs::write(workspace.path().join("undeclared"), "secret").unwrap();
    workspace
}

#[tokio::test]
async fn stages_declared_inputs_preserves_captures_and_restores_only_accepted_outputs() {
    for exit in [0, 7, 9] {
        let (url, server) = guest::spawn(exit).await;
        let workspace = workspace();
        let result = run(client(url), workspace.path(), None).await.unwrap();
        let requests = test_server::assert_ready(server).await.unwrap();
        let cache = CacheProvider::open_local(workspace.path().join("cache"));
        assert_eq!(result.exit_code, exit);
        assert_eq!(
            cache.get_blob(&result.stdout.unwrap()).await.unwrap(),
            b"out\0\xff"
        );
        assert_eq!(
            cache.get_blob(&result.stderr.unwrap()).await.unwrap(),
            b"err\xfe"
        );
        if exit == 9 {
            assert!(!workspace.path().join("reports").exists());
        } else {
            assert_eq!(
                std::fs::read(workspace.path().join("reports/result")).unwrap(),
                b"report\0\xfd"
            );
        }
        assert_eq!(requests.last().unwrap().method, "GET");
        assert!(records(workspace.path()).is_empty());
    }
}

#[tokio::test]
async fn executable_equals_and_exact_arguments_are_not_environment_assignments() {
    for executable in ["tool=probe", "./tool=probe", "/tmp/tool=probe"] {
        let argv: Vec<String> = [executable, "", "space ' $HOME", "a=b", "line\nbreak"]
            .into_iter()
            .map(String::from)
            .collect();
        let (url, server) = guest::spawn_argv(9, argv.clone()).await;
        let workspace = workspace();
        assert_eq!(
            run_argv(client(url), workspace.path(), None, &argv)
                .await
                .unwrap()
                .exit_code,
            9
        );
        test_server::assert_ready(server).await.unwrap();
    }
}

// The guest is a protocol double, not a live LangSmith sandbox.
mod guest {
    use super::*;

    pub async fn spawn(exit: i32) -> (String, tokio::task::JoinHandle<Vec<Request>>) {
        spawn_argv(
            exit,
            vec![
                "node".into(),
                "node_modules/vitest/vitest.mjs".into(),
                "run".into(),
            ],
        )
        .await
    }

    pub async fn spawn_argv(
        exit: i32,
        argv: Vec<String>,
    ) -> (String, tokio::task::JoinHandle<Vec<Request>>) {
        let (url_tx, url_rx) = std::sync::mpsc::channel();
        let mut step = 0;
        let mut command_count = 0;
        let mut marker = String::new();
        let (url, server) = crate::remote::test_server::spawn_with_handler(if exit == 9 { 6 } else { 8 }, move |request| {
            let url = url_rx.try_recv().ok();
            match step {
                0 => {
                    let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
                    assert_eq!(body["snapshot"], "node-tests:v1");
                    assert_eq!(body["persistent"], false);
                    assert_eq!(body["restore_memory"], false);
                    assert!(body.get("access_delegation").is_none());
                    assert!(body.get("mount_config").is_none());
                    marker = body["labels"][MARKER_KEY].as_str().unwrap().to_string();
                    assert!(!marker.is_empty());
                    step += 1;
                    let mut sandbox = serde_json::to_value(json!({"id":ID,"name":"struo-test","status":"ready","dataplane_url": format!("{}/api/v2/sandboxes/{ID}", url.unwrap())})).unwrap();
                    sandbox["labels"] = json!({MARKER_KEY:marker});
                    Response::json(&sandbox.to_string())
                }
                1 => {
                    assert!(request.path.contains("/upload?path=%2Ftmp%2Fstruo-inputs.tar"));
                    let header_end = request.body.windows(4).position(|v| v == b"\r\n\r\n").unwrap() + 4;
                    let mut archive = tar::Archive::new(&request.body[header_end..]);
                    let mut found = false;
                    for entry in archive.entries().unwrap() {
                        let mut entry = entry.unwrap();
                        let name = entry.path().unwrap().to_path_buf();
                        assert!(!name.ends_with("undeclared"));
                        if name.ends_with("node_modules/input") {
                            let mut bytes = Vec::new(); entry.read_to_end(&mut bytes).unwrap();
                            assert_eq!(bytes, b"dependency\0\xff"); found = true;
                        }
                    }
                    assert!(found);
                    let boundary = request.headers["content-type"].split("boundary=").nth(1).unwrap();
                    let len = request.body.len() - header_end - format!("\r\n--{boundary}--\r\n").len();
                    step += 1;
                    Response::json(&json!({"path":INPUT_ARCHIVE,"written":len}).to_string())
                }
                _ if request.path.ends_with("/start") => {
                    command_count += 1;
                    let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
                    let tail = if command_count == 2 {
                        assert_eq!(body["run_config"]["work_dir"], GUEST_ROOT);
                        assert_eq!(body["command"], json!(argv));
                        assert_eq!(body["run_config"]["env_vars"], json!({"IFS":"literal \t ' $HOME", "PWD":"selected-pwd", "not-a-shell-identifier":"literal=value"}));
                        format!("{}{}{}", event("stdout", json!({"offset":0,"data":STANDARD.encode(b"out\0\xff")})), event("stderr", json!({"offset":0,"data":STANDARD.encode(b"err\xfe")})), event("exit", json!({"exit_code":exit})))
                    } else { event("exit", json!({"exit_code":0})) };
                    sse(request, &tail)
                }
                _ if request.path.contains("/download?") => Response::tar_file("./reports/result", b"report\0\xfd"),
                _ if request.method == "DELETE" => Response::empty(204),
                _ => { assert!(!marker.is_empty()); Response::json(r#"{"items":[],"next_cursor":null}"#) },
            }
        }).await;
        url_tx.send(url.clone()).unwrap();
        (url, server)
    }
}

#[tokio::test]
async fn cancellation_retains_admission_until_deletion_is_confirmed() {
    let pool = crate::remote::test_server::single_remote_quota();
    let lease = pool
        .acquire(Executor::LangSmith, &ResourceRequest::default())
        .await;
    let (deleted, hold) = Response::json(r#"{"items":[],"next_cursor":null}"#).held();
    let mut deleted = Some(deleted);
    let (create_tx, create_rx) = tokio::sync::oneshot::channel();
    let mut create_tx = Some(create_tx);
    let (url, server) = crate::remote::test_server::spawn_with_handler(3, move |request| {
        if request.method == "POST" {
            let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
            let (response, hold) = Response::json(
                &json!({"id":ID,"name":"test","status":"provisioning","labels":body["labels"]})
                    .to_string(),
            )
            .held();
            assert!(create_tx.take().unwrap().send(hold).is_ok());
            response
        } else if request.method == "DELETE" {
            Response::empty(204)
        } else {
            deleted.take().unwrap()
        }
    })
    .await;
    let workspace = workspace();
    let mut future = Box::pin(run(client(url), workspace.path(), Some(lease)));
    let created = tokio::select! { result = &mut future => panic!("unexpected completion: {result:?}"), pending = create_rx => pending.unwrap() };
    test_server::assert_ready(created.arrived).await.unwrap();
    drop(future);
    created.release.send(()).unwrap();
    crate::remote::test_server::assert_ready(hold.arrived)
        .await
        .unwrap();
    crate::remote::test_server::assert_pending(
        pool.acquire(Executor::LangSmith, &ResourceRequest::default()),
    )
    .await;
    assert_eq!(records(workspace.path()).len(), 1);
    hold.release.send(()).unwrap();
    crate::remote::test_server::assert_ready(server)
        .await
        .unwrap();
    crate::remote::test_server::assert_ready(
        pool.acquire(Executor::LangSmith, &ResourceRequest::default()),
    )
    .await;
    assert!(records(workspace.path()).is_empty());
}
