use std::time::Duration;

use serde::Deserialize;

use super::client::{api_error, Client, Identity, Info, Sandbox};
use crate::remote::http;
use crate::{RemoteExecution, Result};

impl Client {
    pub async fn create(&self, remote: &RemoteExecution, nonce: &str) -> Result<Sandbox> {
        #[derive(Deserialize)]
        struct Accepted {
            sandbox_id: String,
        }
        let name = format!("struo-{nonce}");
        let mut body = serde_json::json!({
            "name": name, "wait": false, "max_pending_secs": 1800,
            "timeout_secs": 3600, "auto_resume": false,
            "allow_unauthenticated_access": false, "exposed_ports": [],
            "secret_names": [], "file_systems": []
        });
        let kind = remote
            .properties
            .get("environment_kind")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("image");
        body[if kind == "snapshot" {
            "snapshot_id"
        } else {
            "image"
        }] = remote.environment.clone().into();
        let accepted: Accepted = self
            .decode(self.json(self.control(reqwest::Method::POST, None)?, &body)?)
            .await?;
        if accepted.sandbox_id.is_empty() {
            return Err(api_error("create omitted sandbox ID"));
        }
        let identity = Identity {
            id: accepted.sandbox_id.clone(),
            name,
            namespace: self.namespace.clone(),
        };
        Ok(Sandbox {
            identity: serde_json::to_string(&identity).unwrap(),
            provider_id: accepted.sandbox_id,
        })
    }

    pub async fn info(&self, id: &str) -> Result<Option<Info>> {
        let response = self
            .control(reqwest::Method::GET, Some(id))?
            .send()
            .await
            .map_err(|error| http::http_error("tensorlake", error))?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        http::decode(
            response,
            "tensorlake",
            self.limits.control_bytes(),
            self.limits,
        )
        .await
        .map(Some)
    }

    pub async fn ready(&self, sandbox: &Sandbox) -> Result<reqwest::Url> {
        let identity: Identity = serde_json::from_str(&sandbox.identity)
            .map_err(|_| api_error("invalid ownership identity"))?;
        loop {
            let info = self
                .info(&sandbox.provider_id)
                .await?
                .ok_or_else(|| api_error("sandbox disappeared before readiness"))?;
            self.verify(&identity, &info)?;
            match info.status.as_str() {
                "pending" => tokio::time::sleep(Duration::from_millis(250)).await,
                "running" => {
                    if info.allow_unauthenticated_access
                        || info.auto_resume
                        || info.exposed_ports.is_some_and(|ports| !ports.is_empty())
                    {
                        return Err(api_error(
                            "sandbox did not retain private ingress and disabled auto-resume",
                        ));
                    }
                    return self.guest_url(
                        info.sandbox_url
                            .as_deref()
                            .ok_or_else(|| api_error("running sandbox omitted sandbox_url"))?,
                    );
                }
                _ => return Err(api_error("sandbox failed to reach running state")),
            }
        }
    }
}
