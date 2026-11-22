use serde::Deserialize;

use super::client::{api_error, http_error, validate_id, Client, Sandbox};
use crate::remote::lifecycle::{self, HostedSandbox, Lookup, Provider, MARKER_KEY};
use crate::Result;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageInfo {
    has_more: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListResponse {
    sandboxes: Vec<Sandbox>,
    page_info: PageInfo,
}

#[derive(Deserialize)]
struct DeletionResponse {
    operation: Deletion,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Deletion {
    id: String,
    kind: String,
    target_id: String,
    status: String,
}

impl HostedSandbox for Sandbox {
    fn id(&self) -> &str {
        &self.id
    }
}

impl Provider for Client {
    const NAME: &'static str = "boat";
    fn scope(&self) -> String {
        lifecycle::scope("boat", &self.config.url)
    }
    fn credential(&self) -> String {
        lifecycle::credential("boat", &self.config.api_key)
    }

    async fn find(&self, nonce: &str) -> Result<Lookup> {
        let response: ListResponse = self
            .decode(self.request(reqwest::Method::GET, "/sandboxes").query(&[
                ("label", format!("{MARKER_KEY}={nonce}")),
                ("limit", "200".into()),
            ]))
            .await?;
        for sandbox in &response.sandboxes {
            validate_id(&sandbox.id)?;
        }
        let lookup = lifecycle::classify(
            response.sandboxes.iter().map(|sandbox| {
                (
                    sandbox.id.as_str(),
                    sandbox.labels.get(MARKER_KEY).map(String::as_str),
                    true,
                )
            }),
            nonce,
            response.page_info.has_more,
        );
        // The same credential can see a different active wallet on a later run.
        Ok(match lookup {
            Lookup::Absent => Lookup::Inconclusive,
            lookup => lookup,
        })
    }

    async fn delete(&self, id: &str) -> Result<()> {
        validate_id(id)?;
        let response = self
            .request(reqwest::Method::DELETE, &format!("/sandboxes/{id}"))
            .header("X-Ascii-Confirm-Delete", id)
            .send()
            .await
            .map_err(http_error)?;
        let response: DeletionResponse =
            super::super::http::decode(response, "boat", self.limits.control_bytes(), self.limits)
                .await?;
        if response.operation.kind != "sandbox"
            || response.operation.target_id != id
            || !matches!(
                response.operation.status.as_str(),
                "pending" | "processing" | "blocked" | "completed"
            )
        {
            return Err(api_error("invalid sandbox deletion receipt"));
        }
        // Acceptance ends billing and irrevocably hands the purge to Boat.
        tracing::info!(provider = "boat", sandbox = id, operation = %response.operation.id, status = %response.operation.status, "permanent sandbox deletion accepted; data purge is asynchronous");
        Ok(())
    }
}
