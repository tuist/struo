use std::path::Path;
use std::time::Duration;

use serde::Deserialize;
use tokio_util::io::ReaderStream;

use super::client::{api_error, Client, Sandbox};
use crate::remote::{http, PhasePermit};
use crate::{Error, ExecutionPermit, Result};

#[derive(Deserialize)]
struct Process {
    pid: u32,
    status: String,
    exit_code: Option<i32>,
}

impl Client {
    pub fn guest(
        &self,
        sandbox: &Sandbox,
        origin: &reqwest::Url,
        method: reqwest::Method,
        path: &str,
    ) -> Result<reqwest::RequestBuilder> {
        let url = origin
            .join(path)
            .map_err(|_| api_error("invalid guest path"))?;
        Ok(self
            .http
            .request(method, url)
            .bearer_auth(&self.token)
            .header("X-Tensorlake-Sandbox-Id", &sandbox.provider_id))
    }

    pub async fn process(
        &self,
        sandbox: &Sandbox,
        origin: &reqwest::Url,
        argv: &[String],
        cwd: Option<&str>,
    ) -> Result<i32> {
        let (command, arguments) = argv.split_first().ok_or(Error::EmptyArgv)?;
        let mut body = serde_json::json!({
            "command": command, "args": arguments, "env": {}, "stdin_mode": "closed",
            "stdout_mode": "discard", "stderr_mode": "discard"
        });
        if let Some(cwd) = cwd {
            body["working_dir"] = cwd.into();
        }
        let mut process: Process = self
            .decode(
                self.json(
                    self.guest(sandbox, origin, reqwest::Method::POST, "/api/v1/processes")?
                        .timeout(Duration::from_secs(30)),
                    &body,
                )?,
            )
            .await?;
        if process.pid == 0 {
            return Err(api_error("process start omitted a valid PID"));
        }
        let pid = process.pid;
        loop {
            match process.status.as_str() {
                "running" => {}
                "exited" => {
                    return process
                        .exit_code
                        .ok_or_else(|| api_error("exited process omitted exit code"))
                }
                _ => {
                    return Err(api_error(
                        "remote process was signaled, OOM-killed, or lost",
                    ))
                }
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
            process = self
                .decode(
                    self.guest(
                        sandbox,
                        origin,
                        reqwest::Method::GET,
                        &format!("/api/v1/processes/{pid}"),
                    )?
                    .timeout(Duration::from_secs(30)),
                )
                .await?;
            if process.pid != pid {
                return Err(api_error("process polling returned a different PID"));
            }
        }
    }

    pub async fn setup(
        &self,
        sandbox: &Sandbox,
        origin: &reqwest::Url,
        argv: &[String],
    ) -> Result<()> {
        if self.process(sandbox, origin, argv, None).await? != 0 {
            return Err(api_error("remote filesystem setup failed"));
        }
        Ok(())
    }

    pub async fn upload(
        &self,
        sandbox: &Sandbox,
        origin: &reqwest::Url,
        source: &Path,
        len: u64,
        destination: &str,
    ) -> Result<()> {
        http::check_size(
            len,
            self.limits.max_archive_bytes,
            "tensorlake",
            "archive bytes",
        )?;
        let file = tokio::fs::File::open(source).await.map_err(file_error)?;
        let progress = http::Progress::new();
        let body = reqwest::Body::wrap_stream(progress.track_bytes(ReaderStream::new(file), len));
        let response = progress
            .watch_upload("tensorlake", self.limits, async {
                self.guest(sandbox, origin, reqwest::Method::PUT, "/api/v1/files")?
                    .query(&[("path", destination)])
                    .header("Content-Type", "application/octet-stream")
                    .header("Content-Length", len)
                    .body(body)
                    .send()
                    .await
                    .map_err(|error| http::http_error("tensorlake", error))
            })
            .await?;
        http::checked(response, "tensorlake").map(|_| ())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn download(
        &self,
        sandbox: &Sandbox,
        origin: &reqwest::Url,
        source: &str,
        destination: &Path,
        lease: Option<&ExecutionPermit>,
        copies: u64,
    ) -> Result<PhasePermit> {
        let response = http::Progress::new()
            .watch(
                "tensorlake",
                "download headers",
                self.limits.stall_timeout,
                async {
                    self.guest(sandbox, origin, reqwest::Method::GET, "/api/v1/files")?
                        .query(&[("path", source)])
                        .send()
                        .await
                        .map_err(|error| http::http_error("tensorlake", error))
                },
            )
            .await?;
        if response.status() != reqwest::StatusCode::OK {
            http::checked(response, "tensorlake")?;
            return Err(api_error("download did not return a complete file"));
        }
        http::download(
            response,
            destination,
            "tensorlake",
            self.limits,
            lease,
            copies,
        )
        .await
    }
}

pub(super) fn file_error(source: std::io::Error) -> Error {
    Error::FileAction {
        action: "remote_transfer",
        path: ".struo/tmp".into(),
        source,
    }
}
