use std::collections::BTreeMap;
use std::time::Duration;

use serde::Deserialize;

use crate::remote::lifecycle::{Marker, MARKER_KEY};
use crate::{Error, RemoteIoLimits, Result};

#[derive(Clone)]
pub(super) struct Config {
    pub url: String,
    pub api_key: String,
}

impl Config {
    pub(super) fn from_env() -> Result<Self> {
        Ok(Self {
            url: std::env::var("STRUO_BOAT_API_URL")
                .unwrap_or_else(|_| "https://boat.dev/api/v1".into()),
            api_key: std::env::var("STRUO_BOAT_API_KEY")
                .or_else(|_| std::env::var("BOAT_API_KEY"))
                .map_err(|_| Error::RemoteProviderConfig {
                    provider: "boat".into(),
                    message: "set STRUO_BOAT_API_KEY or BOAT_API_KEY".into(),
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

#[derive(Deserialize)]
pub(super) struct Sandbox {
    pub id: String,
    pub state: String,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct SandboxResponse {
    sandbox: Sandbox,
}

impl Client {
    pub(super) fn new(config: Config) -> Result<Self> {
        let http = reqwest::Client::builder()
            .pool_max_idle_per_host(0)
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(30))
            .build()
            .map_err(http_error)?;
        Ok(Self {
            http,
            config,
            limits: RemoteIoLimits::default(),
        })
    }

    pub(super) fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(
                method,
                format!("{}{path}", self.config.url.trim_end_matches('/')),
            )
            .bearer_auth(&self.config.api_key)
            .timeout(Duration::from_secs(60))
    }

    pub(super) async fn decode<T: serde::de::DeserializeOwned>(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<T> {
        let response = request.send().await.map_err(http_error)?;
        super::super::http::decode(response, "boat", self.limits.control_bytes(), self.limits).await
    }

    pub(super) fn json(
        &self,
        request: reqwest::RequestBuilder,
        body: &impl serde::Serialize,
    ) -> Result<reqwest::RequestBuilder> {
        Ok(request
            .header("Content-Type", "application/json")
            .body(super::super::http::encode(
                body,
                "boat",
                self.limits.control_bytes(),
            )?))
    }

    pub(super) fn sandbox_path(sandbox: &Sandbox, suffix: &str) -> Result<String> {
        validate_id(&sandbox.id)?;
        Ok(format!("/sandboxes/{}{suffix}", sandbox.id))
    }

    pub(super) async fn create(
        &self,
        snapshot: Option<&str>,
        ttl: Duration,
        marker: Option<&Marker>,
    ) -> Result<Sandbox> {
        let mut body = serde_json::json!({ "ttlSeconds": ttl.as_secs(), "noEnv": true, "snapshots": false, "monitoring": false, "visibility": "private" });
        if let Some(snapshot) = snapshot {
            body["from"] = snapshot.into();
        }
        let mut request = self.request(reqwest::Method::POST, "/sandboxes");
        if let Some(marker) = marker {
            body["labels"] = serde_json::json!({ MARKER_KEY: marker.nonce });
            request = request.header("Idempotency-Key", &marker.nonce);
        }
        let response: SandboxResponse = self.decode(self.json(request, &body)?).await?;
        validate_id(&response.sandbox.id)?;
        Ok(response.sandbox)
    }

    pub(super) async fn wait_started(&self, sandbox: &Sandbox) -> Result<()> {
        let path = Self::sandbox_path(sandbox, "")?;
        let mut state = sandbox.state.clone();
        let wait = async {
            loop {
                match state.as_str() {
                    "ready" | "idle" | "running" => return Ok(()),
                    "init" | "provisioning" | "provisioned" | "cloning" => {}
                    _ => return Err(api_error("sandbox did not reach a runnable state")),
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
                let response: SandboxResponse = self
                    .decode(self.request(reqwest::Method::GET, &path))
                    .await?;
                if response.sandbox.id != sandbox.id {
                    return Err(api_error("sandbox status identity mismatch"));
                }
                state = response.sandbox.state;
            }
        };
        tokio::time::timeout(Duration::from_mins(10), wait)
            .await
            .map_err(|_| Error::Timeout(Duration::from_mins(10)))?
    }
}

pub(super) fn lifetime(timeout_ms: Option<u64>) -> Result<Duration> {
    let seconds = timeout_ms.map_or(3600, |ms| ms.div_ceil(1000).saturating_add(300));
    if seconds > 2_592_000 {
        return Err(Error::RemoteProviderConfig {
            provider: "boat".into(),
            message: "action timeout plus five minutes must fit Boat's 30-day TTL limit".into(),
        });
    }
    Ok(Duration::from_secs(seconds))
}

pub(super) fn validate_id(id: &str) -> Result<()> {
    if id.strip_prefix("bx_").is_some_and(|suffix| {
        suffix.len() == 8
            && suffix
                .bytes()
                .all(|b| b"23456789abcdefghjkmnpqrstuvwxyz".contains(&b))
    }) {
        Ok(())
    } else {
        Err(api_error("invalid sandbox identity"))
    }
}

pub(super) fn http_error(source: reqwest::Error) -> Error {
    super::super::http::http_error("boat", source)
}
pub(super) fn api_error(message: &str) -> Error {
    Error::RemoteProviderApi {
        provider: "boat".into(),
        message: message.into(),
    }
}
