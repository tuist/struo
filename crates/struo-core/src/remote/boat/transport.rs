use std::path::Path;
use std::time::Duration;

use serde::Deserialize;
use tokio_util::io::ReaderStream;

use super::client::{api_error, http_error, Client, Sandbox};
use super::shell_word;
use crate::remote::{http, PhasePermit};
use crate::{Error, ExecutionPermit, Result};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Started {
    process_id: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CommandStatus {
    status: String,
    running: bool,
    exit_code: Option<i32>,
}

impl CommandStatus {
    pub(super) fn exit_code(&self) -> Result<i32> {
        if self.running || self.status != "exited" {
            return Err(api_error("command did not exit reliably"));
        }
        self.exit_code
            .ok_or_else(|| api_error("command response omitted its exit code"))
    }

    pub(super) fn ensure_success(&self, operation: &str) -> Result<()> {
        if self.exit_code()? == 0 {
            Ok(())
        } else {
            Err(Error::RemoteProviderApi {
                provider: "boat".into(),
                message: format!("{operation} failed"),
            })
        }
    }
}

#[derive(Deserialize)]
struct FileUrl {
    url: String,
    method: String,
}

impl Client {
    pub(super) async fn execute(
        &self,
        sandbox: &Sandbox,
        command: &str,
        cwd: Option<&str>,
    ) -> Result<CommandStatus> {
        let command = format!(
            "cd {} && {command}",
            shell_word(cwd.unwrap_or("/home/user")),
        );
        let path = Self::sandbox_path(sandbox, "/commands")?;
        let started: Started = self
            .decode(self.json(
                self.request(reqwest::Method::POST, &path),
                &serde_json::json!({ "command": command, "detached": true }),
            )?)
            .await?;
        let status_path = format!("{path}/{}", started.process_id);
        loop {
            let status: CommandStatus = self
                .decode(
                    self.request(reqwest::Method::GET, &status_path)
                        .query(&[("tailBytes", "1")]),
                )
                .await?;
            if !status.running {
                status.exit_code()?;
                return Ok(status);
            }
            if status.status != "running" {
                return Err(api_error("detached command was lost"));
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    async fn file_url(&self, sandbox: &Sandbox, path: &str, upload: bool) -> Result<reqwest::Url> {
        let suffix = if upload {
            "/files/upload-url"
        } else {
            "/files/download-url"
        };
        let response: FileUrl = self
            .decode(self.json(
                self.request(reqwest::Method::POST, &Self::sandbox_path(sandbox, suffix)?),
                &serde_json::json!({ "path": path, "ttlSeconds": 3600 }),
            )?)
            .await?;
        if response.method != if upload { "PUT" } else { "GET" } {
            return Err(api_error("unexpected binary transfer method"));
        }
        let url = reqwest::Url::parse(&response.url)
            .map_err(|_| api_error("invalid binary transfer URL"))?;
        let local_test = reqwest::Url::parse(&self.config.url)
            .is_ok_and(|base| base.scheme() == "http" && base.origin() == url.origin());
        if (url.scheme() != "https" && !local_test)
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err(api_error(
                "binary transfer URL requires HTTPS without user-info or fragment",
            ));
        }
        Ok(url)
    }

    pub(super) async fn upload(
        &self,
        sandbox: &Sandbox,
        source: &Path,
        len: u64,
        destination: &str,
    ) -> Result<()> {
        http::check_size(len, self.limits.max_archive_bytes, "boat", "archive bytes")?;
        let url = self.file_url(sandbox, destination, true).await?;
        let file = tokio::fs::File::open(source).await.map_err(file_error)?;
        let progress = http::Progress::new();
        let body = reqwest::Body::wrap_stream(progress.track_bytes(ReaderStream::new(file), len));
        let response = progress
            .watch_upload("boat", self.limits, async {
                self.http
                    .put(url)
                    .header("Content-Type", "application/octet-stream")
                    .header("Content-Length", len)
                    .body(body)
                    .send()
                    .await
                    .map_err(http_error)
            })
            .await?;
        http::checked(response, "boat").map(|_| ())
    }

    pub(super) async fn download(
        &self,
        sandbox: &Sandbox,
        source: &str,
        destination: &Path,
        lease: Option<&ExecutionPermit>,
        copies: u64,
    ) -> Result<PhasePermit> {
        let url = self.file_url(sandbox, source, false).await?;
        let response = http::Progress::new()
            .watch(
                "boat",
                "download headers",
                self.limits.stall_timeout,
                async { self.http.get(url).send().await.map_err(http_error) },
            )
            .await?;
        if response.status() != reqwest::StatusCode::OK {
            http::checked(response, "boat")?;
            return Err(api_error("binary download did not return a complete file"));
        }
        http::download(response, destination, "boat", self.limits, lease, copies).await
    }
}

fn file_error(source: std::io::Error) -> Error {
    Error::FileAction {
        action: "read_remote_input_archive",
        path: ".struo/tmp".into(),
        source,
    }
}
