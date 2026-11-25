use std::time::Duration;

use serde::Deserialize;

use super::client::{api_error, Client, Sprite};
use crate::remote::http;
use crate::Result;

#[derive(Deserialize)]
struct Checkpoint {
    id: String,
    comment: Option<String>,
}

impl Client {
    async fn checkpoint_request(
        &self,
        sprite: &Sprite,
        suffix: &str,
        body: serde_json::Value,
    ) -> Result<()> {
        let response = self
            .request(reqwest::Method::POST, self.url(&sprite.name, suffix)?)
            .timeout(Duration::from_secs(120))
            .json(&body)
            .send()
            .await
            .map_err(|error| http::http_error("sprites", error))?;
        let bytes = http::body(
            http::checked(response, "sprites")?,
            "sprites",
            self.limits.control_bytes(),
            self.limits,
        )
        .await?;
        let mut complete = false;
        for line in bytes
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
        {
            let event: serde_json::Value =
                serde_json::from_slice(line).map_err(|_| api_error("invalid checkpoint event"))?;
            tracing::debug!(
                provider = "sprites",
                operation = suffix,
                event_type = event.get("type").and_then(serde_json::Value::as_str),
                "received Sprite checkpoint event"
            );
            match event.get("type").and_then(serde_json::Value::as_str) {
                Some("error") => return Err(api_error("Sprite checkpoint operation failed")),
                Some("complete") => complete = true,
                Some("info" | "keepalive") => {}
                _ => return Err(api_error("unexpected checkpoint event")),
            }
        }
        if !complete {
            return Err(api_error(
                "Sprite checkpoint stream ended without completion",
            ));
        }
        Ok(())
    }

    pub(super) async fn checkpoint(&self, sprite: &Sprite, nonce: &str) -> Result<String> {
        let comment = format!("struo-prepared-{nonce}");
        self.checkpoint_request(
            sprite,
            "checkpoint",
            serde_json::json!({"comment": comment}),
        )
        .await?;
        let response = self
            .request(reqwest::Method::GET, self.url(&sprite.name, "checkpoints")?)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(|error| http::http_error("sprites", error))?;
        let checkpoints: Vec<Checkpoint> = http::decode(
            response,
            "sprites",
            self.limits.control_bytes(),
            self.limits,
        )
        .await?;
        let mut matching = checkpoints
            .into_iter()
            .filter(|checkpoint| checkpoint.comment.as_deref() == Some(&comment));
        let checkpoint = matching
            .next()
            .ok_or_else(|| api_error("prepared checkpoint was not listed"))?;
        if matching.next().is_some()
            || checkpoint.id.is_empty()
            || !checkpoint
                .id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(api_error(
                "ambiguous or invalid prepared checkpoint identity",
            ));
        }
        Ok(checkpoint.id)
    }

    pub(super) async fn restore(&self, sprite: &Sprite, checkpoint: &str) -> Result<()> {
        if checkpoint.is_empty()
            || !checkpoint
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(api_error("invalid prepared checkpoint identity"));
        }
        self.checkpoint_request(
            sprite,
            &format!("checkpoints/{checkpoint}/restore"),
            serde_json::json!({}),
        )
        .await
    }
}
