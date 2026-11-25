use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::client::api_error;
use crate::remote::http;
use crate::{RemoteIoLimits, Result};

mod action;
pub(super) mod bootstrap;
mod inputs;
mod shared;
#[cfg(all(test, target_os = "linux"))]
mod tests;

pub(super) use bootstrap::install;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Binding {
    pub url: String,
    pub token: String,
    pub binary: String,
}

pub(super) struct Service {
    http: reqwest::Client,
    base: reqwest::Url,
    token: String,
    limits: RemoteIoLimits,
}

impl Service {
    pub(super) fn new(binding: &Binding, limits: RemoteIoLimits) -> Result<Self> {
        let base = reqwest::Url::parse(&binding.url)
            .map_err(|_| api_error("invalid worker service URL"))?;
        if (base.scheme() != "https"
            && !(base.scheme() == "http"
                && matches!(base.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))))
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
        {
            return Err(api_error("worker service requires HTTPS or loopback HTTP"));
        }
        let http = reqwest::Client::builder()
            .http1_only()
            .redirect(reqwest::redirect::Policy::none())
            .pool_max_idle_per_host(0)
            .connect_timeout(limits.stall_timeout)
            .build()
            .map_err(|error| http::http_error("sprites", error))?;
        Ok(Self {
            http,
            base,
            token: binding.token.clone(),
            limits,
        })
    }

    fn request(&self, method: reqwest::Method, path: &str) -> Result<reqwest::RequestBuilder> {
        let url = self
            .base
            .join(path)
            .map_err(|_| api_error("invalid worker service path"))?;
        Ok(self.http.request(method, url).bearer_auth(&self.token))
    }

    async fn send(&self, request: reqwest::RequestBuilder) -> Result<reqwest::Response> {
        request
            .send()
            .await
            .map_err(|error| http::http_error("sprites", error))
    }

    pub(super) async fn ready(&self, environment: &str) -> Result<()> {
        let response = self
            .send(
                self.request(reqwest::Method::GET, "/v1/health")?
                    .timeout(Duration::from_secs(30)),
            )
            .await?;
        let value: serde_json::Value = http::decode(
            response,
            "sprites",
            self.limits.control_bytes(),
            self.limits,
        )
        .await?;
        if value["version"] != crate::worker::PROTOCOL_VERSION
            || value["environment"] != environment
        {
            return Err(api_error("worker service environment or protocol mismatch"));
        }
        Ok(())
    }
}
