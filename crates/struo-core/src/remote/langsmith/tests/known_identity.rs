use std::time::Duration;

use super::*;
use crate::remote::lifecycle::{
    self,
    tests::{exited, intent, mark_created, records, rescope},
    Budget, Journal,
};

#[tokio::test]
async fn successful_delete_with_failed_confirmation_is_recovered_without_another_delete() {
    let mut confirmations = 0;
    let (url, server) = test_server::spawn_with_handler(4, move |request| {
        if request.method == "POST" {
            let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
            Response::json(
                &json!({"id":ID,"name":"test","status":"ready","labels":body["labels"]})
                    .to_string(),
            )
        } else if request.method == "DELETE" {
            Response::empty(204)
        } else {
            assert!(!request.path.contains("label="));
            confirmations += 1;
            Response::json(if confirmations == 1 {
                r#"{"items":[]}"#
            } else {
                r#"{"items":[],"next_cursor":null}"#
            })
        }
    })
    .await;
    let client = client(url);
    let workspace = tempfile::tempdir().unwrap();
    let journal = Journal::new(workspace.path());
    let (_, cleanup) = lifecycle::create_owned(
        client.clone(),
        Some(journal.clone()),
        Duration::ZERO,
        None,
        |client, marker| async move { client.create("snapshot", marker.as_ref()).await },
    )
    .await
    .unwrap();
    assert!(cleanup.run().await.is_err());
    assert_eq!(records(workspace.path())[0]["sandbox_ids"], json!([ID]));
    exited(&journal);
    let report = lifecycle::reconcile(&journal, &client, Budget::default()).await;
    assert_eq!(report.absent, 1);
    assert_eq!(report.retained, 0);
    assert!(records(workspace.path()).is_empty());
    let requests = test_server::assert_ready(server).await.unwrap();
    assert_eq!(requests.iter().filter(|r| r.method == "DELETE").count(), 1);
    assert_eq!(requests.iter().filter(|r| r.method == "POST").count(), 1);
}

#[tokio::test]
async fn known_identity_absence_requires_complete_listing_and_original_credential() {
    for (credential_matches, body, absent) in [
        (true, json!({"items":[],"next_cursor":null}), true),
        (false, json!({"items":[],"next_cursor":null}), false),
        (true, json!({"items":[]}), false),
        (
            true,
            json!({"items":[{"id":ID,"name":"reused","status":"ready","labels":{MARKER_KEY:"foreign"}}],"next_cursor":null}),
            false,
        ),
    ] {
        let (url, server) = test_server::spawn(vec![Response::json(&body.to_string())]).await;
        let client = client(url);
        let workspace = tempfile::tempdir().unwrap();
        let journal = Journal::new(workspace.path());
        intent(&journal);
        mark_created(workspace.path(), ID);
        let credential = if credential_matches {
            client.credential()
        } else {
            "different".into()
        };
        rescope(workspace.path(), "langsmith", &client.scope(), &credential);
        let report = lifecycle::reconcile(&journal, &client, Budget::default()).await;
        assert_eq!(report.absent, usize::from(absent));
        assert_eq!(report.retained, usize::from(!absent));
        assert_eq!(records(workspace.path()).is_empty(), absent);
        let requests = test_server::assert_ready(server).await.unwrap();
        assert!(requests.iter().all(|r| r.method == "GET"));
    }
}

#[tokio::test]
async fn known_identity_lookup_checks_all_pages_and_never_deletes_a_reused_uuid() {
    let (url, server) = test_server::spawn(vec![
        Response::json(r#"{"items":[],"next_cursor":"next"}"#),
        Response::json(
            &json!({"items":[{"id":ID,"name":"reused","status":"ready"}],"next_cursor":null})
                .to_string(),
        ),
    ])
    .await;
    let client = client(url);
    assert_eq!(
        client
            .find_owned("nonce", &[ID.into()], &client.credential())
            .await
            .unwrap(),
        Lookup::Inconclusive
    );
    let requests = test_server::assert_ready(server).await.unwrap();
    assert!(requests[1].path.contains("cursor=next"));
    assert!(requests.iter().all(|r| !r.path.contains("label=")));
}
