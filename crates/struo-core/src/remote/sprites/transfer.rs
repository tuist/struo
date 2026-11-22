use std::path::Path;

use struo_cas::CacheProvider;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::io::ReaderStream;

use super::client::{Client, Sprite};
use super::{setup, INPUT_ARCHIVE, OUTPUT_ARCHIVE};
use crate::executor::staging::CapturePermit;
use crate::remote::http::{self, Progress};
use crate::remote::PhasePermit;
use crate::{Error, ExecutionPermit, Result};

const WRITE_LIMIT: u64 = 16 * 1024 * 1024;
const PART_PATH: &str = "/tmp/struo-input-part";

impl Client {
    pub async fn upload(
        &self,
        sprite: &Sprite,
        path: &Path,
        len: u64,
        cache: &CacheProvider,
        budget: Option<&CapturePermit>,
    ) -> Result<()> {
        let mut offset = 0;
        while offset < len {
            let size = (len - offset).min(WRITE_LIMIT);
            let mut file = tokio::fs::File::open(path).await.map_err(file_error)?;
            file.seek(std::io::SeekFrom::Start(offset))
                .await
                .map_err(file_error)?;
            let progress = Progress::new();
            let body = reqwest::Body::wrap_stream(progress.track_bytes(
                ReaderStream::with_capacity(file.take(size), 64 * 1024),
                size,
            ));
            let response = progress
                .watch_upload("sprites", self.limits, async {
                    self.request(reqwest::Method::PUT, self.url(&sprite.name, "fs/write")?)
                        .query(&[("path", PART_PATH), ("workingDir", "/"), ("mode", "0600")])
                        .header("Content-Type", "application/octet-stream")
                        .header("Content-Length", size)
                        .body(body)
                        .send()
                        .await
                        .map_err(|error| http::http_error("sprites", error))
                })
                .await?;
            http::checked(response, "sprites")?;
            let mut args = vec![
                "dd".into(),
                format!("if={PART_PATH}"),
                format!("of={INPUT_ARCHIVE}"),
                "bs=65536".into(),
                "status=none".into(),
            ];
            if offset != 0 {
                args.extend(["oflag=append".into(), "conv=notrunc".into()]);
            }
            setup(self, sprite, &args, cache, budget).await?;
            offset += size;
        }
        Ok(())
    }

    pub async fn download(
        &self,
        sprite: &Sprite,
        destination: &Path,
        lease: Option<&ExecutionPermit>,
    ) -> Result<PhasePermit> {
        let response = Progress::new()
            .watch(
                "sprites",
                "download headers",
                self.limits.stall_timeout,
                async {
                    self.request(reqwest::Method::GET, self.url(&sprite.name, "fs/read")?)
                        .query(&[("path", OUTPUT_ARCHIVE), ("workingDir", "/")])
                        .send()
                        .await
                        .map_err(|error| http::http_error("sprites", error))
                },
            )
            .await?;
        http::download(response, destination, "sprites", self.limits, lease, 2).await
    }
}

fn file_error(source: std::io::Error) -> Error {
    Error::FileAction {
        action: "read_remote_input_archive",
        path: ".struo/tmp".into(),
        source,
    }
}
