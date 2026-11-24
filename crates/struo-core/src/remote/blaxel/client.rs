use std::collections::BTreeMap;
use std::time::Duration;

use serde::{de::DeserializeOwned, Deserialize};

use crate::{Error, RemoteExecution, RemoteIoLimits, Result};

#[derive(Clone)]
pub(super) struct Config {
    pub control_url: String,
    pub run_url: String,
    pub workspace: String,
    pub api_key: String,
    pub region: Option<String>,
}

impl Config {
    pub(super) fn from_env(remote: &RemoteExecution) -> Result<Self> {
        let required = |primary, fallback| {
            std::env::var(primary)
                .or_else(|_| std::env::var(fallback))
                .ok()
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| Error::RemoteProviderConfig {
                    provider: "blaxel".into(),
                    message: format!("set {primary} or {fallback}"),
                })
        };
        Ok(Self {
            control_url: std::env::var("STRUO_BLAXEL_CONTROL_URL")
                .unwrap_or_else(|_| "https://api.blaxel.ai/v0".into()),
            run_url: std::env::var("STRUO_BLAXEL_RUN_URL")
                .unwrap_or_else(|_| "https://run.blaxel.ai".into()),
            workspace: remote
                .properties
                .get("workspace")
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| Error::RemoteProviderConfig {
                    provider: remote.provider.clone(),
                    message: "Blaxel requires properties.workspace before execution".into(),
                })?
                .into(),
            api_key: required("STRUO_BLAXEL_API_KEY", "BL_API_KEY")?,
            region: std::env::var("BL_REGION")
                .ok()
                .filter(|s| !s.trim().is_empty()),
        })
    }

    #[cfg(test)]
    pub(super) fn for_test(url: String) -> Self {
        Self {
            control_url: url.clone(),
            run_url: url,
            workspace: "tests".into(),
            api_key: "test-key".into(),
            region: None,
        }
    }
}

#[derive(Clone)]
pub(super) struct Client {
    pub config: Config,
    pub http: reqwest::Client,
    pub limits: RemoteIoLimits,
}

#[derive(Clone, Deserialize)]
pub(super) struct Sandbox {
    pub metadata: Metadata,
    pub status: Option<String>,
}

#[derive(Clone, Deserialize)]
pub(super) struct Metadata {
    pub name: String,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
}

pub(super) fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 49
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

impl Client {
    pub(super) fn new(config: Config) -> Result<Self> {
        for endpoint in [&config.control_url, &config.run_url] {
            let url =
                reqwest::Url::parse(endpoint).map_err(|_| api_error("invalid Blaxel endpoint"))?;
            if (url.scheme() != "https"
                && !(url.scheme() == "http"
                    && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))))
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
            {
                return Err(api_error("Blaxel endpoints require HTTPS (or loopback HTTP), without credentials, query, or fragment"));
            }
        }
        if !valid_name(&config.workspace) {
            return Err(api_error("invalid Blaxel workspace name"));
        }
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .pool_max_idle_per_host(0)
            .connect_timeout(Duration::from_secs(30))
            .build()
            .map_err(http_error)?;
        Ok(Self {
            http,
            config,
            limits: RemoteIoLimits::default(),
        })
    }

    fn authenticated(&self, method: reqwest::Method, url: String) -> reqwest::RequestBuilder {
        self.http
            .request(method, url)
            .bearer_auth(&self.config.api_key)
            .header("X-Blaxel-Workspace", &self.config.workspace)
    }

    pub(super) fn control(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.authenticated(
            method,
            format!("{}/{}", self.config.control_url.trim_end_matches('/'), path),
        )
        .timeout(Duration::from_secs(60))
    }

    pub(super) fn guest(
        &self,
        sandbox: &Sandbox,
        method: reqwest::Method,
        path: &str,
    ) -> Result<reqwest::RequestBuilder> {
        Ok(self
            .guest_transfer(sandbox, method, path)?
            .timeout(Duration::from_secs(60)))
    }

    pub(super) fn guest_transfer(
        &self,
        sandbox: &Sandbox,
        method: reqwest::Method,
        path: &str,
    ) -> Result<reqwest::RequestBuilder> {
        if !valid_name(&sandbox.metadata.name) {
            return Err(api_error("invalid sandbox name"));
        }
        Ok(self.authenticated(
            method,
            format!(
                "{}/{}/sandboxes/{}/{}",
                self.config.run_url.trim_end_matches('/'),
                self.config.workspace,
                sandbox.metadata.name,
                path
            ),
        ))
    }

    pub(super) async fn decode<T: DeserializeOwned>(
        &self,
        response: reqwest::Response,
    ) -> Result<T> {
        super::super::http::decode(response, "blaxel", self.limits.control_bytes(), self.limits)
            .await
    }
}

pub(super) fn api_error(message: impl Into<String>) -> Error {
    Error::RemoteProviderApi {
        provider: "blaxel".into(),
        message: message.into(),
    }
}
pub(super) fn http_error(error: reqwest::Error) -> Error {
    super::super::http::http_error("blaxel", error)
}
