use super::*;
use crate::remote::lifecycle::{self, tests::records, Budget, Journal, Lookup, Provider};

#[tokio::test]
async fn lost_create_without_recorded_id_remains_manual_under_any_credential() {
    let (url, server) = test_server::spawn(vec![Response::empty(500)]).await;
    let client = client(&url);
    let workspace = tempfile::tempdir().unwrap();
    let journal = Journal::new(workspace.path());
    let result = lifecycle::create_owned(
        client.clone(),
        Some(journal.clone()),
        std::time::Duration::ZERO,
        None,
        |client, marker| async move { client.create(&binding(), &marker.unwrap().nonce).await },
    )
    .await;
    assert!(result.is_err());
    test_server::assert_ready(server).await.unwrap();
    let report = lifecycle::reconcile(&journal, &client, Budget::default()).await;
    assert_eq!(report.retained, 1);
    assert_eq!(records(workspace.path()).len(), 1);
    assert_eq!(
        client
            .find_owned("nonce", &[], &client.credential())
            .await
            .unwrap(),
        Lookup::Inconclusive
    );
    assert_eq!(
        client
            .find_owned("nonce", &[sandbox().identity], "other-key")
            .await
            .unwrap(),
        Lookup::Inconclusive
    );
}

#[tokio::test]
async fn deletion_checks_namespace_name_and_id_and_confirms_termination() {
    for foreign in ["name", "namespace", "id"] {
        let mut response: serde_json::Value = serde_json::from_slice(
            &info(
                "struo-nonce",
                "running",
                "https://sb-one.sandbox.tensorlake.ai",
            )
            .body,
        )
        .unwrap();
        response[foreign] = "foreign".into();
        let (url, server) = test_server::spawn(vec![Response::json(&response.to_string())]).await;
        assert!(client(&url).delete(&sandbox().identity).await.is_err());
        let requests = test_server::assert_ready(server).await.unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "GET");
    }
    let (url, server) = test_server::spawn(vec![
        info("struo-nonce", "running", ""),
        Response::empty(204),
        info("struo-nonce", "running", ""),
        info("struo-nonce", "terminated", ""),
    ])
    .await;
    client(&url).delete(&sandbox().identity).await.unwrap();
    let requests = test_server::assert_ready(server).await.unwrap();
    assert_eq!(requests[1].method, "DELETE");
    assert_eq!(requests[1].path, "/sandboxes/sb-one");
}

#[tokio::test]
async fn readiness_rejects_failed_states_and_insecure_settings() {
    for (field, value) in [
        ("status", serde_json::json!("suspended")),
        ("allow_unauthenticated_access", true.into()),
        ("auto_resume", true.into()),
        ("exposed_ports", serde_json::json!([3000])),
        ("sandbox_url", serde_json::json!("https://evil.test")),
    ] {
        let mut body: serde_json::Value = serde_json::from_slice(
            &info(
                "struo-nonce",
                "running",
                "https://sb-one.sandbox.tensorlake.ai",
            )
            .body,
        )
        .unwrap();
        body[field] = value;
        let (url, server) = test_server::spawn(vec![Response::json(&body.to_string())]).await;
        assert!(client(&url).ready(&sandbox()).await.is_err());
        test_server::assert_ready(server).await.unwrap();
    }
}

#[tokio::test]
async fn cancelled_creation_retains_admission_until_owned_id_is_terminated() {
    let (created, hold) = Response::json(r#"{"sandbox_id":"sb-one","state":"pending"}"#).held();
    let (deleted, deletion_hold) = Response::empty(204).held();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let request = test_server::read_request(&mut stream).await;
        let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
        let name = body["name"].as_str().unwrap();
        let mut created = created;
        let gate = created.hold.take().unwrap();
        let _ = gate.arrived.send(());
        let _ = gate.release.await;
        test_server::write_response(&mut stream, &created).await;
        for mut response in [
            info(name, "running", ""),
            deleted,
            info(name, "terminated", ""),
        ] {
            let (mut stream, _) = listener.accept().await.unwrap();
            test_server::read_request(&mut stream).await;
            if let Some(gate) = response.hold.take() {
                let _ = gate.arrived.send(());
                let _ = gate.release.await;
            }
            test_server::write_response(&mut stream, &response).await;
        }
    });
    let resources = test_server::single_remote_quota();
    let lease = resources
        .acquire(crate::Executor::Tensorlake, &ResourceRequest::default())
        .await;
    let client = client(&url);
    let workspace = tempfile::tempdir().unwrap();
    let journal = Journal::new(workspace.path());
    let task = tokio::spawn(lifecycle::create_owned(
        client,
        Some(journal),
        std::time::Duration::ZERO,
        Some(lease),
        |client, marker| async move { client.create(&binding(), &marker.unwrap().nonce).await },
    ));
    test_server::assert_ready(hold.arrived).await.unwrap();
    task.abort();
    let _ = task.await;
    test_server::assert_pending(
        resources.acquire(crate::Executor::Tensorlake, &ResourceRequest::default()),
    )
    .await;
    hold.release.send(()).unwrap();
    test_server::assert_ready(deletion_hold.arrived)
        .await
        .unwrap();
    test_server::assert_pending(
        resources.acquire(crate::Executor::Tensorlake, &ResourceRequest::default()),
    )
    .await;
    deletion_hold.release.send(()).unwrap();
    test_server::assert_ready(server).await.unwrap();
    let _lease = test_server::assert_ready(
        resources.acquire(crate::Executor::Tensorlake, &ResourceRequest::default()),
    )
    .await;
    assert!(records(workspace.path()).is_empty());
}
