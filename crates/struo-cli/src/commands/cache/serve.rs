use std::net::SocketAddr;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;
use struo_cas::TransferBudget;
use struo_core::ResourceLimits;
use tokio::io::AsyncReadExt;
use tokio::net::TcpListener;

pub(crate) async fn run(
    listen: SocketAddr,
    directory: &Path,
    token_file: &Path,
    capacity: u64,
    limits: &ResourceLimits,
) -> Result<()> {
    #[derive(Deserialize)]
    struct Credential {
        token: String,
    }

    let file = tokio::fs::File::open(token_file)
        .await
        .context("opening cache credential")?;
    let mut bytes = Vec::new();
    file.take(4097)
        .read_to_end(&mut bytes)
        .await
        .context("reading cache credential")?;
    anyhow::ensure!(bytes.len() <= 4096, "cache credential exceeds 4096 bytes");
    let credential: Credential =
        serde_json::from_slice(&bytes).context("invalid cache credential JSON")?;
    anyhow::ensure!(
        !credential.token.is_empty(),
        "cache token must not be empty"
    );
    let listener = TcpListener::bind(listen)
        .await
        .context("binding cache listener")?;
    tracing::info!(address = %listener.local_addr()?, capacity, "serving HTTP cache");
    struo_cas::server::serve(
        listener,
        directory,
        credential.token,
        capacity,
        TransferBudget::new(limits.memory_bytes, capacity),
    )
    .await
    .context("serving HTTP cache")
}
