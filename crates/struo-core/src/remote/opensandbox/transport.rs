use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use reqwest::{
    multipart::{Form, Part},
    Method,
};
use serde::Deserialize;
use tokio_util::io::ReaderStream;

use super::{
    api_error,
    client::{http_error, segment, Client, Endpoint},
    PROVIDER,
};
use crate::remote::{http, PhasePermit};
use crate::{Error, ExecutionPermit, Result};

impl Client {
    pub async fn upload(
        &self,
        endpoint: &Endpoint,
        source: &Path,
        len: u64,
        path: &str,
    ) -> Result<()> {
        let file = tokio::fs::File::open(source)
            .await
            .map_err(|source| Error::FileAction {
                action: "open_remote_input_archive",
                path: ".struo/tmp".into(),
                source,
            })?;
        let progress = http::Progress::new();
        let body = reqwest::Body::wrap_stream(progress.track_bytes(ReaderStream::new(file), len));
        let metadata = serde_json::to_string(&serde_json::json!({"path": path, "mode": 600}))
            .map_err(|_| api_error("invalid upload metadata"))?;
        let form = Form::new()
            .part(
                "metadata",
                Part::text(metadata)
                    .file_name("metadata")
                    .mime_str("application/json")
                    .map_err(http_error)?,
            )
            .part(
                "file",
                Part::stream_with_length(body, len)
                    .file_name("struo-inputs.tar")
                    .mime_str("application/octet-stream")
                    .map_err(http_error)?,
            );
        let response = progress
            .watch_upload(PROVIDER, self.limits, async {
                self.guest(endpoint, Method::POST, "/files/upload")?
                    .multipart(form)
                    .send()
                    .await
                    .map_err(http_error)
            })
            .await?;
        http::checked(response, PROVIDER).map(|_| ())
    }

    pub async fn download(
        &self,
        endpoint: &Endpoint,
        source: &str,
        destination: &Path,
        lease: Option<&ExecutionPermit>,
        copies: u64,
    ) -> Result<PhasePermit> {
        let response = http::Progress::new()
            .watch(
                PROVIDER,
                "download headers",
                self.limits.stall_timeout,
                async {
                    self.guest(endpoint, Method::GET, "/files/download")?
                        .query(&[("path", source)])
                        .send()
                        .await
                        .map_err(http_error)
                },
            )
            .await?;
        if response.status() != reqwest::StatusCode::OK {
            http::checked(response, PROVIDER)?;
            return Err(api_error("expected a complete binary file download"));
        }
        http::download(response, destination, PROVIDER, self.limits, lease, copies).await
    }

    pub async fn execute(
        &self,
        endpoint: &Endpoint,
        command: &str,
        cwd: Option<&str>,
        envs: &BTreeMap<String, String>,
        timeout: Option<u64>,
    ) -> Result<i32> {
        let body = http::encode(
            &serde_json::json!({
                "command": command, "cwd": cwd, "envs": envs, "background": true,
                "timeout": timeout.unwrap_or(900_000)
            }),
            PROVIDER,
            self.limits.control_bytes(),
        )?;
        let response = self
            .guest(endpoint, Method::POST, "/command")?
            .header("Content-Type", "application/json")
            .body(body)
            .timeout(self.limits.stall_timeout)
            .send()
            .await
            .map_err(http_error)?;
        let body = http::body(
            http::checked(response, PROVIDER)?,
            PROVIDER,
            self.limits.control_bytes(),
            self.limits,
        )
        .await?;
        let id = command_id(&body)?;
        loop {
            let status: CommandStatus = self
                .json(
                    self.guest(
                        endpoint,
                        Method::GET,
                        &format!("/command/status/{}", segment(&id)),
                    )?
                    .timeout(self.limits.stall_timeout),
                )
                .await?;
            if !status.running {
                return status
                    .exit_code
                    .ok_or_else(|| api_error("completed command omitted exit_code"));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

#[derive(Deserialize)]
struct CommandStatus {
    running: bool,
    exit_code: Option<i32>,
}

fn command_id(body: &[u8]) -> Result<String> {
    #[derive(Deserialize)]
    struct Event {
        #[serde(rename = "type")]
        kind: String,
        text: Option<String>,
    }
    let text = std::str::from_utf8(body).map_err(|_| api_error("invalid command event stream"))?;
    let mut id = None;
    let mut acknowledged = false;
    for line in text.lines() {
        let data = line.strip_prefix("data:").unwrap_or(line).trim();
        if data.is_empty() || data.starts_with(':') || data.starts_with("event:") {
            continue;
        }
        let event: Event =
            serde_json::from_str(data).map_err(|_| api_error("invalid command event"))?;
        match event.kind.as_str() {
            "init" => id = event.text.filter(|id| !id.is_empty()),
            "execution_complete" => acknowledged = true,
            "error" => return Err(api_error("command failed to start")),
            _ => {}
        }
    }
    if !acknowledged {
        return Err(api_error(
            "command startup stream ended without acknowledgement",
        ));
    }
    id.ok_or_else(|| api_error("command startup omitted its id"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_acknowledgement_is_not_process_success() {
        assert_eq!(command_id(b"data: {\"type\":\"init\",\"text\":\"cmd-1\"}\n\ndata: {\"type\":\"execution_complete\"}\n\n").unwrap(), "cmd-1");
        assert!(command_id(b"data: {\"type\":\"init\",\"text\":\"cmd-1\"}\n\n").is_err());
        assert!(command_id(
            b"data: {\"type\":\"init\",\"text\":\"cmd-1\"}\n\ndata: {\"type\":\"error\"}\n\n"
        )
        .is_err());
    }
}
