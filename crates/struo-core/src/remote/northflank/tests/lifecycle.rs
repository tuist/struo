use std::path::Path;
use std::time::Duration;

use super::*;
use crate::remote::lifecycle::tests::{intent, records, rescope};
use crate::remote::lifecycle::{self, Budget, Journal};
use crate::{ExecutionPermit, Executor, ResourceRequest};

const PLANS: &str = r#"{"data":{"plans":[{"id":"test-plan","cpuResource":2,"ramResource":2048}]}}"#;
const PENDING: &str = r#"{"data":{"id":"struo-pending","description":"owned","status":{"deployment":{"status":"PENDING"}}}}"#;

async fn execute(
    url: &str,
    workspace: &Path,
    lease: Option<ExecutionPermit>,
    timeout_ms: Option<u64>,
) -> crate::Result<struo_cas::ActionResult> {
    let mut remote = RemoteExecution::provider("northflank");
    remote.environment = Some("node:22".into());
    let argv = vec!["true".into()];
    let env = BTreeMap::new();
    let resources = ResourceRequest::default();
    let cache = CacheProvider::open_local(workspace.join("cache"));
    let command = PreparedCommand {
        lease,
        argv: &argv,
        env: &env,
        cwd: None,
        inputs: &[],
        outputs: &[],
        resources: &resources,
        redirect: crate::local::Redirect::default(),
        timeout_ms,
        success_exit_codes: &[0],
    };
    execute_with_client(&remote, command, workspace, &cache, false, client(url)).await
}

#[tokio::test]
async fn startup_failure_deletes_and_releases_quota() {
    let (url, server) = test_server::spawn(vec![
        Response::json(PLANS),
        Response::json(
            r#"{"data":{"id":"struo-failed","status":{"deployment":{"status":"FAILED"}}}}"#,
        ),
        Response::empty(200),
        Response::empty(404),
    ])
    .await;
    let workspace = tempfile::tempdir().unwrap();
    let quota = test_server::single_remote_quota();
    let lease = quota
        .acquire(Executor::Northflank, &ResourceRequest::default())
        .await;
    let result = execute(&url, workspace.path(), Some(lease), None).await;
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("deployment failed"));
    let requests = test_server::assert_ready(server).await.unwrap();
    assert_eq!(requests[2].method, "DELETE");
    assert!(records(workspace.path()).is_empty());
    test_server::assert_ready(quota.acquire(Executor::Northflank, &ResourceRequest::default()))
        .await;
}

#[tokio::test]
async fn cancelling_startup_holds_quota_until_confirmed_deletion() {
    let (poll, poll_hold) = Response::json(PENDING).held();
    let (deleted, delete_hold) = Response::empty(404).held();
    let (url, server) = test_server::spawn(vec![
        Response::json(PLANS),
        Response::json(PENDING),
        poll,
        Response::empty(200),
        deleted,
    ])
    .await;
    let workspace = tempfile::tempdir().unwrap();
    let quota = test_server::single_remote_quota();
    let lease = quota
        .acquire(Executor::Northflank, &ResourceRequest::default())
        .await;
    let mut running = Box::pin(execute(&url, workspace.path(), Some(lease), None));
    tokio::select! {
        _ = &mut running => panic!("execution ended before startup"),
        _ = poll_hold.arrived => {}
    }
    drop(running);
    test_server::assert_pending(quota.acquire(Executor::Northflank, &ResourceRequest::default()))
        .await;
    drop(poll_hold.release);
    test_server::assert_ready(delete_hold.arrived)
        .await
        .unwrap();
    test_server::assert_pending(quota.acquire(Executor::Northflank, &ResourceRequest::default()))
        .await;
    drop(delete_hold.release);
    test_server::assert_ready(server).await.unwrap();
    test_server::assert_ready(quota.acquire(Executor::Northflank, &ResourceRequest::default()))
        .await;
    assert!(records(workspace.path()).is_empty());
}

#[tokio::test]
async fn timeout_still_waits_for_cleanup() {
    let (poll, poll_hold) = Response::json(PENDING).held();
    let (url, server) = test_server::spawn(vec![
        Response::json(PLANS),
        Response::json(PENDING),
        poll,
        Response::empty(200),
        Response::empty(404),
    ])
    .await;
    let workspace = tempfile::tempdir().unwrap();
    let mut running = Box::pin(execute(
        &url,
        workspace.path(),
        None,
        Some(crate::remote::phase::testing::BUDGET_MS),
    ));
    tokio::select! {
        _ = &mut running => panic!("execution ended before startup"),
        _ = poll_hold.arrived => {}
    }
    crate::remote::phase::testing::spend_budget().await;
    assert!(futures::poll!(&mut running).is_pending());
    drop(poll_hold.release);
    assert!(matches!(
        test_server::assert_ready(running).await,
        Err(crate::Error::Timeout(_))
    ));
    let requests = test_server::assert_ready(server).await.unwrap();
    assert_eq!(requests[3].method, "DELETE");
    assert!(records(workspace.path()).is_empty());
}

#[tokio::test]
async fn a_lost_create_response_is_recovered_by_name_and_marker() {
    let workspace = tempfile::tempdir().unwrap();
    let journal = Journal::new(workspace.path());
    let nonce = intent(&journal);
    let body = format!(
        r#"{{"data":{{"id":"struo-{nonce}","description":"struo-operation: {nonce}","status":{{"deployment":{{"status":"COMPLETED"}}}}}}}}"#
    );
    let (url, server) = test_server::spawn(vec![
        Response::json(&body),
        Response::empty(200),
        Response::empty(404),
    ])
    .await;
    let client = client(&url);
    rescope(
        workspace.path(),
        "northflank",
        &client.scope(),
        &client.credential(),
    );
    let report = lifecycle::reconcile(&journal, &client, Budget::default()).await;
    assert_eq!(report.deleted, 1);
    assert!(records(workspace.path()).is_empty());
    let requests = test_server::assert_ready(server).await.unwrap();
    assert_eq!(requests[1].method, "DELETE");
    assert!(requests
        .iter()
        .all(|request| request.path.ends_with(&format!("/struo-{nonce}"))));
}

#[tokio::test]
async fn failed_deletion_retains_the_journal() {
    let workspace = tempfile::tempdir().unwrap();
    let (url, server) =
        test_server::spawn(vec![Response::json(PENDING), Response::empty(403)]).await;
    let client = client(&url);
    let (_, cleanup) = lifecycle::create_owned(
        client,
        Some(Journal::new(workspace.path())),
        Duration::from_secs(300),
        None,
        |client, marker| async move { client.create("node:22", 300, &marker.unwrap()).await },
    )
    .await
    .unwrap();
    assert!(cleanup.run().await.is_err());
    assert_eq!(records(workspace.path()).len(), 1);
    test_server::assert_ready(server).await.unwrap();
}

#[tokio::test]
async fn exec_transport_failure_still_deletes_the_service() {
    let (url, server) = test_server::spawn(vec![
        Response::json(PLANS),
        Response::json(
            r#"{"data":{"id":"struo-ready","status":{"deployment":{"status":"COMPLETED"}}}}"#,
        ),
        Response::empty(403),
        Response::empty(200),
        Response::empty(404),
    ])
    .await;
    let workspace = tempfile::tempdir().unwrap();
    let quota = test_server::single_remote_quota();
    let lease = quota
        .acquire(Executor::Northflank, &ResourceRequest::default())
        .await;
    let error = execute(&url, workspace.path(), Some(lease), None)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("WebSocket connection failed"));
    let requests = test_server::assert_ready(server).await.unwrap();
    assert_eq!(requests[3].method, "DELETE");
    assert!(records(workspace.path()).is_empty());
    test_server::assert_ready(quota.acquire(Executor::Northflank, &ResourceRequest::default()))
        .await;
}
