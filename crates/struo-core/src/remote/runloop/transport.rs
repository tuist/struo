use reqwest::{
    multipart::{Form, Part},
    Method,
};
use serde::Deserialize;
use std::{collections::BTreeMap, path::Path, time::Duration};
use tokio_util::io::ReaderStream;

use super::client::{api_error, http_error, validate_id, Client, Sandbox};
use super::shell_word;
use crate::remote::PhasePermit;
use crate::{Error, ExecutionPermit, Result};

#[derive(Debug, Deserialize)]
pub(super) struct ExecuteResponse {
    execution_id: String,
    devbox_id: String,
    status: String,
    exit_status: Option<i32>,
}

impl ExecuteResponse {
    pub fn exit_code(&self) -> Result<i32> {
        if self.status != "completed" {
            return Err(api_error("execution has not completed".into()));
        }
        self.exit_status
            .ok_or_else(|| api_error("execution did not include an exit status".into()))
    }
}

impl Client {
    pub async fn upload(
        &self,
        sandbox: &Sandbox,
        source: &Path,
        len: u64,
        destination: &str,
    ) -> Result<()> {
        let file = tokio::fs::File::open(source).await.map_err(file_error)?;
        let progress = super::super::http::Progress::new();
        let body = reqwest::Body::wrap_stream(progress.track_bytes(ReaderStream::new(file), len));
        let form = Form::new().text("path", home_path(destination)?).part(
            "file",
            Part::stream_with_length(body, len).file_name("struo-inputs.tar"),
        );
        let response = progress
            .watch_upload("runloop", self.limits, async {
                self.http
                    .post(format!(
                        "{}/devboxes/{}/upload_file",
                        self.config.url.trim_end_matches('/'),
                        sandbox.id
                    ))
                    .bearer_auth(&self.config.key)
                    .multipart(form)
                    .send()
                    .await
                    .map_err(http_error)
            })
            .await?;
        super::super::http::checked(response, "runloop").map(|_| ())
    }

    pub async fn execute(
        &self,
        sandbox: &Sandbox,
        command: &str,
        cwd: Option<&str>,
        env: &BTreeMap<String, String>,
    ) -> Result<ExecuteResponse> {
        let environment = env
            .iter()
            .map(|(key, value)| shell_word(&format!("{key}={value}")))
            .collect::<Vec<_>>()
            .join(" ");
        let command = format!(
            "cd {} && env {} sh -c {}",
            shell_word(cwd.unwrap_or("/home/user")),
            environment,
            shell_word(command)
        );
        let body = super::super::http::encode(
            &serde_json::json!({"command": command}),
            "runloop",
            self.limits.control_bytes(),
        )?;
        let response = self
            .request(
                Method::POST,
                &format!("devboxes/{}/execute_async", sandbox.id),
            )
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .await
            .map_err(http_error)?;
        let mut execution: ExecuteResponse = self.decode(response).await?;
        validate_id(&execution.execution_id)?;
        let id = execution.execution_id.clone();
        loop {
            if execution.devbox_id != sandbox.id || execution.execution_id != id {
                return Err(api_error(
                    "execution response changed resource identity".into(),
                ));
            }
            match execution.status.as_str() {
                "completed" => {
                    execution.exit_code()?;
                    return Ok(execution);
                }
                "queued" | "running" => {}
                status => return Err(api_error(format!("unknown execution status {status}"))),
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
            let response = self
                .request(
                    Method::GET,
                    &format!("devboxes/{}/executions/{id}", sandbox.id),
                )
                .query(&[("last_n", "0")])
                .send()
                .await
                .map_err(http_error)?;
            execution = self.decode(response).await?;
        }
    }

    pub async fn download(
        &self,
        sandbox: &Sandbox,
        source: &str,
        destination: &Path,
        lease: Option<&ExecutionPermit>,
        copies: u64,
    ) -> Result<PhasePermit> {
        let body = super::super::http::encode(
            &serde_json::json!({"path": home_path(source)?}),
            "runloop",
            self.limits.control_bytes(),
        )?;
        let response = super::super::http::Progress::new()
            .watch(
                "runloop",
                "download headers",
                self.limits.stall_timeout,
                async {
                    self.http
                        .post(format!(
                            "{}/devboxes/{}/download_file",
                            self.config.url.trim_end_matches('/'),
                            sandbox.id
                        ))
                        .bearer_auth(&self.config.key)
                        .header("Content-Type", "application/json")
                        .body(body)
                        .send()
                        .await
                        .map_err(http_error)
                },
            )
            .await?;
        let response = super::super::http::checked(response, "runloop")?;
        if response.status() != reqwest::StatusCode::OK
            || response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| {
                    value
                        .split(';')
                        .next()
                        .is_some_and(|v| v.trim() == "application/json")
                })
        {
            return Err(api_error("expected complete binary download".into()));
        }
        super::super::http::download(response, destination, "runloop", self.limits, lease, copies)
            .await
    }
}

fn file_error(source: std::io::Error) -> Error {
    Error::FileAction {
        action: "open_remote_input_archive",
        path: ".struo/tmp".into(),
        source,
    }
}

fn home_path(path: &str) -> Result<String> {
    path.strip_prefix("/home/user/")
        .map(str::to_string)
        .ok_or_else(|| api_error("file transfer path must be relative to the devbox home".into()))
}
