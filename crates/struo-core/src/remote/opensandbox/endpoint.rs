use std::collections::BTreeMap;
use std::time::Duration;

use reqwest::{header::HeaderMap, Method, RequestBuilder};
use serde::Deserialize;

use super::{
    api_error,
    client::{http_error, segment, valid_url, Client, Sandbox},
    PROVIDER,
};
use crate::remote::http;
use crate::Result;

#[derive(Deserialize)]
struct State {
    state: String,
}

#[derive(Deserialize)]
struct Status {
    status: State,
}

#[derive(Deserialize)]
pub(super) struct Endpoint {
    pub endpoint: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
}

impl Client {
    pub async fn ready(&self, sandbox: &Sandbox) -> Result<Endpoint> {
        loop {
            let status: Status = self
                .json(self.control(Method::GET, &format!("/sandboxes/{}", segment(&sandbox.id))))
                .await?;
            match status.status.state.as_str() {
                "Running" => break,
                "Pending" | "Allocated" => tokio::time::sleep(Duration::from_millis(250)).await,
                _ => return Err(api_error("sandbox did not reach Running state")),
            }
        }
        let mut endpoint: Endpoint = self
            .json(self.control(
                Method::GET,
                &format!("/sandboxes/{}/endpoints/44772", segment(&sandbox.id)),
            ))
            .await?;
        if !endpoint.endpoint.contains("://") {
            endpoint.endpoint = format!("{}://{}", self.base.scheme(), endpoint.endpoint);
        }
        endpoint.endpoint = valid_url(&endpoint.endpoint)?.to_string();
        loop {
            let response = self
                .guest(&endpoint, Method::GET, "/ping")?
                .timeout(self.limits.stall_timeout)
                .send()
                .await
                .map_err(http_error)?;
            if response.status().is_success() {
                return Ok(endpoint);
            }
            if !matches!(response.status().as_u16(), 502..=504) {
                http::checked(response, PROVIDER)?;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    pub fn guest(&self, endpoint: &Endpoint, method: Method, path: &str) -> Result<RequestBuilder> {
        let mut headers = HeaderMap::new();
        for (name, value) in &endpoint.headers {
            headers.insert(
                reqwest::header::HeaderName::from_bytes(name.as_bytes())
                    .map_err(|_| api_error("invalid endpoint access header name"))?,
                reqwest::header::HeaderValue::from_str(value)
                    .map_err(|_| api_error("invalid endpoint access header value"))?,
            );
        }
        Ok(self
            .http
            .request(
                method,
                format!("{}{path}", endpoint.endpoint.trim_end_matches('/')),
            )
            .headers(headers))
    }
}
