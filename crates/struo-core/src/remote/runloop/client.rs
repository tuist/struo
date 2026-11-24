use serde::{de::DeserializeOwned, Deserialize};
use std::{collections::BTreeMap, time::Duration};

use crate::remote::lifecycle::{Marker, MARKER_KEY};
use crate::{Error, RemoteIoLimits, Result};

#[derive(Clone)]
pub(super) struct Config {
    pub url: String,
    pub key: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            url: std::env::var("STRUO_RUNLOOP_URL")
                .unwrap_or_else(|_| "https://api.runloop.ai/v1".into()),
            key: std::env::var("STRUO_RUNLOOP_API_KEY")
                .or_else(|_| std::env::var("RUNLOOP_API_KEY"))
                .map_err(|_| Error::RemoteProviderConfig {
                    provider: "runloop".into(),
                    message: "set STRUO_RUNLOOP_API_KEY or RUNLOOP_API_KEY".into(),
                })?,
        })
    }
}

#[derive(Clone)]
pub(super) struct Client {
    pub http: reqwest::Client,
    pub config: Config,
    pub limits: RemoteIoLimits,
}

#[derive(Clone, Deserialize)]
pub(super) struct Sandbox {
    pub id: String,
    pub status: String,
    pub metadata: BTreeMap<String, String>,
}

impl Client {
    pub fn new(config: Config) -> Result<Self> {
        let url =
            reqwest::Url::parse(&config.url).map_err(|_| api_error("invalid API URL".into()))?;
        if !(url.scheme() == "https"
            || (url.scheme() == "http"
                && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))))
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(api_error("API URL must use HTTPS (or loopback HTTP), without credentials, query, or fragment".into()));
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

    pub fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(
                method,
                format!("{}/{}", self.config.url.trim_end_matches('/'), path),
            )
            .bearer_auth(&self.config.key)
            .timeout(Duration::from_secs(30))
    }

    pub async fn decode<T: DeserializeOwned>(&self, response: reqwest::Response) -> Result<T> {
        super::super::http::decode(
            response,
            "runloop",
            self.limits.control_bytes(),
            self.limits,
        )
        .await
    }

    pub async fn create_marked(
        &self,
        blueprint: Option<&str>,
        lifetime: Duration,
        marker: Option<&Marker>,
    ) -> Result<Sandbox> {
        let metadata = marker
            .map(|m| BTreeMap::from([(MARKER_KEY, m.nonce.as_str())]))
            .unwrap_or_default();
        let body = super::super::http::encode(
            &serde_json::json!({
                "blueprint_id": blueprint,
                "metadata": metadata,
                "launch_parameters": { "keep_alive_time_seconds": lifetime.as_secs() }
            }),
            "runloop",
            self.limits.control_bytes(),
        )?;
        let response = self
            .request(reqwest::Method::POST, "devboxes")
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .await
            .map_err(http_error)?;
        let sandbox: Sandbox = self.decode(response).await?;
        validate_id(&sandbox.id)?;
        tracing::debug!(provider = "runloop", sandbox = %sandbox.id, "created devbox");
        Ok(sandbox)
    }

    pub async fn get(&self, id: &str) -> Result<Sandbox> {
        validate_id(id)?;
        let response = self
            .request(reqwest::Method::GET, &format!("devboxes/{id}"))
            .send()
            .await
            .map_err(http_error)?;
        let sandbox: Sandbox = self.decode(response).await?;
        if sandbox.id != id {
            return Err(api_error("devbox lookup returned a different ID".into()));
        }
        Ok(sandbox)
    }

    pub async fn wait_started(&self, mut sandbox: Sandbox) -> Result<Sandbox> {
        loop {
            match sandbox.status.as_str() {
                "running" => return Ok(sandbox),
                "scheduled" | "queued" | "provisioning" | "initializing" => {}
                status => {
                    return Err(api_error(format!(
                        "devbox entered {status} before readiness"
                    )))
                }
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
            sandbox = self.get(&sandbox.id).await?;
        }
    }
}

pub(super) fn lifetime(timeout_ms: Option<u64>) -> Result<Duration> {
    let seconds = timeout_ms
        .unwrap_or(600_000)
        .div_ceil(1000)
        .saturating_add(120);
    if seconds > 172_800 {
        return Err(api_error(
            "action timeout plus 120 seconds must not exceed Runloop's 48-hour keep-alive limit"
                .into(),
        ));
    }
    Ok(Duration::from_secs(seconds))
}

pub(super) fn validate_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 256
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(api_error("invalid resource ID in API response".into()));
    }
    Ok(())
}
pub(super) fn http_error(source: reqwest::Error) -> Error {
    super::super::http::http_error("runloop", source)
}
pub(super) fn api_error(message: String) -> Error {
    Error::RemoteProviderApi {
        provider: "runloop".into(),
        message,
    }
}
