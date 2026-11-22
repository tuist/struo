use std::collections::BTreeMap;
use std::time::Duration;

use reqwest::{Method, RequestBuilder, Url};
use serde::Deserialize;

pub(super) use super::endpoint::Endpoint;
use super::PROVIDER;
use crate::remote::{
    http,
    lifecycle::{self, HostedSandbox, Lookup, Marker, Provider, MARKER_KEY},
};
use crate::{Error, RemoteIoLimits, ResourceRequest, Result};

#[derive(Clone)]
pub(super) struct Client {
    pub http: reqwest::Client,
    pub base: Url,
    key: String,
    pub limits: RemoteIoLimits,
    pub secure_access: bool,
}

#[derive(Deserialize)]
pub(super) struct Sandbox {
    pub id: String,
}

impl HostedSandbox for Sandbox {
    fn id(&self) -> &str {
        &self.id
    }
}

impl Client {
    pub fn from_env() -> Result<Self> {
        let base = std::env::var("STRUO_OPENSANDBOX_URL").map_err(|_| {
            config_error("set STRUO_OPENSANDBOX_URL to the lifecycle API base URL, including /v1")
        })?;
        let key = std::env::var("STRUO_OPENSANDBOX_API_KEY")
            .or_else(|_| std::env::var("OPEN_SANDBOX_API_KEY"))
            .map_err(|_| config_error("set OPEN_SANDBOX_API_KEY or STRUO_OPENSANDBOX_API_KEY"))?;
        Self::new(&base, key)
    }

    pub fn new(base: &str, key: String) -> Result<Self> {
        let base = valid_url(base)?;
        if key.trim().is_empty() {
            return Err(config_error("OpenSandbox API key must not be empty"));
        }
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .pool_max_idle_per_host(0)
            .connect_timeout(Duration::from_secs(30))
            .build()
            .map_err(http_error)?;
        Ok(Self {
            http,
            base,
            key,
            limits: RemoteIoLimits::default(),
            secure_access: true,
        })
    }

    pub fn control(&self, method: Method, path: &str) -> RequestBuilder {
        self.http
            .request(
                method,
                format!("{}{path}", self.base.as_str().trim_end_matches('/')),
            )
            .header("OPEN-SANDBOX-API-KEY", &self.key)
            .timeout(self.limits.stall_timeout)
    }

    pub async fn json<T: serde::de::DeserializeOwned>(&self, request: RequestBuilder) -> Result<T> {
        http::decode(
            request.send().await.map_err(http_error)?,
            PROVIDER,
            self.limits.control_bytes(),
            self.limits,
        )
        .await
    }

    pub async fn create(
        &self,
        image: &str,
        resources: &ResourceRequest,
        timeout: Option<u64>,
        marker: Option<&Marker>,
    ) -> Result<Sandbox> {
        let metadata = marker
            .map(|marker| BTreeMap::from([(MARKER_KEY, marker.nonce.as_str())]))
            .unwrap_or_default();
        let memory = if resources.memory_bytes == 0 {
            crate::resources::DEFAULT_ACTION_MEMORY_BYTES
        } else {
            resources.memory_bytes
        };
        let body = http::encode(
            &serde_json::json!({
                "image": {"uri": image}, "entrypoint": ["tail", "-f", "/dev/null"],
                "timeout": lifetime(timeout).as_secs(), "metadata": metadata,
                "secureAccess": self.secure_access,
                "resourceLimits": {"cpu": resources.cpu_slots.max(1).to_string(), "memory": format!("{}Mi", memory.div_ceil(1024 * 1024))}
            }),
            PROVIDER,
            self.limits.control_bytes(),
        )?;
        self.json(
            self.control(Method::POST, "/sandboxes")
                .header("Content-Type", "application/json")
                .body(body),
        )
        .await
    }
}

impl Provider for Client {
    const NAME: &'static str = PROVIDER;
    fn scope(&self) -> String {
        lifecycle::scope(PROVIDER, self.base.as_str())
    }
    fn credential(&self) -> String {
        lifecycle::credential(PROVIDER, &self.key)
    }
    async fn find(&self, nonce: &str) -> Result<Lookup> {
        #[derive(Deserialize)]
        struct Listing {
            items: Vec<Listed>,
            pagination: Pagination,
        }
        #[derive(Deserialize)]
        struct Listed {
            id: String,
            #[serde(default)]
            metadata: BTreeMap<String, String>,
        }
        #[derive(Deserialize)]
        struct Pagination {
            #[serde(rename = "hasNextPage")]
            has_next_page: bool,
        }
        let listing: Listing = self
            .json(self.control(Method::GET, "/sandboxes").query(&[
                ("metadata", format!("{MARKER_KEY}={nonce}")),
                ("pageSize", "100".into()),
                ("page", "1".into()),
            ]))
            .await?;
        Ok(lifecycle::classify(
            listing.items.iter().map(|item| {
                (
                    item.id.as_str(),
                    item.metadata.get(MARKER_KEY).map(String::as_str),
                    true,
                )
            }),
            nonce,
            listing.pagination.has_next_page,
        ))
    }
    async fn delete(&self, id: &str) -> Result<()> {
        let response = self
            .control(Method::DELETE, &format!("/sandboxes/{}", segment(id)))
            .send()
            .await
            .map_err(http_error)?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(());
        }
        http::checked(response, PROVIDER).map(|_| ())
    }
}

pub(super) fn lifetime(timeout: Option<u64>) -> Duration {
    Duration::from_secs(
        timeout
            .map_or(900, |ms| ms.div_ceil(1000).saturating_add(300))
            .max(60),
    )
}

pub(super) fn segment(value: &str) -> String {
    value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"-_~".contains(&byte) {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

pub(super) fn http_error(source: reqwest::Error) -> Error {
    http::http_error(PROVIDER, source)
}

fn config_error(message: &str) -> Error {
    Error::RemoteProviderConfig {
        provider: PROVIDER.into(),
        message: message.into(),
    }
}

pub(super) fn valid_url(value: &str) -> Result<Url> {
    let url = Url::parse(value).map_err(|_| config_error("invalid OpenSandbox endpoint URL"))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(config_error(
            "OpenSandbox endpoints require HTTP(S) without user-info, query, or fragment",
        ));
    }
    Ok(url)
}
