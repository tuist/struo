use std::time::Duration;

use reqwest::Method;

use super::client::{api_error, http_error, name, valid_id, Client, Sandbox};
use crate::remote::lifecycle::{
    self, Cleanup, HostedSandbox, Journal, Lookup, Provider, MARKER_KEY,
};
use crate::{ExecutionPermit, ResourceRequest, Result};

impl HostedSandbox for Sandbox {
    fn id(&self) -> &str {
        &self.sandbox.name
    }
}

impl Provider for Client {
    const NAME: &'static str = "vercel";

    fn scope(&self) -> String {
        lifecycle::scope(
            "vercel",
            &format!(
                "{}\n{}\n{}",
                self.config.endpoint, self.config.team, self.config.project
            ),
        )
    }

    fn credential(&self) -> String {
        lifecycle::credential("vercel", &self.config.token)
    }

    async fn find(&self, nonce: &str) -> Result<Lookup> {
        let response = self
            .named_request(Method::GET, &name(nonce))?
            .query(&[("resume", "false")])
            .send()
            .await
            .map_err(http_error)?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(Lookup::Absent);
        }
        let sandbox: Sandbox = crate::remote::http::decode(
            response,
            "vercel",
            self.limits.control_bytes(),
            self.limits,
        )
        .await?;
        if sandbox.sandbox.name == name(nonce)
            && sandbox
                .sandbox
                .tags
                .as_ref()
                .and_then(|tags| tags.get(MARKER_KEY))
                .map(String::as_str)
                == Some(nonce)
        {
            Ok(Lookup::Found(vec![sandbox.sandbox.name]))
        } else {
            Ok(Lookup::Inconclusive)
        }
    }

    async fn delete(&self, id: &str) -> Result<()> {
        let nonce = id
            .strip_prefix("struo-")
            .filter(|nonce| !nonce.is_empty())
            .ok_or_else(|| api_error("sandbox ownership name is invalid"))?;
        match self.find(nonce).await? {
            Lookup::Absent => return Ok(()),
            Lookup::Found(ids) if ids == [id] => {}
            _ => {
                return Err(api_error(
                    "sandbox ownership could not be confirmed before deletion",
                ))
            }
        }
        let response = self
            .named_request(Method::DELETE, id)?
            .query(&[("deleteOrphanSnapshots", "true")])
            .send()
            .await
            .map_err(http_error)?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(());
        }
        crate::remote::http::checked(response, "vercel")?;
        for _ in 0..20 {
            let response = self
                .named_request(Method::GET, id)?
                .query(&[("resume", "false")])
                .send()
                .await
                .map_err(http_error)?;
            if response.status() == reqwest::StatusCode::NOT_FOUND {
                tracing::debug!(
                    provider = "vercel",
                    sandbox = id,
                    "confirmed sandbox deletion"
                );
                return Ok(());
            }
            crate::remote::http::checked(response, "vercel")?;
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        Err(api_error("sandbox deletion was not confirmed"))
    }
}

impl Client {
    fn named_request(&self, method: Method, id: &str) -> Result<reqwest::RequestBuilder> {
        valid_id(id)?;
        Ok(self
            .request(method, &format!("/v2/sandboxes/{id}"))
            .query(&[("projectId", &self.config.project)])
            .timeout(Duration::from_secs(10)))
    }

    pub(super) async fn create_owned(
        &self,
        journal: Journal,
        runtime: String,
        resources: ResourceRequest,
        lifetime: u64,
        lease: Option<ExecutionPermit>,
    ) -> Result<(Sandbox, Cleanup<Self>)> {
        lifecycle::create_owned(
            self.clone(),
            Some(journal),
            Duration::from_millis(lifetime).saturating_add(Duration::from_mins(10)),
            lease,
            move |client, marker| async move {
                let marker = marker.ok_or_else(|| {
                    api_error("sandbox creation requires a durable ownership marker")
                })?;
                client
                    .create(&runtime, &resources, lifetime, &marker.nonce)
                    .await
            },
        )
        .await
    }
}
