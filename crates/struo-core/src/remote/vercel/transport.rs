use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use reqwest::Method;
use serde::Deserialize;

use super::client::{api_error, http_error, valid_id, Client};
use super::ROOT;
use crate::remote::{http, PhasePermit};
use crate::{Error, ExecutionPermit, Result};

impl Client {
    pub(super) async fn upload(
        &self,
        id: &str,
        archive: crate::remote::archive::TempArchive,
        phase: PhasePermit,
    ) -> Result<()> {
        let (sender, receiver) = tokio::sync::mpsc::channel(2);
        tokio::task::spawn_blocking(move || {
            let _phase = phase;
            let result = (|| -> std::io::Result<()> {
                let file = std::fs::File::open(archive.path())?;
                let mut gzip = flate2::read::GzEncoder::new(file, flate2::Compression::fast());
                loop {
                    let mut chunk = vec![0; 64 * 1024];
                    let count = std::io::Read::read(&mut gzip, &mut chunk)?;
                    if count == 0 {
                        return Ok(());
                    }
                    chunk.truncate(count);
                    if sender
                        .blocking_send(Ok::<_, std::io::Error>(chunk))
                        .is_err()
                    {
                        return Ok(());
                    }
                }
            })();
            if let Err(error) = result {
                let _ = sender.blocking_send(Err(error));
            }
        });
        let chunks = futures::stream::unfold(receiver, |mut receiver| async move {
            receiver.recv().await.map(|chunk| (chunk, receiver))
        });
        let progress = http::Progress::new();
        let body = reqwest::Body::wrap_stream(progress.track(chunks));
        let response = progress
            .watch_upload("vercel", self.limits, async {
                self.request(
                    Method::POST,
                    &format!("/v2/sandboxes/sessions/{id}/fs/write"),
                )
                .header("Content-Type", "application/gzip")
                .header("x-cwd", ROOT)
                .body(body)
                .send()
                .await
                .map_err(http_error)
            })
            .await?;
        http::checked(response, "vercel")?;
        Ok(())
    }

    pub(super) async fn command(
        &self,
        id: &str,
        argv: &[String],
        cwd: &str,
        env: &BTreeMap<String, String>,
    ) -> Result<i32> {
        #[derive(Deserialize)]
        struct Response {
            command: Command,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Command {
            id: String,
            session_id: String,
            exit_code: Option<i32>,
        }
        let (executable, arguments) = argv.split_first().ok_or(Error::EmptyArgv)?;
        let body = serde_json::json!({"command": executable, "args": arguments, "cwd": cwd, "env": env, "wait": false, "logs": false});
        let response: Response = self
            .decode(
                self.json(
                    self.request(Method::POST, &format!("/v2/sandboxes/sessions/{id}/cmd"))
                        .timeout(Duration::from_secs(30)),
                    &body,
                )?,
            )
            .await?;
        valid_id(&response.command.id)?;
        if response.command.session_id != id {
            return Err(api_error("command session identity changed"));
        }
        let command_id = response.command.id;
        // A non-waiting GET never records completion in Vercel's API.
        let response: Response = self
            .decode(
                self.request(
                    Method::GET,
                    &format!("/v2/sandboxes/sessions/{id}/cmd/{command_id}"),
                )
                .query(&[("wait", "true")]),
            )
            .await?;
        if response.command.id != command_id || response.command.session_id != id {
            return Err(api_error("command identity changed"));
        }
        response
            .command
            .exit_code
            .ok_or_else(|| api_error("wait response omitted the command exit code"))
    }

    pub(super) async fn download(
        &self,
        id: &str,
        source: &str,
        destination: &Path,
        lease: Option<&ExecutionPermit>,
        copies: u64,
    ) -> Result<PhasePermit> {
        let response = http::Progress::new()
            .watch(
                "vercel",
                "download headers",
                self.limits.stall_timeout,
                async {
                    self.json(
                        self.request(
                            Method::POST,
                            &format!("/v2/sandboxes/sessions/{id}/fs/read"),
                        ),
                        &serde_json::json!({"path": source}),
                    )?
                    .header("Accept", "application/octet-stream")
                    .send()
                    .await
                    .map_err(http_error)
                },
            )
            .await?;
        let response = http::checked(response, "vercel")?;
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        if response.status() != reqwest::StatusCode::OK
            || content_type
                .split(';')
                .next()
                .is_none_or(|value| value.trim() != "application/octet-stream")
        {
            return Err(api_error("expected a complete binary file response"));
        }
        http::download(response, destination, "vercel", self.limits, lease, copies).await
    }
}

pub(super) fn file_error(source: std::io::Error) -> Error {
    Error::FileAction {
        action: "transfer_remote_file",
        path: ".struo/tmp".into(),
        source,
    }
}
