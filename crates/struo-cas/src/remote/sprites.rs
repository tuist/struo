//! A retained, lazily provisioned Sprites HTTP cache, separate from execution VMs.

use std::fmt;
use std::path::{Path, PathBuf};

use bytes::Bytes;
use tokio::sync::OnceCell;

use super::{HttpCacheRemote, RemoteBody, RemoteStore};
use crate::{Error, ErrorKind, Result};

mod api;
mod bootstrap;
mod state;
#[cfg(all(test, unix))]
mod tests;

const PROVIDER: &str = "sprites";
const ROOT: &str = "/home/sprite/.struo-cache";
const SERVICE: &str = "struo-cache";
const PORT: u16 = 8080;
const SERVER: &str = include_str!("sprites/server.py");

/// A user-owned cache Sprite created on the first remote cache operation.
///
/// The cache is retained across commands and is never part of disposable
/// execution cleanup. Its identity and cache-only bearer credential are kept
/// under the user's credentials directory, scoped to the API and credential.
pub struct SpritesCacheRemote {
    api: api::Api,
    state_root: PathBuf,
    capacity: u64,
    remote: OnceCell<HttpCacheRemote>,
}

impl fmt::Debug for SpritesCacheRemote {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SpritesCacheRemote")
            .field("capacity", &self.capacity)
            .field("initialized", &self.remote.initialized())
            .finish_non_exhaustive()
    }
}

impl SpritesCacheRemote {
    /// Configure lazy provisioning with `STRUO_SPRITES_TOKEN` or `SPRITES_TOKEN`.
    /// This performs no filesystem writes or network requests. Cache capacity
    /// must be between 4 KiB and 80 GiB, leaving room on the guest's system disk.
    pub fn from_env(credentials_root: &Path, max_size_bytes: u64) -> Result<Self> {
        let token = std::env::var("STRUO_SPRITES_TOKEN")
            .or_else(|_| std::env::var("SPRITES_TOKEN"))
            .unwrap_or_default();
        let endpoint = std::env::var("STRUO_SPRITES_API_URL")
            .unwrap_or_else(|_| "https://api.sprites.dev".into());
        Self::new(credentials_root, max_size_bytes, &endpoint, token)
    }

    fn new(root: &Path, capacity: u64, endpoint: &str, token: String) -> Result<Self> {
        if !(4096..=80 * 1024 * 1024 * 1024).contains(&capacity) {
            return Err(config(
                "cache_max_size_bytes must be between 4096 and 85899345920",
            ));
        }
        let api = api::Api::new(endpoint, token)?;
        let state_root = root.join("sprites-cache").join(api.scope());
        Ok(Self {
            api,
            state_root,
            capacity,
            remote: OnceCell::new(),
        })
    }

    async fn remote(&self) -> Result<&HttpCacheRemote> {
        self.remote
            .get_or_try_init(|| bootstrap::provision(self))
            .await
    }
}

#[async_trait::async_trait]
impl RemoteStore for SpritesCacheRemote {
    fn provider(&self) -> &'static str {
        PROVIDER
    }

    async fn http_connection(&self) -> Result<Option<super::HttpCacheConnection>> {
        self.remote().await?.http_connection().await
    }

    async fn head(&self, key: &str) -> Result<Option<u64>> {
        self.remote().await?.head(key).await
    }

    async fn get(&self, key: &str) -> Result<Option<RemoteBody>> {
        self.remote().await?.get(key).await
    }

    async fn put_bytes(&self, key: &str, bytes: Bytes) -> Result<()> {
        self.remote().await?.put_bytes(key, bytes).await
    }

    async fn put_file(&self, key: &str, path: &Path, size: u64) -> Result<()> {
        self.remote().await?.put_file(key, path, size).await
    }

    fn upload_memory(&self, size: u64) -> u64 {
        super::http::upload_memory(size)
    }
}

fn config(message: impl Into<String>) -> Error {
    Error::InvalidConfig {
        provider: PROVIDER,
        message: message.into(),
    }
}

fn protocol(message: impl Into<String>) -> Error {
    Error::remote(PROVIDER, "provision", ErrorKind::Protocol, message)
}
