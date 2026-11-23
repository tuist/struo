use std::collections::BTreeSet;

use serde::Deserialize;

use super::client::{api_error, http_error, validate_id, Client, Sandbox};
use crate::remote::{
    http,
    lifecycle::{self, HostedSandbox, Lookup, Provider, MARKER_KEY},
};
use crate::Result;

#[derive(Deserialize)]
struct Page {
    items: Vec<Sandbox>,
    next_cursor: Cursor,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Cursor {
    More(String),
    End(()),
}

impl HostedSandbox for Sandbox {
    fn id(&self) -> &str {
        &self.id
    }
}

impl Client {
    async fn list(&self, nonce: Option<&str>) -> Result<Option<Vec<Sandbox>>> {
        let mut cursor = None;
        let mut seen = BTreeSet::new();
        let mut sandboxes = Vec::new();
        let mut bytes = 0_u64;
        for _ in 0..100 {
            let mut request = self
                .request(reqwest::Method::GET, "/api/v2/sandboxes/boxes")
                .query(&[("page_size", "100"), ("created_by", "me")]);
            if let Some(nonce) = nonce {
                request = request.query(&[("label", format!("{MARKER_KEY}={nonce}"))]);
            }
            if let Some(cursor) = &cursor {
                request = request.query(&[("cursor", cursor)]);
            }
            let response = http::checked(request.send().await.map_err(http_error)?, "langsmith")?;
            let body = http::body(
                response,
                "langsmith",
                self.limits.control_bytes(),
                self.limits,
            )
            .await?;
            bytes = bytes.saturating_add(body.len() as u64);
            http::check_size(
                bytes,
                self.limits.max_metadata_bytes,
                "langsmith",
                "listing bytes",
            )?;
            let page: Page =
                serde_json::from_slice(&body).map_err(|_| api_error("invalid sandbox listing"))?;
            for sandbox in &page.items {
                validate_id(&sandbox.id)?;
            }
            sandboxes.extend(page.items);
            match page.next_cursor {
                Cursor::End(()) => return Ok(Some(sandboxes)),
                Cursor::More(next) if !next.is_empty() && seen.insert(next.clone()) => {
                    cursor = Some(next);
                }
                Cursor::More(_) => return Ok(None),
            }
        }
        Ok(None)
    }
}

impl Provider for Client {
    const NAME: &'static str = "langsmith";
    fn scope(&self) -> String {
        lifecycle::scope(
            "langsmith",
            &format!(
                "{}\n{}",
                self.config.url.trim_end_matches('/'),
                self.config.workspace
            ),
        )
    }
    fn credential(&self) -> String {
        lifecycle::credential("langsmith", &self.config.api_key)
    }

    async fn find(&self, nonce: &str) -> Result<Lookup> {
        let Some(sandboxes) = self.list(Some(nonce)).await? else {
            return Ok(Lookup::Inconclusive);
        };
        Ok(
            match lifecycle::classify(
                sandboxes.iter().map(|s| {
                    (
                        s.id.as_str(),
                        s.labels.get(MARKER_KEY).map(String::as_str),
                        true,
                    )
                }),
                nonce,
                false,
            ) {
                Lookup::Absent => Lookup::Inconclusive,
                lookup => lookup,
            },
        )
    }

    async fn find_owned(&self, nonce: &str, ids: &[String], credential: &str) -> Result<Lookup> {
        if ids.is_empty() || credential != self.credential() {
            return self.find(nonce).await;
        }
        let Some(sandboxes) = self.list(None).await? else {
            return Ok(Lookup::Inconclusive);
        };
        let owned: Vec<_> = sandboxes
            .iter()
            .filter(|sandbox| sandbox.labels.get(MARKER_KEY).is_some_and(|v| v == nonce))
            .map(|sandbox| sandbox.id.clone())
            .collect();
        if !owned.is_empty() {
            return Ok(Lookup::Found(owned));
        }
        Ok(
            if ids.iter().all(|id| !sandboxes.iter().any(|s| &s.id == id)) {
                Lookup::Absent
            } else {
                Lookup::Inconclusive
            },
        )
    }

    async fn delete(&self, id: &str) -> Result<()> {
        validate_id(id)?;
        let response = self
            .request(
                reqwest::Method::DELETE,
                &format!("/api/v2/sandboxes/boxes/{id}"),
            )
            .send()
            .await
            .map_err(http_error)?;
        if response.status() != reqwest::StatusCode::NOT_FOUND {
            http::checked(response, "langsmith")?;
        }
        // GET is name-only, so confirm UUID absence through the complete creator-scoped list.
        let Some(sandboxes) = self.list(None).await? else {
            return Err(api_error("deletion confirmation listing was incomplete"));
        };
        if sandboxes.iter().any(|s| s.id == id) {
            return Err(api_error("sandbox is still present after deletion"));
        }
        Ok(())
    }
}
