use std::time::Duration;

use serde::Deserialize;

use crate::remote::http;
use crate::remote::lifecycle::{self, HostedSandbox, Lookup, Provider};
use crate::{Error, RemoteIoLimits, Result};

#[derive(Clone)]
pub(super) struct Client {
    pub http: reqwest::Client,
    pub endpoint: String,
    pub token: String,
    pub limits: RemoteIoLimits,
}

#[derive(Deserialize)]
pub(super) struct Sprite {
    pub name: String,
    #[serde(rename = "id")]
    pub provider_id: String,
    #[serde(skip)]
    ownership: String,
}

impl HostedSandbox for Sprite {
    fn id(&self) -> &str {
        &self.ownership
    }
}

impl Client {
    pub fn from_env() -> Result<Self> {
        let token = std::env::var("STRUO_SPRITES_TOKEN")
            .or_else(|_| std::env::var("SPRITES_TOKEN"))
            .map_err(|_| Error::RemoteProviderConfig {
                provider: "sprites".into(),
                message: "set STRUO_SPRITES_TOKEN or SPRITES_TOKEN".into(),
            })?;
        Self::new(
            &std::env::var("STRUO_SPRITES_API_URL")
                .unwrap_or_else(|_| "https://api.sprites.dev".into()),
            token,
        )
    }

    pub fn new(endpoint: &str, token: String) -> Result<Self> {
        let http = Self::http_builder()
            .build()
            .map_err(|error| http::http_error("sprites", error))?;
        Ok(Self {
            http,
            endpoint: endpoint.trim_end_matches('/').into(),
            token,
            limits: RemoteIoLimits::default(),
        })
    }

    pub fn http_builder() -> reqwest::ClientBuilder {
        reqwest::Client::builder()
            .http1_only()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(30))
            .pool_max_idle_per_host(0)
    }

    pub fn url(&self, name: &str, suffix: &str) -> Result<reqwest::Url> {
        let mut url =
            reqwest::Url::parse(&self.endpoint).map_err(|_| api_error("invalid API URL"))?;
        url.path_segments_mut()
            .map_err(|()| api_error("invalid API URL"))?
            .pop_if_empty()
            .extend(["v1", "sprites", name]);
        if !suffix.is_empty() {
            url.path_segments_mut().unwrap().extend(suffix.split('/'));
        }
        Ok(url)
    }

    pub fn request(&self, method: reqwest::Method, url: reqwest::Url) -> reqwest::RequestBuilder {
        self.http.request(method, url).bearer_auth(&self.token)
    }

    pub async fn create(&self, nonce: &str) -> Result<Sprite> {
        let name = format!("struo-{nonce}");
        let response = self
            .http
            .post(format!("{}/v1/sprites", self.endpoint))
            .bearer_auth(&self.token)
            .timeout(Duration::from_secs(60))
            .header("Content-Type", "application/json")
            .body(http::encode(
                &serde_json::json!({"name": name, "url_settings": {"auth": "sprite"}}),
                "sprites",
                self.limits.control_bytes(),
            )?)
            .send()
            .await
            .map_err(|error| http::http_error("sprites", error))?;
        let mut sprite: Sprite = http::decode(
            response,
            "sprites",
            self.limits.control_bytes(),
            self.limits,
        )
        .await?;
        if sprite.name != name {
            return Err(api_error("create returned a different Sprite name"));
        }
        if sprite.provider_id.is_empty() {
            return Err(api_error("create returned an empty Sprite id"));
        }
        sprite.ownership = serde_json::to_string(&(&sprite.name, &sprite.provider_id)).unwrap();
        tracing::debug!(provider = "sprites", sprite = %sprite.name, "created remote sandbox");
        Ok(sprite)
    }
}

impl Provider for Client {
    const NAME: &'static str = "sprites";

    fn scope(&self) -> String {
        lifecycle::scope("sprites", &self.endpoint)
    }

    fn credential(&self) -> String {
        lifecycle::credential("sprites", &self.token)
    }

    async fn find(&self, nonce: &str) -> Result<Lookup> {
        self.find_owned(nonce, &[], &self.credential()).await
    }

    async fn find_owned(&self, nonce: &str, ids: &[String], credential: &str) -> Result<Lookup> {
        if credential != self.credential() {
            return Ok(Lookup::Inconclusive);
        }
        let name = format!("struo-{nonce}");
        let response = self
            .request(reqwest::Method::GET, self.url(&name, "")?)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(|error| http::http_error("sprites", error))?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(Lookup::Absent);
        }
        let sprite: Sprite = http::decode(
            response,
            "sprites",
            self.limits.control_bytes(),
            self.limits,
        )
        .await?;
        let identity = serde_json::to_string(&(&sprite.name, &sprite.provider_id)).unwrap();
        Ok(if sprite.name == name && ids.contains(&identity) {
            Lookup::Found(vec![identity])
        } else {
            Lookup::Inconclusive
        })
    }

    async fn delete(&self, identity: &str) -> Result<()> {
        let (name, id): (String, String) = serde_json::from_str(identity)
            .map_err(|_| api_error("Sprite ownership identity is unavailable"))?;
        let response = self
            .request(reqwest::Method::GET, self.url(&name, "")?)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(|error| http::http_error("sprites", error))?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(());
        }
        let sprite: Sprite = http::decode(
            response,
            "sprites",
            self.limits.control_bytes(),
            self.limits,
        )
        .await?;
        if sprite.name != name || sprite.provider_id != id {
            return Err(api_error("Sprite identity changed; refusing deletion"));
        }
        let response = self
            .request(reqwest::Method::DELETE, self.url(&name, "")?)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(|error| http::http_error("sprites", error))?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(());
        }
        http::checked(response, "sprites")?;
        Ok(())
    }
}

pub(super) fn api_error(message: &str) -> Error {
    Error::RemoteProviderApi {
        provider: "sprites".into(),
        message: message.into(),
    }
}
