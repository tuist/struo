use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::remote::http;
use crate::remote::lifecycle::HostedSandbox;
use crate::{Error, RemoteExecution, RemoteIoLimits, Result};

#[derive(Clone)]
pub(super) struct Client {
    pub http: reqwest::Client,
    pub endpoint: reqwest::Url,
    pub token: String,
    pub namespace: String,
    pub sandbox_origin: Option<reqwest::Url>,
    pub limits: RemoteIoLimits,
}

#[derive(Deserialize, Serialize)]
pub(super) struct Identity {
    pub id: String,
    pub name: String,
    pub namespace: String,
}

pub(super) struct Sandbox {
    pub identity: String,
    pub provider_id: String,
}

impl HostedSandbox for Sandbox {
    fn id(&self) -> &str {
        &self.identity
    }
}

#[derive(Deserialize)]
pub(super) struct Info {
    pub id: String,
    pub name: Option<String>,
    pub namespace: String,
    pub status: String,
    pub sandbox_url: Option<String>,
    pub allow_unauthenticated_access: bool,
    pub auto_resume: bool,
    pub exposed_ports: Option<Vec<u16>>,
}

impl Client {
    pub fn from_env(remote: &RemoteExecution) -> Result<Self> {
        let token = std::env::var("STRUO_TENSORLAKE_API_KEY")
            .or_else(|_| std::env::var("TENSORLAKE_API_KEY"))
            .map_err(|_| api_error("set STRUO_TENSORLAKE_API_KEY or TENSORLAKE_API_KEY"))?;
        let namespace = remote
            .properties
            .get("namespace")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| api_error("Tensorlake requires properties.namespace"))?;
        Self::new(
            &std::env::var("STRUO_TENSORLAKE_API_URL")
                .unwrap_or_else(|_| "https://api.tensorlake.ai".into()),
            token,
            namespace.into(),
            std::env::var("STRUO_TENSORLAKE_SANDBOX_ORIGIN")
                .ok()
                .as_deref(),
        )
    }

    pub fn new(
        endpoint: &str,
        token: String,
        namespace: String,
        sandbox_origin: Option<&str>,
    ) -> Result<Self> {
        let endpoint = secure_url(endpoint)?;
        let sandbox_origin = sandbox_origin.map(secure_url).transpose()?;
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(30))
            .build()
            .map_err(|error| http::http_error("tensorlake", error))?;
        Ok(Self {
            http,
            endpoint,
            token,
            namespace,
            sandbox_origin,
            limits: RemoteIoLimits::default(),
        })
    }

    pub fn control(
        &self,
        method: reqwest::Method,
        id: Option<&str>,
    ) -> Result<reqwest::RequestBuilder> {
        let mut url = self.endpoint.clone();
        let mut segments = url
            .path_segments_mut()
            .map_err(|()| api_error("invalid control URL"))?;
        segments.pop_if_empty().push("sandboxes");
        if let Some(id) = id {
            segments.push(id);
        }
        drop(segments);
        Ok(self
            .http
            .request(method, url)
            .bearer_auth(&self.token)
            .timeout(Duration::from_secs(30)))
    }

    pub async fn decode<T: serde::de::DeserializeOwned>(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<T> {
        let response = request
            .send()
            .await
            .map_err(|error| http::http_error("tensorlake", error))?;
        http::decode(
            response,
            "tensorlake",
            self.limits.control_bytes(),
            self.limits,
        )
        .await
    }

    pub fn json(
        &self,
        request: reqwest::RequestBuilder,
        body: &serde_json::Value,
    ) -> Result<reqwest::RequestBuilder> {
        Ok(request
            .header("Content-Type", "application/json")
            .body(http::encode(
                body,
                "tensorlake",
                self.limits.control_bytes(),
            )?))
    }

    pub fn verify(&self, identity: &Identity, info: &Info) -> Result<()> {
        if identity.namespace != self.namespace
            || info.id != identity.id
            || info.name.as_deref() != Some(&identity.name)
            || info.namespace != identity.namespace
        {
            return Err(api_error("sandbox identity changed; refusing ownership"));
        }
        Ok(())
    }

    pub fn guest_url(&self, value: &str) -> Result<reqwest::Url> {
        let mut url = secure_url(value)?;
        let allowed = self.sandbox_origin.as_ref().map_or_else(
            || {
                url.scheme() == "https"
                    && url.port_or_known_default() == Some(443)
                    && url
                        .host_str()
                        .is_some_and(|host| host.ends_with(".sandbox.tensorlake.ai"))
            },
            |origin| origin.origin() == url.origin(),
        );
        if !allowed {
            return Err(api_error(
                "sandbox_url is outside the trusted Tensorlake origin policy",
            ));
        }
        url.set_path("/");
        Ok(url)
    }
}

fn secure_url(value: &str) -> Result<reqwest::Url> {
    let url = reqwest::Url::parse(value).map_err(|_| api_error("invalid API URL"))?;
    let local_test = cfg!(test) && url.scheme() == "http" && url.host_str() == Some("127.0.0.1");
    if (url.scheme() != "https" && !local_test)
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(api_error(
            "API URLs require HTTPS without credentials, query, or fragment",
        ));
    }
    Ok(url)
}

pub(super) fn api_error(message: &str) -> Error {
    Error::RemoteProviderApi {
        provider: "tensorlake".into(),
        message: message.into(),
    }
}
