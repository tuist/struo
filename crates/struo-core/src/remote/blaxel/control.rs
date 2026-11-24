use std::time::Duration;

use super::client::{api_error, http_error, valid_name, Client, Sandbox};
use crate::remote::lifecycle::{self, Lookup, Marker, MARKER_KEY};
use crate::{Error, ResourceRequest, Result};

pub(super) fn sandbox_name(nonce: &str) -> String {
    format!("struo-{nonce}")
}

impl Client {
    pub(super) async fn create_marked(
        &self,
        image: Option<&str>,
        resources: &ResourceRequest,
        timeout_ms: Option<u64>,
        marker: &Marker,
    ) -> Result<Sandbox> {
        let name = sandbox_name(&marker.nonce);
        let memory = resources.memory_bytes.div_ceil(1_000_000).max(
            u64::try_from(resources.cpu_slots.max(1))
                .unwrap_or(u64::MAX)
                .saturating_mul(2048),
        );
        let body = serde_json::json!({
            "metadata": { "name": name, "labels": { MARKER_KEY: marker.nonce } },
            "spec": { "region": self.config.region, "runtime": {
                "image": image.unwrap_or("blaxel/base-image:latest"), "memory": memory,
                "ttl": format!("{}s", lifetime(timeout_ms).as_secs()), "terminationGracePeriodSeconds": 0
            }}
        });
        let response = self
            .control(reqwest::Method::POST, "sandboxes")
            .header("Content-Type", "application/json")
            .body(super::super::http::encode(
                &body,
                "blaxel",
                self.limits.control_bytes(),
            )?)
            .send()
            .await
            .map_err(http_error)?;
        let sandbox: Sandbox = self.decode(response).await?;
        if sandbox.metadata.name != name
            || sandbox.metadata.labels.get(MARKER_KEY) != Some(&marker.nonce)
        {
            return Err(api_error(
                "create response did not confirm sandbox ownership",
            ));
        }
        Ok(sandbox)
    }

    async fn get(&self, name: &str) -> Result<Option<Sandbox>> {
        if !valid_name(name) {
            return Err(api_error("invalid sandbox name"));
        }
        let response = self
            .control(reqwest::Method::GET, &format!("sandboxes/{name}"))
            .send()
            .await
            .map_err(http_error)?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        self.decode(response).await.map(Some)
    }

    pub(super) async fn wait_started(&self, mut sandbox: Sandbox) -> Result<Sandbox> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
        loop {
            match sandbox.status.as_deref() {
                Some("DEPLOYED") => {
                    let response = self
                        .guest(&sandbox, reqwest::Method::GET, "health")?
                        .send()
                        .await
                        .map_err(http_error)?;
                    if response.status().is_success() {
                        return Ok(sandbox);
                    }
                    if !matches!(response.status().as_u16(), 502..=504) {
                        super::super::http::checked(response, "blaxel")?;
                    }
                }
                Some("FAILED" | "TERMINATED" | "DELETING" | "DEACTIVATED" | "ARCHIVED") => {
                    return Err(api_error("sandbox failed to become ready"))
                }
                _ => {}
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(Error::Timeout(Duration::from_secs(60)));
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
            sandbox = self
                .get(&sandbox.metadata.name)
                .await?
                .ok_or_else(|| api_error("sandbox disappeared during startup"))?;
        }
    }

    pub(super) async fn find_marked(&self, nonce: &str) -> Result<Lookup> {
        let name = sandbox_name(nonce);
        let Some(sandbox) = self.get(&name).await? else {
            return Ok(Lookup::Absent);
        };
        if sandbox.metadata.name != name
            || sandbox.metadata.labels.get(MARKER_KEY).map(String::as_str) != Some(nonce)
        {
            return Ok(Lookup::Inconclusive);
        }
        if sandbox.status.as_deref() == Some("TERMINATED") {
            return Ok(Lookup::Absent);
        }
        Ok(Lookup::Found(vec![name]))
    }

    pub(super) async fn delete(&self, name: &str) -> Result<()> {
        if !valid_name(name) {
            return Err(api_error("invalid sandbox name"));
        }
        let response = self
            .control(reqwest::Method::DELETE, &format!("sandboxes/{name}"))
            .send()
            .await
            .map_err(http_error)?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(());
        }
        super::super::http::checked(response, "blaxel")?;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        loop {
            match self.get(name).await? {
                None => return Ok(()),
                Some(sandbox) if sandbox.status.as_deref() == Some("TERMINATED") => return Ok(()),
                _ => {}
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(Error::Timeout(Duration::from_secs(30)));
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    pub(super) fn scope(&self) -> String {
        lifecycle::scope(
            "blaxel",
            &format!(
                "{}/{}",
                self.config.control_url.trim_end_matches('/'),
                self.config.workspace
            ),
        )
    }
    pub(super) fn credential(&self) -> String {
        lifecycle::credential("blaxel", &self.config.api_key)
    }
}

pub(super) fn lifetime(timeout_ms: Option<u64>) -> Duration {
    Duration::from_secs(
        timeout_ms
            .unwrap_or(600_000)
            .div_ceil(1000)
            .saturating_add(120)
            .max(300),
    )
}
