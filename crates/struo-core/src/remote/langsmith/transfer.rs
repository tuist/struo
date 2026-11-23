use std::path::Path;

use serde::Deserialize;
use tokio_util::io::ReaderStream;

use super::client::{api_error, http_error, Client, Sandbox};
use crate::remote::{http, PhasePermit};
use crate::{Error, ExecutionPermit, Result};

#[derive(Deserialize)]
struct Uploaded {
    path: String,
    written: u64,
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
            "langsmith",
            "archive bytes",
        )?;
        let file = tokio::fs::File::open(source).await.map_err(file_error)?;
        let progress = http::Progress::new();
        let body = reqwest::Body::wrap_stream(progress.track_bytes(ReaderStream::new(file), len));
        let part = reqwest::multipart::Part::stream_with_length(body, len).file_name("inputs.tar");
        let request = self
            .runtime(sandbox, reqwest::Method::POST, "/upload")?
            .query(&[("path", destination)])
            .multipart(reqwest::multipart::Form::new().part("file", part));
        let response = progress
            .watch_upload("langsmith", self.limits, async {
                request.send().await.map_err(http_error)
            })
            .await?;
        let uploaded: Uploaded = http::decode(
            response,
            "langsmith",
            self.limits.control_bytes(),
            self.limits,
        )
        .await?;
        if uploaded.path != destination || uploaded.written != len {
            return Err(api_error("upload receipt did not match archive"));
        }
        Ok(())
    }

    pub(super) async fn download(
        &self,
        sandbox: &Sandbox,
        source: &str,
        destination: &Path,
        lease: Option<&ExecutionPermit>,
    ) -> Result<PhasePermit> {
        let request = self
            .runtime(sandbox, reqwest::Method::GET, "/download")?
            .query(&[("path", source)]);
        let response = http::Progress::new()
            .watch(
                "langsmith",
                "download headers",
                self.limits.stall_timeout,
                async { request.send().await.map_err(http_error) },
            )
            .await?;
        if response.status() != reqwest::StatusCode::OK {
            http::checked(response, "langsmith")?;
            return Err(api_error("download did not return a complete file"));
        }
        http::download(response, destination, "langsmith", self.limits, lease, 2).await
    }
}

pub(super) fn file_error(source: std::io::Error) -> Error {
    Error::FileAction {
        action: "remote_transfer",
        path: ".struo/tmp".into(),
        source,
    }
}
