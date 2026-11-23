use std::collections::BTreeMap;
use std::path::Path;

use base64::{engine::general_purpose::STANDARD, Engine};
use futures::StreamExt;
use tokio::io::AsyncWriteExt;

use super::client::{api_error, http_error, Client, Sandbox};
use super::transfer::file_error;
use crate::remote::{archive::output_archive_file, http};
use crate::Result;

mod parser;

pub(super) struct Capture {
    pub exit_code: i32,
    pub stdout: tempfile::NamedTempFile,
    pub stderr: tempfile::NamedTempFile,
}

impl Capture {
    pub(super) fn ensure_success(&self) -> Result<()> {
        if self.exit_code == 0 {
            Ok(())
        } else {
            Err(api_error("sandbox preparation command failed"))
        }
    }
}

impl Client {
    pub(super) async fn execute(
        &self,
        sandbox: &Sandbox,
        argv: &[String],
        cwd: &str,
        workspace: &Path,
    ) -> Result<Capture> {
        self.execute_with_env(sandbox, argv, &BTreeMap::new(), cwd, workspace)
            .await
    }

    #[allow(clippy::too_many_lines)]
    pub(super) async fn execute_with_env(
        &self,
        sandbox: &Sandbox,
        argv: &[String],
        env: &BTreeMap<String, String>,
        cwd: &str,
        workspace: &Path,
    ) -> Result<Capture> {
        let stdout = output_archive_file(workspace)?;
        let stderr = output_archive_file(workspace)?;
        let mut files = [
            tokio::fs::File::create(stdout.path())
                .await
                .map_err(file_error)?,
            tokio::fs::File::create(stderr.path())
                .await
                .map_err(file_error)?,
        ];
        let command_id = uuid::Uuid::now_v7().to_string();
        let mut offsets = [0_u64; 2];
        let mut resumes = 0;
        let mut started = false;
        loop {
            let suffix = if started || resumes > 0 {
                "/execute/stream/resume"
            } else {
                "/execute/stream/start"
            };
            let body = if suffix.ends_with("start") {
                serde_json::json!({"command": argv, "command_id": command_id, "run_config": {"work_dir": cwd, "env_vars": env}, "timeout_seconds": 0, "idle_timeout_seconds": -1})
            } else {
                serde_json::json!({"command_id": command_id, "stdout_offset": offsets[0], "stderr_offset": offsets[1]})
            };
            let request =
                self.json(self.runtime(sandbox, reqwest::Method::POST, suffix)?, &body)?;
            let response = http::Progress::new()
                .watch(
                    "langsmith",
                    "command headers",
                    self.limits.stall_timeout,
                    async { request.send().await.map_err(http_error) },
                )
                .await;
            let Ok(response) = response else {
                retry_stream(&mut resumes)?;
                continue;
            };
            if response.status().is_server_error()
                || matches!(response.status().as_u16(), 408 | 429)
            {
                retry_stream(&mut resumes)?;
                continue;
            }
            let response = http::checked(response, "langsmith")?;
            if response
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .is_none_or(|v| v.split(';').next() != Some("text/event-stream"))
            {
                return Err(api_error("command response is not an SSE stream"));
            }
            let mut stream = response.bytes_stream();
            let mut parser = parser::Parser::default();
            let mut acknowledge = false;
            'stream: while let Ok(Some(Ok(chunk))) =
                tokio::time::timeout(self.limits.stall_timeout, stream.next()).await
            {
                for byte in chunk {
                    let Some((event, payload)) =
                        parser.push(byte, self.limits.max_response_bytes)?
                    else {
                        continue;
                    };
                    match event.as_str() {
                        "started" => {
                            if payload.get("command_id").and_then(|v| v.as_str())
                                != Some(&command_id)
                            {
                                return Err(api_error("command identity mismatch"));
                            }
                            started = true;
                        }
                        "stdout" | "stderr" => {
                            if !started {
                                return Err(api_error("output before command identity"));
                            }
                            let index = usize::from(event == "stderr");
                            let offset = payload
                                .get("offset")
                                .and_then(serde_json::Value::as_u64)
                                .ok_or_else(|| api_error("output omitted offset"))?;
                            if offset != offsets[index] {
                                return Err(api_error("command output offset gap or overlap"));
                            }
                            let data = STANDARD
                                .decode(
                                    payload
                                        .get("data")
                                        .and_then(|v| v.as_str())
                                        .ok_or_else(|| api_error("output omitted data"))?,
                                )
                                .map_err(|_| api_error("invalid base64 command output"))?;
                            http::check_size(
                                offsets[0]
                                    .saturating_add(offsets[1])
                                    .saturating_add(data.len() as u64),
                                self.limits.max_capture_bytes,
                                "langsmith",
                                "capture bytes",
                            )?;
                            files[index].write_all(&data).await.map_err(file_error)?;
                            offsets[index] += data.len() as u64;
                        }
                        "stream_end" => {
                            let index = match payload.get("stream").and_then(|v| v.as_str()) {
                                Some("stdout") => 0,
                                Some("stderr") => 1,
                                _ => return Err(api_error("invalid stream end")),
                            };
                            if payload.get("offset").and_then(serde_json::Value::as_u64)
                                != Some(offsets[index])
                            {
                                return Err(api_error("stream ended with missing bytes"));
                            }
                        }
                        "ack_required" => {
                            if !started
                                || payload
                                    .get("stdout_offset")
                                    .and_then(serde_json::Value::as_u64)
                                    != Some(offsets[0])
                                || payload
                                    .get("stderr_offset")
                                    .and_then(serde_json::Value::as_u64)
                                    != Some(offsets[1])
                            {
                                return Err(api_error("acknowledgement offset mismatch"));
                            }
                            acknowledge = true;
                            break;
                        }
                        "exit" => {
                            if !started {
                                return Err(api_error("exit before command identity"));
                            }
                            let exit_code = payload
                                .get("exit_code")
                                .and_then(serde_json::Value::as_i64)
                                .and_then(|v| i32::try_from(v).ok())
                                .ok_or_else(|| api_error("command omitted exit code"))?;
                            for file in &mut files {
                                file.flush().await.map_err(file_error)?;
                            }
                            return Ok(Capture {
                                exit_code,
                                stdout,
                                stderr,
                            });
                        }
                        "error" => {
                            if payload.get("error_type").and_then(|v| v.as_str())
                                == Some("ServerShuttingDown")
                            {
                                break 'stream;
                            }
                            return Err(api_error("sandbox command stream reported an error"));
                        }
                        _ => return Err(api_error("unknown command stream event")),
                    }
                }
                if acknowledge {
                    break;
                }
            }
            if !acknowledge {
                retry_stream(&mut resumes)?;
            }
            // Resume only attaches, including when the start response was lost.
            started = true;
        }
    }
}

fn retry_stream(resumes: &mut u32) -> Result<()> {
    *resumes += 1;
    if *resumes > 3 {
        return Err(api_error("command stream ended without a valid exit"));
    }
    Ok(())
}
