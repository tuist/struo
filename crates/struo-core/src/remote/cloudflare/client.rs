use std::path::Path;
use std::time::Duration;

use reqwest::{Method, RequestBuilder};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::io::ReaderStream;

use crate::remote::{http, lifecycle, PhasePermit};
use crate::{Error, ExecutionPermit, RemoteIoLimits, Result};

pub(super) const PROVIDER: &str = "cloudflare";
pub(super) const LIFETIME: Duration = Duration::from_hours(1);
const CHUNK_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Clone)]
pub(super) struct Client {
    http: reqwest::Client,
    endpoint: String,
    token: String,
    pub limits: RemoteIoLimits,
}

#[derive(Deserialize)]
pub(super) struct Sandbox {
    pub id: String,
}

impl lifecycle::HostedSandbox for Sandbox {
    fn id(&self) -> &str {
        &self.id
    }
}

impl Client {
    pub fn from_env(limits: RemoteIoLimits) -> Result<Self> {
        let endpoint = std::env::var("STRUO_CLOUDFLARE_BRIDGE_URL")
            .map_err(|_| config_error("set STRUO_CLOUDFLARE_BRIDGE_URL to your deployed Worker"))?;
        let token = std::env::var("STRUO_CLOUDFLARE_BRIDGE_TOKEN")
            .map_err(|_| config_error("set STRUO_CLOUDFLARE_BRIDGE_TOKEN"))?;
        Self::new(&endpoint, token, limits)
    }

    pub fn new(endpoint: &str, token: String, limits: RemoteIoLimits) -> Result<Self> {
        let url = reqwest::Url::parse(endpoint).map_err(|_| config_error("invalid bridge URL"))?;
        if !(url.scheme() == "https"
            || (url.scheme() == "http"
                && matches!(url.host_str(), Some("127.0.0.1" | "localhost"))))
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || token.trim().is_empty()
        {
            return Err(config_error("bridge requires HTTPS (HTTP only on loopback), no URL credentials/query/fragment, and a non-empty token"));
        }
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(30))
            .pool_max_idle_per_host(0)
            .build()
            .map_err(http_error)?;
        Ok(Self {
            http,
            endpoint: endpoint.trim_end_matches('/').to_string(),
            token,
            limits,
        })
    }

    fn request(&self, method: Method, path: &str) -> RequestBuilder {
        self.http
            .request(method, format!("{}/v1/{path}", self.endpoint))
            .bearer_auth(&self.token)
    }

    pub async fn json<T: DeserializeOwned>(&self, path: &str, value: &Value) -> Result<T> {
        let response = self
            .request(Method::POST, path)
            .header("Content-Type", "application/json")
            .body(http::encode(value, PROVIDER, self.limits.control_bytes())?)
            .timeout(LIFETIME)
            .send()
            .await
            .map_err(http_error)?;
        http::decode(response, PROVIDER, self.limits.control_bytes(), self.limits).await
    }

    pub async fn upload(&self, id: &str, source: &Path, len: u64) -> Result<()> {
        let mut offset = 0;
        while offset < len {
            let mut file = tokio::fs::File::open(source).await.map_err(file_error)?;
            file.seek(std::io::SeekFrom::Start(offset))
                .await
                .map_err(file_error)?;
            let size = CHUNK_BYTES.min(len - offset);
            let progress = http::Progress::new();
            let body = reqwest::Body::wrap_stream(
                progress.track_bytes(ReaderStream::new(file.take(size)), size),
            );
            let response = progress
                .watch_upload(PROVIDER, self.limits, async {
                    self.request(Method::PUT, &format!("sandboxes/{id}/inputs/{offset}"))
                        .header("Content-Length", size)
                        .body(body)
                        .send()
                        .await
                        .map_err(http_error)
                })
                .await?;
            http::checked(response, PROVIDER)?;
            offset += size;
        }
        let _: Value = self
            .json(&format!("sandboxes/{id}/stage"), &json!({}))
            .await?;
        Ok(())
    }

    pub async fn download(
        &self,
        path: &str,
        destination: &Path,
        lease: Option<&ExecutionPermit>,
        copies: u64,
        limit: u64,
    ) -> Result<PhasePermit> {
        let response = http::Progress::new()
            .watch(
                PROVIDER,
                "download headers",
                self.limits.stall_timeout,
                async {
                    self.request(Method::GET, path)
                        .timeout(self.limits.artifact_download_timeout)
                        .send()
                        .await
                        .map_err(http_error)
                },
            )
            .await?;
        let limits = RemoteIoLimits {
            max_archive_bytes: limit.min(self.limits.max_archive_bytes),
            ..self.limits
        };
        http::download(response, destination, PROVIDER, limits, lease, copies).await
    }
}

impl lifecycle::Provider for Client {
    const NAME: &'static str = PROVIDER;

    fn scope(&self) -> String {
        lifecycle::scope(PROVIDER, &self.endpoint)
    }
    fn credential(&self) -> String {
        lifecycle::credential(PROVIDER, &self.token)
    }

    async fn find(&self, nonce: &str) -> Result<lifecycle::Lookup> {
        #[derive(Deserialize)]
        struct Entry {
            id: String,
            marker: String,
        }
        let response = self
            .request(Method::GET, "sandboxes")
            .query(&[("marker", nonce)])
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(http_error)?;
        let entries: Vec<Entry> =
            http::decode(response, PROVIDER, self.limits.control_bytes(), self.limits).await?;
        Ok(lifecycle::classify(
            entries.iter().map(|entry| {
                let marker = (entry.id == entry.marker).then_some(entry.marker.as_str());
                (entry.id.as_str(), marker, true)
            }),
            nonce,
            false,
        ))
    }

    async fn delete(&self, id: &str) -> Result<()> {
        // IDs are bridge-generated operation markers, never URLs or arbitrary paths.
        validate_id(id)?;
        let response = self
            .request(Method::DELETE, &format!("sandboxes/{id}"))
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(http_error)?;
        if response.status() != reqwest::StatusCode::NOT_FOUND {
            http::checked(response, PROVIDER)?;
        }
        Ok(())
    }
}

pub(super) fn validate_id(id: &str) -> Result<()> {
    if id.is_empty() || id.len() > 64 || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err(Error::RemoteProviderApi {
            provider: PROVIDER.into(),
            message: "invalid sandbox operation ID".into(),
        });
    }
    Ok(())
}

fn config_error(message: &str) -> Error {
    Error::RemoteProviderConfig {
        provider: PROVIDER.into(),
        message: message.into(),
    }
}
fn http_error(source: reqwest::Error) -> Error {
    http::http_error(PROVIDER, source)
}
fn file_error(source: std::io::Error) -> Error {
    Error::FileAction {
        action: "read_remote_input_archive",
        path: ".struo/tmp".into(),
        source,
    }
}
