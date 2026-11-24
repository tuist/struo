use std::path::Path;

use reqwest::multipart::{Form, Part};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::io::ReaderStream;

use super::client::{api_error, http_error, Client, Sandbox};
use crate::remote::{http, PhasePermit};
use crate::{Error, ExecutionPermit, Result};

const PART_BYTES: u64 = 5 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Initiated {
    upload_id: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct UploadedPart {
    part_number: u64,
    etag: String,
}

fn encoded_path(path: &str) -> String {
    let mut url = reqwest::Url::parse("http://localhost/").expect("static URL");
    url.path_segments_mut().expect("base URL").push(path);
    url.path().trim_start_matches('/').to_string()
}

impl Client {
    pub(super) async fn upload(
        &self,
        sandbox: &Sandbox,
        source: &Path,
        len: u64,
        destination: &str,
    ) -> Result<()> {
        http::check_size(
            len,
            self.limits.max_archive_bytes,
            "blaxel",
            "archive bytes",
        )?;
        let parts = len.div_ceil(PART_BYTES);
        http::check_size(parts, 10_000, "blaxel", "multipart parts")?;
        http::check_size(
            parts.saturating_mul(256),
            self.limits.control_bytes(),
            "blaxel",
            "request metadata bytes",
        )?;
        let response = self
            .guest(
                sandbox,
                reqwest::Method::POST,
                &format!(
                    "filesystem-multipart/initiate/{}",
                    encoded_path(destination)
                ),
            )?
            .json(&serde_json::json!({ "permissions": "0600" }))
            .send()
            .await
            .map_err(http_error)?;
        let upload: Initiated = self.decode(response).await?;
        if upload.upload_id.is_empty()
            || !upload
                .upload_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(api_error("invalid multipart upload id"));
        }
        // Whole-sandbox deletion also discards an interrupted multipart session.
        let result = self
            .upload_parts(sandbox, source, len, &upload.upload_id)
            .await;
        if result.is_err() {
            let _ = self
                .guest(
                    sandbox,
                    reqwest::Method::DELETE,
                    &format!("filesystem-multipart/{}/abort", upload.upload_id),
                )?
                .send()
                .await;
        }
        result
    }

    async fn upload_parts(
        &self,
        sandbox: &Sandbox,
        source: &Path,
        len: u64,
        id: &str,
    ) -> Result<()> {
        let mut parts = Vec::new();
        let mut offset = 0;
        while offset < len {
            let length = (len - offset).min(PART_BYTES);
            let mut file = tokio::fs::File::open(source).await.map_err(file_error)?;
            file.seek(std::io::SeekFrom::Start(offset))
                .await
                .map_err(file_error)?;
            let progress = http::Progress::new();
            let body = reqwest::Body::wrap_stream(
                progress.track_bytes(ReaderStream::new(file.take(length)), length),
            );
            let part_number = offset / PART_BYTES + 1;
            let form = Form::new().part(
                "file",
                Part::stream_with_length(body, length).file_name("struo-inputs.part"),
            );
            let response = progress
                .watch_upload("blaxel", self.limits, async {
                    self.guest_transfer(
                        sandbox,
                        reqwest::Method::PUT,
                        &format!("filesystem-multipart/{id}/part"),
                    )?
                    .query(&[("partNumber", part_number)])
                    .multipart(form)
                    .send()
                    .await
                    .map_err(http_error)
                })
                .await?;
            let part: UploadedPart = self.decode(response).await?;
            if part.part_number != part_number || part.etag.is_empty() || part.etag.len() > 128 {
                return Err(api_error("multipart part acknowledgement is invalid"));
            }
            parts.push(part);
            offset += length;
        }
        let response = self
            .guest(
                sandbox,
                reqwest::Method::POST,
                &format!("filesystem-multipart/{id}/complete"),
            )?
            .header("Content-Type", "application/json")
            .body(http::encode(
                &serde_json::json!({ "parts": parts }),
                "blaxel",
                self.limits.control_bytes(),
            )?)
            .send()
            .await
            .map_err(http_error)?;
        http::checked(response, "blaxel")?;
        Ok(())
    }

    pub(super) async fn download(
        &self,
        sandbox: &Sandbox,
        source: &str,
        destination: &Path,
        lease: Option<&ExecutionPermit>,
        copies: u64,
    ) -> Result<PhasePermit> {
        let response = http::Progress::new()
            .watch(
                "blaxel",
                "download headers",
                self.limits.stall_timeout,
                async {
                    self.guest_transfer(
                        sandbox,
                        reqwest::Method::GET,
                        &format!("filesystem/{}", encoded_path(source)),
                    )?
                    .header("Accept", "application/octet-stream")
                    .send()
                    .await
                    .map_err(http_error)
                },
            )
            .await?;
        let response = http::checked(response, "blaxel")?;
        if response.status() != reqwest::StatusCode::OK {
            return Err(api_error("binary download was not a complete response"));
        }
        if response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .is_some_and(|value| {
                value
                    .to_str()
                    .is_ok_and(|value| value.starts_with("application/json"))
            })
        {
            return Err(api_error(
                "binary download returned JSON instead of file bytes",
            ));
        }
        http::download(response, destination, "blaxel", self.limits, lease, copies).await
    }
}

fn file_error(source: std::io::Error) -> Error {
    Error::FileAction {
        action: "read_remote_input_archive",
        path: ".struo/tmp".into(),
        source,
    }
}
