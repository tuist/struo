//! Authenticated, disposable action service. The caller owns listener placement,
//! credential delivery, prepared tools, and provider idle suspension.

mod cas;
mod execution;
mod http;
mod sandbox;
mod shared;
pub(crate) use shared::Pull;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use struo_cas::{ActionResult, Digest};

use crate::WorkspacePath;

pub const PROTOCOL_VERSION: u32 = 1;
pub const EXECUTION_ROOT: &str = "/home/sprite/struo";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub path: WorkspacePath,
    pub kind: InputKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum InputKind {
    File { digest: Digest, executable: bool },
    Directory,
    Symlink { target: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: u32,
    pub environment: String,
    pub argv: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: Option<WorkspacePath>,
    pub inputs: Vec<Input>,
    pub post_setup_inputs: Vec<Input>,
    pub outputs: Vec<WorkspacePath>,
    pub clean_paths: Vec<WorkspacePath>,
    pub create_dirs: Vec<WorkspacePath>,
    pub stdout: Option<WorkspacePath>,
    pub stderr: Option<WorkspacePath>,
    pub timeout_ms: Option<u64>,
    pub success_exit_codes: Vec<i32>,
    pub memory_bytes: u64,
    #[serde(default)]
    pub shared_cache: Option<SharedCache>,
}

/// Cache-only connection owned by the service, never mounted into an action.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SharedCache {
    pub connection: struo_cas::HttpCacheConnection,
    pub read_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub result: ActionResult,
    pub output_archive: Option<Digest>,
    pub output_archive_bytes: u64,
    #[serde(default)]
    pub shared_blobs: Vec<Digest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServeOptions {
    pub environment: String,
    pub preparation_root: PathBuf,
    pub tool_environment: BTreeMap<String, String>,
    pub tool_launcher: Vec<String>,
    pub memory_bytes: u64,
    pub capacity_bytes: u64,
}

/// Run until the listener fails or the future is dropped. All endpoints require
/// the bearer token. Only one connection is admitted at a time, including body
/// transfer and action cleanup; disconnected POSTs are never replayed.
pub async fn serve(
    listener: tokio::net::TcpListener,
    root: PathBuf,
    token: String,
    options: ServeOptions,
) -> crate::Result<()> {
    http::serve(listener, root, token, options)
        .await
        .map_err(|source| crate::Error::FileAction {
            action: "worker_service",
            path: "worker".into(),
            source,
        })
}
