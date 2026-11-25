use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{Error, Result, MANAGED_MISE_VERSION};

/// Locked inputs and capacity policy for retained execution workers.
///
/// Callers snapshot configuration before action lookup. The execution adapter
/// consumes these bytes, never a later version of the coordinator's files.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct WorkerPoolSpec {
    /// Wire contract included in the resolved execution identity.
    pub protocol_version: u32,
    /// Maximum retained workers across all environment generations.
    pub max_workers: usize,
    /// Replace an idle worker on its next lease after this age.
    pub max_age_seconds: u64,
    /// Direct preparation argument vectors, run before the locked installation.
    pub setup: Vec<Vec<String>>,
    /// Workspace-relative tool configuration and lock files, as UTF-8.
    pub files: BTreeMap<String, String>,
}

impl Default for WorkerPoolSpec {
    fn default() -> Self {
        Self {
            protocol_version: crate::worker::PROTOCOL_VERSION,
            max_workers: 1,
            max_age_seconds: 7 * 24 * 60 * 60,
            setup: Vec::new(),
            files: BTreeMap::new(),
        }
    }
}

impl WorkerPoolSpec {
    /// Check the bounded, portable preparation contract before provisioning.
    pub fn validate(&self) -> Result<()> {
        let invalid = |message: &str| Error::RemoteProviderConfig {
            provider: "worker_pool".into(),
            message: message.into(),
        };
        if self.protocol_version != crate::worker::PROTOCOL_VERSION {
            return Err(invalid("unsupported worker protocol version"));
        }
        if self.max_workers == 0 || self.max_age_seconds == 0 {
            return Err(invalid(
                "worker_pool max_workers and max_age_seconds must be positive",
            ));
        }
        if !self.files.contains_key("mise.toml") || !self.files.contains_key("mise.lock") {
            return Err(invalid(
                "worker reuse requires mise.toml and mise.lock; run mise lock for linux-x64",
            ));
        }
        let mut bytes = 0usize;
        for (path, contents) in &self.files {
            if path.is_empty()
                || path.contains('\\')
                || path.chars().any(char::is_control)
                || path
                    .split('/')
                    .any(|part| part.is_empty() || matches!(part, "." | ".."))
                || path.contains(':')
            {
                return Err(invalid(
                    "worker environment files must have portable relative paths",
                ));
            }
            bytes = bytes
                .saturating_add(path.len())
                .saturating_add(contents.len());
        }
        for command in &self.setup {
            if command.first().is_none_or(String::is_empty)
                || command.iter().any(|argument| argument.contains('\0'))
            {
                return Err(invalid(
                    "worker_pool setup must contain nonempty argument vectors without NUL bytes",
                ));
            }
            for argument in command {
                bytes = bytes.saturating_add(argument.len());
            }
        }
        if bytes > 1024 * 1024 {
            return Err(invalid("worker environment configuration exceeds 1 MiB"));
        }
        Ok(())
    }

    /// Versioned environment identity, independent of pool size and lifetime.
    #[must_use]
    pub fn fingerprint(&self) -> String {
        let mut hash = blake3::Hasher::new();
        hash.update(b"struo.worker-environment.v3\0linux-x64\0");
        hash.update(&self.protocol_version.to_le_bytes());
        hash.update(MANAGED_MISE_VERSION.as_bytes());
        // Length framing preserves argument and file boundaries.
        for (path, contents) in &self.files {
            for bytes in [path.as_bytes(), contents.as_bytes()] {
                hash.update(&(bytes.len() as u64).to_le_bytes());
                hash.update(bytes);
            }
        }
        hash.update(&(self.setup.len() as u64).to_le_bytes());
        for command in &self.setup {
            hash.update(&(command.len() as u64).to_le_bytes());
            for argument in command {
                hash.update(&(argument.len() as u64).to_le_bytes());
                hash.update(argument.as_bytes());
            }
        }
        hash.finalize().to_hex().to_string()
    }
}
