use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use tokio::io::AsyncReadExt;

#[cfg(target_os = "linux")]
pub(crate) mod mounts;

pub(crate) async fn run(
    listen: SocketAddr,
    root: PathBuf,
    options: &Path,
    token_file: &Path,
) -> Result<()> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Credential {
        token: String,
    }
    let options = read::<struo_core::worker::ServeOptions>(options).await?;
    let credential = read::<Credential>(token_file).await?;
    let listener = tokio::net::TcpListener::bind(listen)
        .await
        .context("binding worker listener")?;
    struo_core::worker::serve(listener, root, credential.token, options)
        .await
        .context("serving isolated actions")
}

async fn read<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let mut bytes = Vec::new();
    tokio::fs::File::open(path)
        .await?
        .take(65_537)
        .read_to_end(&mut bytes)
        .await?;
    anyhow::ensure!(bytes.len() <= 65_536, "worker configuration exceeds 64 KiB");
    serde_json::from_slice(&bytes).context("invalid worker configuration")
}
