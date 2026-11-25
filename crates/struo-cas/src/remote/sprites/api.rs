use std::time::Duration;

use reqwest::{Method, Response, StatusCode, Url};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::Value;

use super::{config, protocol, PROVIDER};
use crate::{Error, ErrorKind, Result};

const CONTROL_LIMIT: usize = 64 * 1024;

pub(super) struct Api {
    client: reqwest::Client,
    endpoint: Url,
    token: String,
}

#[derive(Deserialize)]
pub(super) struct Sprite {
    pub id: String,
    pub name: String,
    pub url: String,
    pub url_settings: Value,
}

impl Api {
    pub fn new(endpoint: &str, token: String) -> Result<Self> {
        if token.trim().is_empty() {
            return Err(config(
                "set STRUO_SPRITES_TOKEN or SPRITES_TOKEN to a nonempty token",
            ));
        }
        let endpoint = super::super::http::base_url(endpoint, true).map_err(|_| {
            config("Sprites API URL must use HTTPS or loopback HTTP without credentials, a query, or a fragment")
        })?;
        let client = reqwest::Client::builder()
            .http1_only()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|_| config("could not create Sprites API client"))?;
        Ok(Self {
            client,
            endpoint,
            token,
        })
    }

    pub fn scope(&self) -> String {
        let mut hash = blake3::Hasher::new();
        hash.update(self.endpoint.as_str().as_bytes());
        hash.update(&[0]);
        hash.update(self.token.as_bytes());
        hash.finalize().to_hex().to_string()
    }

    pub fn request(&self, method: Method, parts: &[&str]) -> reqwest::RequestBuilder {
        let mut url = self.endpoint.clone();
        url.path_segments_mut()
            .unwrap()
            .pop_if_empty()
            .extend(parts);
        self.client.request(method, url).bearer_auth(&self.token)
    }

    pub async fn send(&self, request: reqwest::RequestBuilder) -> Result<Response> {
        request.send().await.map_err(transport)
    }

    pub async fn find(&self, name: &str) -> Result<Option<Sprite>> {
        let response = self
            .send(self.request(Method::GET, &["v1", "sprites", name]))
            .await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        decode(response).await.map(Some)
    }

    pub async fn create(&self, name: &str) -> Result<Sprite> {
        let response = self
            .send(
                self.request(Method::POST, &["v1", "sprites"])
                    .json(&serde_json::json!({"name": name, "url_settings": {"auth": "sprite"}})),
            )
            .await?;
        decode(response).await
    }

    pub async fn service(&self, name: &str) -> Result<Option<Value>> {
        let response = self
            .send(self.request(
                Method::GET,
                &["v1", "sprites", name, "services", super::SERVICE],
            ))
            .await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        decode(response).await.map(Some)
    }

    pub async fn install_file(&self, name: &str, path: &str, body: Vec<u8>) -> Result<()> {
        let temporary = format!("{path}.staging");
        let response = self
            .send(
                self.request(Method::PUT, &["v1", "sprites", name, "fs", "write"])
                    .query(&[
                        ("path", temporary.as_str()),
                        ("workingDir", "/"),
                        ("mode", "0600"),
                        ("mkdir", "true"),
                    ])
                    .header("content-type", "application/octet-stream")
                    .body(body),
            )
            .await?;
        bounded(response).await?;
        let response = self.send(self.request(Method::POST,
            &["v1", "sprites", name, "fs", "rename"])
            .json(&serde_json::json!({"source": temporary, "dest": path, "workingDir": "/", "asRoot": false}))).await?;
        bounded(response).await?;
        Ok(())
    }

    pub async fn configure(&self, name: &str, definition: &Value, restart: bool) -> Result<()> {
        let parts = ["v1", "sprites", name, "services", super::SERVICE];
        let response = self
            .send(
                self.request(Method::PUT, &parts)
                    .query(&[("duration", "1")])
                    .json(definition),
            )
            .await?;
        bounded(response).await?;
        if restart {
            let response = self
                .send(
                    self.request(
                        Method::POST,
                        &["v1", "sprites", name, "services", super::SERVICE, "restart"],
                    )
                    .query(&[("duration", "1")]),
                )
                .await?;
            bounded(response).await?;
        }
        Ok(())
    }

    pub async fn publish(&self, name: &str) -> Result<()> {
        let response = self
            .send(
                self.request(Method::PUT, &["v1", "sprites", name])
                    .json(&serde_json::json!({"url_settings": {"auth": "public"}})),
            )
            .await?;
        bounded(response).await?;
        Ok(())
    }
}

async fn decode<T: DeserializeOwned>(response: Response) -> Result<T> {
    serde_json::from_slice(&bounded(response).await?)
        .map_err(|_| protocol("invalid Sprites API response"))
}

async fn bounded(mut response: Response) -> Result<Vec<u8>> {
    let status = response.status();
    if !status.is_success() {
        let kind = match status {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => ErrorKind::Refused,
            StatusCode::REQUEST_TIMEOUT | StatusCode::TOO_MANY_REQUESTS => ErrorKind::Unavailable,
            status if status.is_server_error() => ErrorKind::Unavailable,
            _ => ErrorKind::Protocol,
        };
        return Err(Error::remote(
            PROVIDER,
            "provision",
            kind,
            format!("HTTP {}", status.as_u16()),
        ));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(transport)? {
        if chunk.len() > CONTROL_LIMIT.saturating_sub(body.len()) {
            return Err(protocol("Sprites API response exceeds 64 KiB"));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn transport(error: reqwest::Error) -> Error {
    Error::remote(
        PROVIDER,
        "provision",
        ErrorKind::Unavailable,
        error.without_url().to_string(),
    )
}
