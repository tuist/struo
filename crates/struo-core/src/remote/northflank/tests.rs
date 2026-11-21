use std::collections::BTreeMap;

use struo_cas::CacheProvider;

use super::client::Client;
use super::execution::{execute_with_client, expiry_seconds};
use crate::remote::lifecycle::{Lookup, Marker, Provider};
use crate::remote::test_server::{self, Response};
use crate::remote::PreparedCommand;
use crate::RemoteExecution;

fn client(url: &str) -> Client {
    Client::new(
        url,
        "test-token".into(),
        "test-project".into(),
        None,
        "test-plan".into(),
    )
    .unwrap()
}

#[tokio::test]
async fn recovery_requires_the_exact_marker_and_scope() {
    let body = r#"{"data":{"id":"struo-operation","description":"foreign","status":{"deployment":{"status":"COMPLETED"}}}}"#;
    let (url, server) = test_server::spawn(vec![Response::json(body), Response::empty(404)]).await;
    let client = client(&url);
    assert_eq!(
        client.find("operation").await.unwrap(),
        Lookup::Inconclusive
    );
    assert_eq!(client.find("missing").await.unwrap(), Lookup::Absent);
    let mut other = client.clone();
    other.project = "other-project".into();
    assert_ne!(client.scope(), other.scope());
    other = client.clone();
    other.team = Some("team".into());
    assert_ne!(client.scope(), other.scope());
    let requests = test_server::assert_ready(server).await.unwrap();
    assert_eq!(
        requests[0].path,
        "/v1/projects/test-project/services/struo-operation"
    );
}

#[tokio::test]
async fn deletion_waits_for_absence_and_does_not_delete_project_or_volumes() {
    let (url, server) = test_server::spawn(vec![Response::empty(200), Response::empty(404)]).await;
    client(&url).delete("struo-owned").await.unwrap();
    let requests = test_server::assert_ready(server).await.unwrap();
    assert_eq!(
        requests
            .iter()
            .map(|request| request.method.as_str())
            .collect::<Vec<_>>(),
        ["DELETE", "GET"]
    );
    assert!(requests
        .iter()
        .all(|request| request.path == "/v1/projects/test-project/services/struo-owned"));
}

#[tokio::test]
async fn create_carries_expiry_and_recoverable_ownership() {
    let (url, server) = test_server::spawn(vec![Response::json(r#"{"data":{"id":"struo-operation","description":"struo-operation: operation","status":{"deployment":{"status":"PENDING"}}}}"#)]).await;
    let client = client(&url);
    client
        .create(
            "node:22",
            900,
            &Marker {
                nonce: "operation".into(),
            },
        )
        .await
        .unwrap();
    let requests = test_server::assert_ready(server).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(body["name"], "struo-operation");
    assert_eq!(body["description"], "struo-operation: operation");
    assert_eq!(body["createOptions"]["expiryTime"], 900);
    assert_eq!(body["deployment"]["instances"], 1);
    assert!(body.get("ports").is_none());
    assert!(body["createOptions"].get("volumesToAttach").is_none());
    assert_eq!(body["billing"]["deploymentPlan"], "test-plan");
}

#[test]
fn expiry_obeys_provider_boundaries() {
    assert_eq!(expiry_seconds(1).unwrap(), 301);
    assert_eq!(expiry_seconds(604_500_000).unwrap(), 604_800);
    assert!(expiry_seconds(604_500_001).is_err());
    assert!(expiry_seconds(u64::MAX).is_err());
}

#[tokio::test]
async fn plan_capacity_must_cover_both_cpu_and_memory() {
    let body = r#"{"data":{"plans":[{"id":"test-plan","cpuResource":2,"ramResource":4096}]}}"#;
    let (url, server) = test_server::spawn(vec![
        Response::json(body),
        Response::json(body),
        Response::json(body),
    ])
    .await;
    let client = client(&url);
    let mut resources = crate::ResourceRequest {
        cpu_slots: 2,
        memory_bytes: 4_294_967_296,
        ..Default::default()
    };
    client.validate_plan(&resources).await.unwrap();
    resources.memory_bytes += 1;
    assert!(client.validate_plan(&resources).await.is_err());
    resources.memory_bytes = 4_294_967_296;
    resources.cpu_slots = 3;
    assert!(client.validate_plan(&resources).await.is_err());
    test_server::assert_ready(server).await.unwrap();
}

mod lifecycle;
mod websocket;
mod workflow;
