use serde::Deserialize;
use std::time::Duration;

use super::client::{api_error, http_error, validate_id, Client, Sandbox};
use crate::remote::lifecycle::{Lookup, MARKER_KEY};
use crate::{Error, Result};

#[derive(Deserialize)]
struct Listing {
    devboxes: Vec<Sandbox>,
    has_more: bool,
}

impl Client {
    pub async fn find_marked(&self, nonce: &str) -> Result<Lookup> {
        let mut cursor = None;
        for _ in 0..32 {
            let mut request = self
                .request(reqwest::Method::GET, "devboxes")
                .query(&[("limit", "100"), ("include_total_count", "false")]);
            if let Some(cursor) = cursor.as_deref() {
                request = request.query(&[("starting_after", cursor)]);
            }
            let page: Listing = self
                .decode(request.send().await.map_err(http_error)?)
                .await?;
            let found = page
                .devboxes
                .iter()
                .filter(|s| {
                    s.metadata.get(MARKER_KEY).is_some_and(|m| m == nonce) && s.status != "shutdown"
                })
                .map(|s| s.id.clone())
                .collect::<Vec<_>>();
            for id in &found {
                validate_id(id)?;
            }
            if !found.is_empty() {
                return Ok(Lookup::Found(found));
            }
            if !page.has_more {
                return Ok(Lookup::Absent);
            }
            let next = page.devboxes.last().map(|s| s.id.clone());
            if next.is_none() || next == cursor {
                return Ok(Lookup::Inconclusive);
            }
            cursor = next;
        }
        Ok(Lookup::Inconclusive)
    }

    pub async fn shutdown(&self, id: &str) -> Result<()> {
        let timeout = Duration::from_secs(30);
        tokio::time::timeout(timeout, async {
            validate_id(id)?;
            let response = self
                .request(reqwest::Method::POST, &format!("devboxes/{id}/shutdown"))
                .query(&[("force", "true")])
                .send()
                .await
                .map_err(http_error)?;
            if response.status() == reqwest::StatusCode::NOT_FOUND {
                return Ok(());
            }
            let mut sandbox: Sandbox = self.decode(response).await?;
            if sandbox.id != id {
                return Err(api_error("shutdown returned a different devbox ID".into()));
            }
            while sandbox.status != "shutdown" {
                tokio::time::sleep(Duration::from_millis(250)).await;
                sandbox = self.get(id).await?;
            }
            tracing::debug!(
                provider = "runloop",
                sandbox = id,
                "confirmed devbox shutdown"
            );
            Ok(())
        })
        .await
        .map_err(|_| Error::Timeout(timeout))?
    }
}
