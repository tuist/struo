use std::time::Duration;

use reqwest::{Method, RequestBuilder};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::remote::http;
use crate::{Error, RemoteIoLimits, Result};

#[derive(Clone)]
pub(super) struct Client {
    pub http: reqwest::Client,
    pub url: String,
    pub key: String,
    pub limits: RemoteIoLimits,
}

#[derive(Deserialize)]
pub(super) struct Sandbox {
    pub id: String,
    pub status: String,
    #[serde(default)]
    pub agent: Option<Agent>,
}

#[derive(Deserialize)]
pub(super) struct Agent {
    pub url: Option<String>,
    pub token: Option<String>,
}

impl Client {
    pub fn from_env() -> Result<Self> {
        let key = std::env::var("STRUO_TOGETHER_API_KEY").or_else(|_| std::env::var("TOGETHER_API_KEY"))
            .map_err(|_| config_error("set STRUO_TOGETHER_API_KEY or TOGETHER_API_KEY; your organization must have Together Code Sandbox access enabled"))?;
        let url = std::env::var("STRUO_TOGETHER_API_URL")
            .unwrap_or_else(|_| "https://api.bartender.codesandbox.io".into());
        Self::new(url, key)
    }

    pub fn new(url: String, key: String) -> Result<Self> {
        let parsed =
            reqwest::Url::parse(&url).map_err(|_| config_error("invalid Together API URL"))?;
        if !matches!(parsed.scheme(), "http" | "https")
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err(config_error(
                "Together API URL must be HTTP(S) without credentials, query or fragment",
            ));
        }
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .pool_max_idle_per_host(0)
            .connect_timeout(Duration::from_secs(30))
            .build()
            .map_err(http_error)?;
        Ok(Self {
            http,
            url,
            key,
            limits: RemoteIoLimits::default(),
        })
    }

    pub fn control(&self, method: Method, path: &str) -> RequestBuilder {
        self.http
            .request(
                method,
                format!("{}/v1{path}", self.url.trim_end_matches('/')),
            )
            .bearer_auth(&self.key)
            .timeout(Duration::from_secs(60))
    }

    pub fn guest(&self, method: Method, sandbox: &Sandbox, path: &str) -> Result<RequestBuilder> {
        let agent = sandbox
            .agent
            .as_ref()
            .ok_or_else(|| api_error("missing agent connection details".into()))?;
        let url = agent
            .url
            .as_deref()
            .ok_or_else(|| api_error("missing agent URL".into()))?;
        let token = agent
            .token
            .as_deref()
            .ok_or_else(|| api_error("missing agent token".into()))?;
        Ok(self
            .http
            .request(method, format!("{}{path}", url.trim_end_matches('/')))
            .bearer_auth(token))
    }

    pub async fn json<T: DeserializeOwned>(&self, request: RequestBuilder) -> Result<T> {
        http::decode(
            request.send().await.map_err(http_error)?,
            "together",
            self.limits.control_bytes(),
            self.limits,
        )
        .await
    }

    pub fn body(&self, request: RequestBuilder, value: &impl Serialize) -> Result<RequestBuilder> {
        Ok(request
            .header("Content-Type", "application/json")
            .body(http::encode(
                value,
                "together",
                self.limits.control_bytes(),
            )?))
    }

    pub async fn wait_running(&self, sandbox: Sandbox) -> Result<Sandbox> {
        let sandbox = if matches!(sandbox.status.as_str(), "starting" | "terminating") {
            self.json(self.control(
                Method::POST,
                &format!("/sandboxes/{}/wait", component(&sandbox.id)),
            ))
            .await?
        } else {
            sandbox
        };
        if sandbox.status != "running" {
            return Err(api_error(format!(
                "sandbox did not start (status {})",
                sandbox.status
            )));
        }
        Ok(sandbox)
    }
}

pub(super) fn component(value: &str) -> String {
    let mut url = reqwest::Url::parse("http://localhost/").expect("static URL");
    url.path_segments_mut().expect("HTTP URL").push(value);
    url.path()[1..].to_string()
}

pub(super) fn http_error(source: reqwest::Error) -> Error {
    http::http_error("together", source)
}
pub(super) fn api_error(message: String) -> Error {
    Error::RemoteProviderApi {
        provider: "together".into(),
        message,
    }
}
pub(super) fn config_error(message: &str) -> Error {
    Error::RemoteProviderConfig {
        provider: "together".into(),
        message: message.into(),
    }
}
