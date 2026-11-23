use std::collections::BTreeMap;
use std::path::Path;

use struo_cas::{ActionResult, CacheProvider};

use super::client::{Client, Sandbox};
use super::execution::execute_with_client;
use crate::remote::test_server::{self, Response};
use crate::remote::PreparedCommand;
use crate::{ExecutionPermit, RemoteExecution, ResourceRequest, Result, WorkspacePath};

mod admission;
mod failures;
#[cfg(unix)]
mod guest;
mod lifecycle;
mod workflow;

fn client(url: &str) -> Client {
    Client::new(
        url,
        "project-key".into(),
        "test-namespace".into(),
        Some(url),
    )
    .unwrap()
}

fn binding() -> RemoteExecution {
    let mut remote = RemoteExecution::provider("tensorlake");
    remote.environment = Some("node-tests-v1".into());
    remote
        .properties
        .insert("namespace".into(), "test-namespace".into());
    remote
}

fn info(name: &str, status: &str, url: &str) -> Response {
    Response::json(&serde_json::json!({
        "id":"sb-one", "name":name, "namespace":"test-namespace", "status":status,
        "sandbox_url":url, "auto_resume":false, "allow_unauthenticated_access":false, "exposed_ports":[]
    }).to_string())
}

fn exited(code: i32) -> Response {
    Response::json(&format!(
        r#"{{"pid":42,"status":"exited","exit_code":{code}}}"#
    ))
}

fn sandbox() -> Sandbox {
    Sandbox {
        provider_id: "sb-one".into(),
        identity: r#"{"id":"sb-one","name":"struo-nonce","namespace":"test-namespace"}"#.into(),
    }
}

async fn run(
    client: Client,
    workspace: &Path,
    lease: Option<ExecutionPermit>,
    timeout_ms: Option<u64>,
    outputs: &[WorkspacePath],
) -> Result<ActionResult> {
    let argv = vec![
        "printf".into(),
        "%s|%s|%s".into(),
        "literal ' ; $HOME".into(),
        "two".into(),
        "three".into(),
    ];
    let env = BTreeMap::from([
        ("IFS".into(), "literal-ifs".into()),
        ("PWD".into(), "selected-pwd".into()),
    ]);
    let inputs = [WorkspacePath::try_from("node_modules").unwrap()];
    let resources = ResourceRequest::default();
    let cwd = WorkspacePath::try_from("tests").unwrap();
    Box::pin(execute_with_client(
        &binding(),
        PreparedCommand {
            lease,
            argv: &argv,
            env: &env,
            cwd: Some(&cwd),
            inputs: &inputs,
            outputs,
            resources: &resources,
            redirect: crate::local::Redirect::default(),
            timeout_ms,
            success_exit_codes: &[0, 7],
        },
        workspace,
        &CacheProvider::open_local(workspace.join("cache")),
        false,
        client,
    ))
    .await
}

fn workspace() -> tempfile::TempDir {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::create_dir(workspace.path().join("node_modules")).unwrap();
    std::fs::write(
        workspace.path().join("node_modules/runner.mjs"),
        b"dependency\0\xff",
    )
    .unwrap();
    std::fs::write(workspace.path().join("undeclared"), "secret").unwrap();
    workspace
}
