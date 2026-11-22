use std::collections::BTreeMap;
use std::time::Duration;

use reqwest::{Method, RequestBuilder};
use serde::Deserialize;

use crate::remote::{http, lifecycle::MARKER_KEY};
use crate::{Error, RemoteExecution, RemoteIoLimits, ResourceRequest, Result};

#[derive(Clone)]
pub(super) struct Config {
    pub endpoint: String,
    pub token: String,
    pub team: String,
    pub project: String,
}

impl Config {
    pub(super) fn from_env(remote: &RemoteExecution) -> Result<Self> {
        let (Some(team), Some(project)) = (&remote.account, &remote.project) else {
            return Err(Error::RemoteProviderConfig {
                provider: "vercel".into(),
                message: "declare account and project in the execution binding".into(),
            });
        };
        let required = |name: &str| {
            std::env::var(name)
                .ok()
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| Error::RemoteProviderConfig {
                    provider: "vercel".into(),
                    message: format!("set {name}"),
                })
        };
        Ok(Self {
            endpoint: "https://api.vercel.com".into(),
            token: required("VERCEL_TOKEN")?,
            team: team.clone(),
            project: project.clone(),
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
    pub sandbox: Named,
    pub session: Session,
}

#[derive(Deserialize)]
pub(super) struct Named {
    pub name: String,
    pub tags: Option<BTreeMap<String, String>>,
}

#[derive(Deserialize)]
pub(super) struct Session {
    pub id: String,
    pub status: String,
}

impl Client {
    pub(super) fn new(config: Config) -> Result<Self> {
        Ok(Self {
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .pool_max_idle_per_host(0)
                .connect_timeout(Duration::from_secs(30))
                .build()
                .map_err(http_error)?,
            config,
            limits: RemoteIoLimits::default(),
        })
    }

    pub(super) fn request(&self, method: Method, path: &str) -> RequestBuilder {
        self.http
            .request(method, format!("{}{path}", self.config.endpoint))
            .bearer_auth(&self.config.token)
            .query(&[("teamId", &self.config.team)])
    }

    pub(super) fn json(
        &self,
        request: RequestBuilder,
        value: &impl serde::Serialize,
    ) -> Result<RequestBuilder> {
        Ok(request
            .header("Content-Type", "application/json")
            .body(http::encode(value, "vercel", self.limits.control_bytes())?))
    }

    pub(super) async fn decode<T: serde::de::DeserializeOwned>(
        &self,
        request: RequestBuilder,
    ) -> Result<T> {
        http::decode(
            request.send().await.map_err(http_error)?,
            "vercel",
            self.limits.control_bytes(),
            self.limits,
        )
        .await
    }

    pub(super) async fn create(
        &self,
        runtime: &str,
        resources: &ResourceRequest,
        lifetime: u64,
        nonce: &str,
    ) -> Result<Sandbox> {
        let cpu = (resources.cpu_slots.max(1) as u64)
            .max(resources.memory_bytes.div_ceil(2048 * 1024 * 1024));
        let cpu = if cpu > 1 { cpu.div_ceil(2) * 2 } else { cpu };
        let body = serde_json::json!({
            "runtime": runtime, "name": name(nonce), "projectId": self.config.project,
            "persistent": false, "timeout": lifetime,
            "resources": {"vcpus": cpu, "memory": cpu * 2048},
            "tags": {MARKER_KEY: nonce},
        });
        let sandbox: Sandbox = self
            .decode(
                self.json(
                    self.request(Method::POST, "/v2/sandboxes")
                        .timeout(Duration::from_secs(60)),
                    &body,
                )?,
            )
            .await?;
        if sandbox.sandbox.name != name(nonce)
            || sandbox
                .sandbox
                .tags
                .as_ref()
                .and_then(|tags| tags.get(MARKER_KEY))
                .map(String::as_str)
                != Some(nonce)
        {
            return Err(api_error(
                "create response did not confirm sandbox ownership",
            ));
        }
        valid_id(&sandbox.session.id)?;
        tracing::debug!(provider = "vercel", sandbox = %sandbox.sandbox.name, "created sandbox");
        Ok(sandbox)
    }

    pub(super) async fn ready(&self, id: &str) -> Result<()> {
        #[derive(Deserialize)]
        struct Response {
            session: Session,
        }
        loop {
            let response: Response = self
                .decode(
                    self.request(Method::GET, &format!("/v2/sandboxes/sessions/{id}"))
                        .timeout(Duration::from_secs(30)),
                )
                .await?;
            if response.session.id != id {
                return Err(api_error("session identity changed"));
            }
            match response.session.status.as_str() {
                "running" => return Ok(()),
                "pending" | "starting" => tokio::time::sleep(Duration::from_millis(250)).await,
                _ => return Err(api_error("session did not reach running state")),
            }
        }
    }
}

pub(super) fn name(nonce: &str) -> String {
    format!("struo-{nonce}")
}

pub(super) fn valid_id(id: &str) -> Result<()> {
    if !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        Ok(())
    } else {
        Err(api_error("invalid provider resource identifier"))
    }
}

pub(super) fn http_error(error: reqwest::Error) -> Error {
    http::http_error("vercel", error)
}
pub(super) fn api_error(message: impl Into<String>) -> Error {
    Error::RemoteProviderApi {
        provider: "vercel".into(),
        message: message.into(),
    }
}
