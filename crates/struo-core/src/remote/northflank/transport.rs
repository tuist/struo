use std::path::Path;

use base64::prelude::*;
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::tungstenite::{protocol::WebSocketConfig, Message};

use super::client::{api_error, Client};
use crate::{Error, Result};

#[derive(Deserialize)]
struct Frame {
    #[serde(rename = "type")]
    kind: String,
    data: Value,
}

impl Client {
    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    pub(super) async fn exec(
        &self,
        service: &str,
        argv: &[String],
        input: Option<&Path>,
        stdout: &Path,
        stderr: &Path,
        output_limit: u64,
        capture: Option<&crate::executor::staging::CapturePermit>,
    ) -> Result<i32> {
        let mut url = self.url(true, Some(service));
        let scheme = if url.scheme() == "https" { "wss" } else { "ws" };
        url.set_scheme(scheme).expect("websocket scheme");
        let frame_limit = usize::try_from(self.limits.max_response_bytes).unwrap_or(usize::MAX);
        let config = WebSocketConfig::default()
            .max_message_size(Some(frame_limit))
            .max_frame_size(Some(frame_limit));
        let (socket, _) = bounded(
            self,
            tokio_tungstenite::connect_async_with_config(url.as_str(), Some(config), false),
        )
        .await?
        .map_err(|_| api_error("exec WebSocket connection failed"))?;
        let (mut writer, mut reader) = socket.split();
        let init = json!({
            "type": "init",
            "data": {
                "auth": { "type": "apiToken", "apiToken": self.token },
                "context": { "command": argv, "shell": "none", "tty": false, "encoding": "base64" }
            }
        });
        let init = super::super::http::encode(&init, "northflank", self.limits.control_bytes())?;
        bounded(
            self,
            writer.send(Message::Text(
                String::from_utf8(init).expect("JSON is UTF-8").into(),
            )),
        )
        .await?
        .map_err(|_| api_error("exec initialization failed"))?;

        let (authenticated, ready) = tokio::sync::oneshot::channel();
        let (progress, mut uploaded) = tokio::sync::watch::channel(());
        let upload = async move {
            ready
                .await
                .map_err(|_| api_error("exec authentication failed"))?;
            if let Some(input) = input {
                let mut file = tokio::fs::File::open(input).await.map_err(file_error)?;
                let mut buffer = vec![0; (frame_limit / 4).clamp(1, 16 * 1024)];
                let mut size = 0_u64;
                loop {
                    let count = file.read(&mut buffer).await.map_err(file_error)?;
                    if count == 0 {
                        break;
                    }
                    size = size.saturating_add(count as u64);
                    super::super::http::check_size(
                        size,
                        self.limits.max_archive_bytes,
                        "northflank",
                        "archive bytes",
                    )?;
                    let packet = json!({ "type": "payload", "data": BASE64_STANDARD.encode(&buffer[..count]) });
                    bounded(self, writer.send(Message::Text(packet.to_string().into())))
                        .await?
                        .map_err(|_| api_error("exec upload failed"))?;
                    progress.send_replace(());
                }
            }
            bounded(
                self,
                writer.send(Message::Text(
                    json!({ "type": "stdInEnd" }).to_string().into(),
                )),
            )
            .await?
            .map_err(|_| api_error("exec stdin close failed"))?;
            drop(progress);
            Ok::<_, Error>(())
        };
        let download = async {
            let mut stdout = tokio::fs::File::create(stdout).await.map_err(file_error)?;
            let mut stderr = tokio::fs::File::create(stderr).await.map_err(file_error)?;
            let mut authenticated = Some(authenticated);
            let mut completion = None;
            let mut size = 0_u64;
            let mut uploading = input.is_some();
            loop {
                // Quiet test commands may run until the action deadline, but
                // transfers and post-completion draining must make progress.
                let timeout = if completion.is_some() || authenticated.is_some() || input.is_some()
                {
                    Some(self.limits.stall_timeout)
                } else {
                    self.limits.command_idle_timeout
                };
                let next = tokio::select! {
                    progress = uploaded.changed(), if uploading => {
                        if progress.is_err() {
                            uploading = false;
                        }
                        continue;
                    }
                    next = async {
                        match timeout {
                            Some(timeout) => tokio::time::timeout(timeout, reader.next())
                                .await
                                .map_err(|_| stalled(self)),
                            None => Ok(reader.next().await),
                        }
                    } => next?,
                };
                let message = next
                    .ok_or_else(|| api_error("exec stream ended without a close frame"))?
                    .map_err(|_| api_error("exec WebSocket stream failed"))?;
                let payload = match message {
                    Message::Text(text) => text.as_bytes().to_vec(),
                    Message::Binary(bytes) => bytes.to_vec(),
                    Message::Close(close) => {
                        if close.is_some_and(|close| !matches!(u16::from(close.code), 1000 | 1005))
                        {
                            return Err(api_error("exec WebSocket closed abnormally"));
                        }
                        break;
                    }
                    Message::Ping(_) | Message::Pong(_) => continue,
                    Message::Frame(_) => return Err(api_error("unexpected WebSocket frame")),
                };
                let frame: Frame = serde_json::from_slice(&payload)
                    .map_err(|_| api_error("invalid exec protocol frame"))?;
                match frame.kind.as_str() {
                    "init" if authenticated.is_some() && frame.data["auth"] == "successful" => {
                        let _ = authenticated.take().expect("checked sender").send(());
                    }
                    "completion" if authenticated.is_none() && completion.is_none() => {
                        let code = frame.data["exitCode"]
                            .as_i64()
                            .and_then(|value| i32::try_from(value).ok())
                            .filter(|_| {
                                matches!(frame.data["status"].as_str(), Some("Success" | "Failure"))
                            })
                            .ok_or_else(|| {
                                api_error("exec completion has no reliable exit status")
                            })?;
                        completion = Some(code);
                    }
                    "stdOut" | "stdErr" if authenticated.is_none() => {
                        let encoded = frame
                            .data
                            .as_str()
                            .ok_or_else(|| api_error("invalid exec output frame"))?;
                        let bytes = BASE64_STANDARD
                            .decode(encoded)
                            .map_err(|_| api_error("invalid binary exec output"))?;
                        size = size.saturating_add(bytes.len() as u64);
                        super::super::http::check_size(
                            size,
                            output_limit,
                            "northflank",
                            "captured bytes",
                        )?;
                        if let Some(capture) = capture {
                            capture.reserve(bytes.len() as u64)?;
                        }
                        let file = if frame.kind == "stdOut" {
                            &mut stdout
                        } else {
                            &mut stderr
                        };
                        file.write_all(&bytes).await.map_err(file_error)?;
                    }
                    _ => return Err(api_error("exec authentication or protocol failure")),
                }
            }
            stdout.flush().await.map_err(file_error)?;
            stderr.flush().await.map_err(file_error)?;
            completion.ok_or_else(|| api_error("exec closed before command completion"))
        };
        let ((), exit_code) = tokio::try_join!(upload, download)?;
        Ok(exit_code)
    }
}

async fn bounded<T>(client: &Client, future: impl std::future::Future<Output = T>) -> Result<T> {
    tokio::time::timeout(client.limits.stall_timeout, future)
        .await
        .map_err(|_| stalled(client))
}

fn stalled(client: &Client) -> Error {
    Error::ExecutorStalled {
        executor: "northflank".into(),
        phase: "exec stream",
        timeout: client.limits.stall_timeout,
    }
}

fn file_error(source: std::io::Error) -> Error {
    Error::FileAction {
        action: "northflank_transfer",
        path: ".struo/tmp".into(),
        source,
    }
}
