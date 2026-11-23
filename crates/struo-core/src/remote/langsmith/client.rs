use std::collections::BTreeMap;
use std::time::Duration;

use serde::Deserialize;

use crate::remote::{
    http,
    lifecycle::{Marker, MARKER_KEY},
};
use crate::{Error, RemoteIoLimits, Result};

#[derive(Clone)]
pub(super) struct Config {
    pub url: String,
    pub api_key: String,
    pub workspace: String,
    pub dataplane_origins: Vec<String>,
}

impl Config {
    pub(super) fn from_env() -> Result<Self> {
        Ok(Self {
            url: std::env::var("STRUO_LANGSMITH_API_URL")
                .unwrap_or_else(|_| "https://api.smith.langchain.com".into()),
            api_key: std::env::var("STRUO_LANGSMITH_API_KEY")
                .or_else(|_| std::env::var("LANGSMITH_API_KEY"))
                .map_err(|_| config_error("set STRUO_LANGSMITH_API_KEY or LANGSMITH_API_KEY"))?,
            workspace: std::env::var("STRUO_LANGSMITH_WORKSPACE_ID")
                .or_else(|_| std::env::var("LANGSMITH_WORKSPACE_ID"))
                .map_err(|_| {
                    config_error("set STRUO_LANGSMITH_WORKSPACE_ID or LANGSMITH_WORKSPACE_ID")
                })?,
            dataplane_origins: std::env::var("STRUO_LANGSMITH_DATAPLANE_ORIGINS")
                .unwrap_or_default()
                .split(',')
                .filter(|v| !v.trim().is_empty())
                .map(|v| v.trim().to_string())
                .collect(),
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
    pub name: String,
    pub status: String,
    #[serde(default)]
    pub dataplane_url: Option<String>,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
}

impl Client {
    pub(super) fn new(config: Config) -> Result<Self> {
        if validate_url(&config.url)?.path() != "/" {
            return Err(config_error(
                "LangSmith API URL must be an origin without a path",
            ));
        }
        validate_id(&config.workspace)?;
        if config.api_key.trim().is_empty() {
            return Err(config_error("LangSmith API key must not be empty"));
        }
        for origin in &config.dataplane_origins {
            let url = validate_url(origin)?;
            if url.path() != "/" {
                return Err(config_error(
                    "data-plane allowlist entries must be exact origins without paths",
                ));
            }
        }
        Ok(Self {
            http: reqwest::Client::builder()
                .pool_max_idle_per_host(0)
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(30))
                .build()
                .map_err(http_error)?,
            config,
            limits: RemoteIoLimits::default(),
        })
    }

    fn authenticated(&self, method: reqwest::Method, url: String) -> reqwest::RequestBuilder {
        self.http
            .request(method, url)
            .header("X-Api-Key", &self.config.api_key)
            .header("X-Tenant-Id", &self.config.workspace)
    }

    pub(super) fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.authenticated(
            method,
            format!("{}{path}", self.config.url.trim_end_matches('/')),
        )
        .timeout(Duration::from_secs(60))
    }

    pub(super) fn runtime(
        &self,
        sandbox: &Sandbox,
        method: reqwest::Method,
        suffix: &str,
    ) -> Result<reqwest::RequestBuilder> {
        let raw = sandbox
            .dataplane_url
            .as_deref()
            .ok_or_else(|| api_error("ready sandbox omitted data-plane URL"))?;
        let url = validate_url(raw)?;
        let base = validate_url(&self.config.url)?;
        if url.origin() != base.origin()
            && !self.config.dataplane_origins.iter().any(|allowed| {
                validate_url(allowed).is_ok_and(|allowed| allowed.origin() == url.origin())
            })
        {
            return Err(config_error("returned data-plane origin is not trusted; configure STRUO_LANGSMITH_DATAPLANE_ORIGINS with its exact HTTPS origin"));
        }
        Ok(self.authenticated(method, format!("{}{suffix}", raw.trim_end_matches('/'))))
    }

    pub(super) async fn decode<T: serde::de::DeserializeOwned>(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<T> {
        http::decode(
            request.send().await.map_err(http_error)?,
            "langsmith",
            self.limits.control_bytes(),
            self.limits,
        )
        .await
    }

    pub(super) fn json(
        &self,
        request: reqwest::RequestBuilder,
        body: &impl serde::Serialize,
    ) -> Result<reqwest::RequestBuilder> {
        Ok(request
            .header("Content-Type", "application/json")
            .body(http::encode(
                body,
                "langsmith",
                self.limits.control_bytes(),
            )?))
    }

    pub(super) async fn create(&self, snapshot: &str, marker: Option<&Marker>) -> Result<Sandbox> {
        let marker =
            marker.ok_or_else(|| api_error("sandbox creation requires durable ownership"))?;
        let body = serde_json::json!({
            "snapshot": snapshot,
            "name": format!("struo-{}", marker.nonce),
            "labels": {MARKER_KEY: marker.nonce},
            "persistent": false,
            "restore_memory": false,
            "preserve_memory_on_stop": false,
            "idle_ttl_seconds": 600,
            "delete_after_stop_seconds": 60
        });
        let sandbox: Sandbox = self
            .decode(self.json(
                self.request(reqwest::Method::POST, "/api/v2/sandboxes/boxes"),
                &body,
            )?)
            .await?;
        validate_id(&sandbox.id)?;
        if sandbox.labels.get(MARKER_KEY) != Some(&marker.nonce) {
            return Err(api_error(
                "created sandbox did not preserve ownership label",
            ));
        }
        Ok(sandbox)
    }

    pub(super) async fn wait_started(&self, mut sandbox: Sandbox) -> Result<Sandbox> {
        let id = sandbox.id.clone();
        let name = sandbox.name.clone();
        let mut url = validate_url(&self.config.url)?;
        url.path_segments_mut()
            .map_err(|()| config_error("invalid API base"))?
            .pop_if_empty()
            .extend(["api", "v2", "sandboxes", "boxes", &name]);
        loop {
            match sandbox.status.as_str() {
                "ready" => return Ok(sandbox),
                "provisioning" => {}
                _ => return Err(api_error("sandbox failed to become ready")),
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
            sandbox = self
                .decode(
                    self.authenticated(reqwest::Method::GET, url.to_string())
                        .timeout(Duration::from_secs(60)),
                )
                .await?;
            if sandbox.id != id {
                return Err(api_error("sandbox readiness identity changed"));
            }
        }
    }
}

fn validate_url(raw: &str) -> Result<reqwest::Url> {
    let url = reqwest::Url::parse(raw).map_err(|_| config_error("invalid LangSmith endpoint"))?;
    let local_test = cfg!(test) && url.scheme() == "http" && url.host_str() == Some("127.0.0.1");
    if (url.scheme() != "https" && !local_test)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(config_error(
            "LangSmith endpoints require HTTPS without user-info, query or fragment",
        ));
    }
    Ok(url)
}

pub(super) fn validate_id(id: &str) -> Result<()> {
    uuid::Uuid::parse_str(id)
        .map(|_| ())
        .map_err(|_| api_error("invalid LangSmith UUID"))
}

pub(super) fn config_error(message: &str) -> Error {
    Error::RemoteProviderConfig {
        provider: "langsmith".into(),
        message: message.into(),
    }
}
pub(super) fn api_error(message: &str) -> Error {
    Error::RemoteProviderApi {
        provider: "langsmith".into(),
        message: message.into(),
    }
}
pub(super) fn http_error(source: reqwest::Error) -> Error {
    http::http_error("langsmith", source)
}
