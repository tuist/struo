use std::time::Duration;

use super::*;
use crate::remote::lifecycle::{
    self,
    tests::{exited, intent, records, rescope},
    Budget, Journal,
};

#[tokio::test]
async fn lost_creation_is_not_retried_or_forgotten_after_empty_discovery() {
    let (url, server) = test_server::spawn(vec![
        Response::empty(504),
        Response::json(r#"{"items":[],"next_cursor":null}"#),
    ])
    .await;
    let client = client(url);
    let workspace = tempfile::tempdir().unwrap();
    let journal = Journal::new(workspace.path());
    let result = lifecycle::create_owned(
        client.clone(),
        Some(journal.clone()),
        Duration::ZERO,
        None,
        |client, marker| async move { client.create("node-tests:v1", marker.as_ref()).await },
    )
    .await;
    assert!(result.is_err());
    assert_eq!(records(workspace.path()).len(), 1);
    exited(&journal);
    let report = lifecycle::reconcile(&journal, &client, Budget::default()).await;
    assert_eq!(report.retained, 1);
    assert_eq!(report.absent, 0);
    assert_eq!(records(workspace.path()).len(), 1);
    let requests = test_server::assert_ready(server).await.unwrap();
    assert_eq!(requests.iter().filter(|r| r.method == "POST").count(), 1);
}

#[tokio::test]
async fn recovery_deletes_only_exact_markers_and_confirms_absence() {
    for exact in [false, true] {
        let workspace = tempfile::tempdir().unwrap();
        let journal = Journal::new(workspace.path());
        let nonce = intent(&journal);
        let marker = if exact { nonce } else { "foreign".into() };
        let mut responses = vec![Response::json(&json!({"items":[{"id":ID,"name":"test","status":"ready","labels":{MARKER_KEY:marker}}],"next_cursor":null}).to_string())];
        if exact {
            responses.extend([
                Response::empty(204),
                Response::json(r#"{"items":[],"next_cursor":null}"#),
            ]);
        }
        let (url, server) = test_server::spawn(responses).await;
        let client = client(url);
        rescope(
            workspace.path(),
            "langsmith",
            &client.scope(),
            &client.credential(),
        );
        let report = lifecycle::reconcile(&journal, &client, Budget::default()).await;
        assert_eq!(report.deleted, usize::from(exact));
        assert_eq!(records(workspace.path()).len(), usize::from(!exact));
        let requests = test_server::assert_ready(server).await.unwrap();
        assert_eq!(
            requests.iter().filter(|r| r.method == "DELETE").count(),
            usize::from(exact)
        );
    }
}

#[tokio::test]
async fn malformed_or_unfinished_streams_never_report_success() {
    let (url, server) = test_server::spawn_with_handler(4, |request| sse(request, "")).await;
    let workspace = tempfile::tempdir().unwrap();
    assert!(client(url.clone())
        .execute(&sandbox(&url), &["vitest".into()], "/tmp", workspace.path())
        .await
        .is_err());
    let requests = test_server::assert_ready(server).await.unwrap();
    assert!(requests[0].path.ends_with("/start"));
    assert!(requests[1..].iter().all(|r| r.path.ends_with("/resume")));
}

#[tokio::test]
async fn foreign_and_cyclic_pages_never_prove_absence() {
    let (url, server) = test_server::spawn(vec![
        Response::json(r#"{"items":[],"next_cursor":"repeat"}"#),
        Response::json(r#"{"items":[],"next_cursor":"repeat"}"#),
    ])
    .await;
    assert_eq!(
        client(url).find("nonce").await.unwrap(),
        Lookup::Inconclusive
    );
    test_server::assert_ready(server).await.unwrap();
}

#[tokio::test]
async fn oversized_events_fail_before_allocating_unbounded_capture() {
    let (url, server) = test_server::spawn_with_handler(1, |request| {
        sse(
            request,
            &event("stdout", json!({"offset":0,"data":"a".repeat(4096)})),
        )
    })
    .await;
    let mut client = client(url.clone());
    client.limits.max_response_bytes = 1024;
    let workspace = tempfile::tempdir().unwrap();
    assert!(client
        .execute(&sandbox(&url), &["vitest".into()], "/tmp", workspace.path())
        .await
        .is_err());
    test_server::assert_ready(server).await.unwrap();
}
