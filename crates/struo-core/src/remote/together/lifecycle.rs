use std::time::Duration;

use reqwest::Method;
use serde::Deserialize;

use super::client::{api_error, Client, Sandbox};
use crate::remote::{
    http,
    lifecycle::{self, Cleanup, HostedSandbox, Journal, Lookup, Provider, MARKER_KEY},
};
use crate::{ExecutionPermit, Result};

impl HostedSandbox for Sandbox {
    fn id(&self) -> &str {
        &self.id
    }
}

pub(super) fn lifetime(timeout: Option<u64>) -> Duration {
    Duration::from_secs(
        timeout
            .map_or(600, |ms| ms.div_ceil(1000).saturating_add(120))
            .max(1),
    )
}

impl Client {
    pub async fn create_owned(
        &self,
        journal: Journal,
        snapshot: String,
        timeout: Option<u64>,
        lease: Option<ExecutionPermit>,
    ) -> Result<(Sandbox, Cleanup<Self>)> {
        lifecycle::create_owned(self.clone(), Some(journal), lifetime(timeout).saturating_add(Duration::from_mins(10)), lease, move |client, marker| async move {
            let tags = marker.map_or_else(|| serde_json::json!({}), |marker| serde_json::json!({MARKER_KEY: marker.nonce}));
            let request = client.body(client.control(Method::POST, "/sandboxes"), &serde_json::json!({"snapshot_id": snapshot, "ttl": lifetime(timeout).as_secs(), "tags": tags}))?;
            client.json(request).await
        }).await
    }

    pub async fn reconcile(&self, journal: &Journal) {
        lifecycle::reconcile(journal, self, lifecycle::Budget::default()).await;
    }
}

impl Provider for Client {
    const NAME: &'static str = "together";
    fn scope(&self) -> String {
        lifecycle::scope("together", &self.url)
    }
    fn credential(&self) -> String {
        lifecycle::credential("together", &self.key)
    }

    async fn find(&self, nonce: &str) -> Result<Lookup> {
        #[derive(Deserialize)]
        struct Entry {
            id: String,
            status: String,
            tags: std::collections::BTreeMap<String, String>,
        }
        #[derive(Deserialize)]
        struct Page {
            data: Vec<Entry>,
            next_cursor: Option<String>,
        }
        let page: Page = self
            .json(
                self.control(Method::GET, "/sandboxes")
                    .query(&[("limit", "100"), ("tags[struo-operation]", nonce)]),
            )
            .await?;
        Ok(lifecycle::classify(
            page.data.iter().map(|entry| {
                (
                    entry.id.as_str(),
                    entry.tags.get(MARKER_KEY).map(String::as_str),
                    entry.status != "terminated",
                )
            }),
            nonce,
            page.next_cursor.is_some(),
        ))
    }

    async fn delete(&self, id: &str) -> Result<()> {
        let path = format!("/sandboxes/{}/terminate", super::client::component(id));
        let request = self.body(
            self.control(Method::POST, &path),
            &serde_json::json!({"snapshot": null}),
        )?;
        let response = request.send().await.map_err(super::client::http_error)?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(());
        }
        let mut sandbox: Sandbox = http::decode(
            response,
            "together",
            self.limits.control_bytes(),
            self.limits,
        )
        .await?;
        if sandbox.status == "terminating" {
            sandbox = self
                .json(self.control(
                    Method::POST,
                    &format!("/sandboxes/{}/wait", super::client::component(id)),
                ))
                .await?;
        }
        if sandbox.status != "terminated" {
            return Err(api_error(format!(
                "termination not confirmed (status {})",
                sandbox.status
            )));
        }
        Ok(())
    }
}
