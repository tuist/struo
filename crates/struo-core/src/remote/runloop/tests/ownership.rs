use super::*;
use crate::remote::lifecycle::tests::{capture, exited, intent, mark_created, records, rescope};
use crate::remote::lifecycle::{Journal, Lookup, Provider};

#[tokio::test]
async fn lost_create_response_recovers_only_exact_metadata_owner() {
    let root = tempfile::tempdir().unwrap();
    let journal = Journal::new(root.path());
    let nonce = intent(&journal);
    let listed = serde_json::json!({"has_more": false, "devboxes": [
        {"id":"foreign", "status":"running", "metadata":{"struo-operation":"other"}},
        {"id":"dbx_test", "status":"running", "metadata":{"struo-operation":nonce}}
    ]})
    .to_string();
    let (url, server) = capture(vec![
        (200, listed),
        (200, String::from_utf8(sandbox("shutdown").body).unwrap()),
    ])
    .await;
    let client = client(&url);
    rescope(
        root.path(),
        "runloop",
        &client.scope(),
        &client.credential(),
    );
    let report = client.reconcile(&journal).await;
    assert_eq!(report.deleted, 1);
    assert!(records(root.path()).is_empty());
    let requests = test_server::assert_ready(server).await.unwrap();
    assert_eq!(requests[1].path, "/devboxes/dbx_test/shutdown?force=true");
    for request in requests {
        assert_eq!(request.headers["authorization"], "Bearer test-key");
    }
}

#[tokio::test]
async fn incomplete_and_malformed_listings_retain_uncertain_records() {
    for page in [
        r#"{"devboxes":[],"has_more":true}"#,
        r#"{"devboxes":[],"has_more":"unknown"}"#,
        r#"{"devboxes":[]}"#,
    ] {
        let root = tempfile::tempdir().unwrap();
        let journal = Journal::new(root.path());
        intent(&journal);
        let (url, server) = test_server::spawn(vec![Response::json(page)]).await;
        let client = client(&url);
        rescope(
            root.path(),
            "runloop",
            &client.scope(),
            &client.credential(),
        );
        mark_created(root.path(), "dbx_test");
        assert_eq!(client.reconcile(&journal).await.retained, 1);
        assert_eq!(test_server::assert_ready(server).await.unwrap().len(), 1);
        assert!(!records(root.path()).is_empty());
    }
}

#[tokio::test]
async fn uncertain_create_is_not_retried_and_retains_intent() {
    let root = tempfile::tempdir().unwrap();
    let (url, server) = test_server::spawn(vec![Response::empty(500)]).await;
    assert!(client(&url)
        .create_owned(
            Journal::new(root.path()),
            Some("bpt_test".into()),
            None,
            None
        )
        .await
        .is_err());
    assert_eq!(records(root.path())[0]["phase"], "intent");
    let requests = test_server::assert_ready(server).await.unwrap();
    assert_eq!(requests.len(), 1);
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(
        body["metadata"]["struo-operation"],
        records(root.path())[0]["nonce"]
    );
    assert_eq!(body["blueprint_id"], "bpt_test");
    assert_eq!(body["launch_parameters"]["keep_alive_time_seconds"], 720);
}

#[tokio::test]
async fn ownership_scan_advances_pages_and_requires_complete_absence() {
    let (url, server) = test_server::spawn(vec![
        Response::json(
            r#"{"devboxes":[{"id":"foreign","status":"running","metadata":{}}],"has_more":true}"#,
        ),
        Response::json(r#"{"devboxes":[],"has_more":false}"#),
    ])
    .await;
    assert_eq!(
        client(&url).find_marked("nonce").await.unwrap(),
        Lookup::Absent
    );
    let requests = test_server::assert_ready(server).await.unwrap();
    assert!(requests[1].path.ends_with("&starting_after=foreign"));
}

#[tokio::test]
async fn complete_absence_retires_settled_intents_only_under_original_credentials() {
    for status in ["running", "shutdown"] {
        for (original_credential, settled) in [(true, true), (false, true), (true, false)] {
            let root = tempfile::tempdir().unwrap();
            let journal = Journal::new(root.path());
            let nonce = if settled {
                intent(&journal)
            } else {
                let nonce = journal
                    .begin(
                        "pending",
                        "pending",
                        "pending",
                        std::time::Duration::from_secs(3600),
                    )
                    .unwrap()
                    .nonce()
                    .to_string();
                exited(&journal);
                nonce
            };
            let page = serde_json::json!({"has_more": false, "devboxes": [
                {"id":"foreign", "status":status, "metadata":{"struo-operation":"other"}},
                {"id":"unmarked", "status":status, "metadata":{}},
                {"id":"dbx_test", "status":"shutdown", "metadata":{"struo-operation":nonce}}
            ]});
            let (url, server) = test_server::spawn(vec![Response::json(&page.to_string())]).await;
            let client = client(&url);
            let credential = client.credential();
            rescope(
                root.path(),
                "runloop",
                &client.scope(),
                if original_credential {
                    &credential
                } else {
                    "different-credential"
                },
            );
            let report = client.reconcile(&journal).await;
            let retired = original_credential && settled;
            assert_eq!(report.absent, usize::from(retired));
            assert_eq!(report.retained, usize::from(!retired));
            assert_eq!(records(root.path()).is_empty(), retired);
            let requests = test_server::assert_ready(server).await.unwrap();
            assert_eq!(requests.len(), 1);
            assert_eq!(requests[0].method, "GET");
        }
    }
}

#[tokio::test]
async fn missing_exit_status_partial_and_oversized_downloads_fail_closed() {
    let (url, server) = test_server::spawn(vec![execution("completed", None)]).await;
    let sandbox = serde_json::from_slice(&sandbox("running").body).unwrap();
    assert!(client(&url)
        .execute(&sandbox, "true", None, &BTreeMap::new())
        .await
        .unwrap_err()
        .to_string()
        .contains("exit status"));
    test_server::assert_ready(server).await.unwrap();
    for response in [Response::empty(206), binary(b"12345"), Response::json("{}")] {
        let (url, server) = test_server::spawn(vec![response]).await;
        let mut client = client(&url);
        client.limits.max_archive_bytes = 4;
        let file = tempfile::NamedTempFile::new().unwrap();
        assert!(client
            .download(&sandbox, INPUT_ARCHIVE, file.path(), None, 1)
            .await
            .is_err());
        test_server::assert_ready(server).await.unwrap();
    }
}

#[tokio::test]
async fn binary_captures_share_one_download_budget() {
    let (url, server) = test_server::spawn(vec![
        execution("completed", Some(0)),
        binary(b"123"),
        binary(b"456"),
    ])
    .await;
    let root = tempfile::tempdir().unwrap();
    let mut client = client(&url);
    client.limits.max_capture_bytes = 5;
    let sandbox = serde_json::from_slice(&sandbox("running").body).unwrap();
    let cache = CacheProvider::open_local(root.path().join("cache"));
    let argv = ["true".into()];
    let env = BTreeMap::new();
    let resources = ResourceRequest::default();
    let command = PreparedCommand {
        lease: None,
        argv: &argv,
        env: &env,
        cwd: None,
        inputs: &[],
        outputs: &[],
        resources: &resources,
        redirect: crate::local::Redirect::default(),
        timeout_ms: None,
        success_exit_codes: &[0],
    };
    assert!(matches!(
        capture::execute(&client, &sandbox, &command, root.path(), &cache, false).await,
        Err(Error::ExecutorResourceLimit { .. })
    ));
    test_server::assert_ready(server).await.unwrap();
}

#[test]
fn lifetime_respects_provider_limit() {
    assert_eq!(client::lifetime(None).unwrap().as_secs(), 720);
    assert_eq!(
        client::lifetime(Some(172_680_000)).unwrap().as_secs(),
        172_800
    );
    assert!(client::lifetime(Some(172_680_001)).is_err());
}

#[tokio::test]
async fn failed_shutdown_retains_journal_for_recovery() {
    let root = tempfile::tempdir().unwrap();
    let (url, server) = test_server::spawn(vec![
        sandbox("running"),
        Response::empty(500),
        Response::empty(500),
        Response::empty(500),
    ])
    .await;
    let (_, cleanup) = client(&url)
        .create_owned(Journal::new(root.path()), None, None, None)
        .await
        .unwrap();
    assert!(cleanup.run().await.is_err());
    assert_eq!(records(root.path())[0]["sandbox_ids"][0], "dbx_test");
    assert_eq!(test_server::assert_ready(server).await.unwrap().len(), 4);
}
