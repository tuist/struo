use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{config, protocol, PROVIDER};
use crate::{Error, ErrorKind, Result};

#[derive(Deserialize, Serialize)]
pub(super) struct Record {
    pub name: String,
    pub id: Option<String>,
    pub token: String,
}

pub(super) struct State {
    path: PathBuf,
    _lock: File,
    pub record: Record,
}

impl State {
    pub async fn open(root: &Path) -> Result<Self> {
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(root).map_err(|source| io(root, source))?;
        let lock_path = root.join("lock");
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .map_err(|source| io(&lock_path, source))?;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(120);
        loop {
            match lock.try_lock() {
                Ok(()) => break,
                Err(std::fs::TryLockError::WouldBlock)
                    if tokio::time::Instant::now() < deadline =>
                {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
                Err(std::fs::TryLockError::WouldBlock) => {
                    return Err(Error::remote(
                        PROVIDER,
                        "provision",
                        ErrorKind::Unavailable,
                        "another process is provisioning the cache",
                    ))
                }
                Err(std::fs::TryLockError::Error(source)) => return Err(io(&lock_path, source)),
            }
        }
        let path = root.join("state.json");
        let record = match File::open(&path) {
            Ok(file) => {
                let mut bytes = Vec::new();
                file.take(64 * 1024)
                    .read_to_end(&mut bytes)
                    .map_err(|source| io(&path, source))?;
                let record: Record = serde_json::from_slice(&bytes).map_err(|_| {
                    config("invalid Sprites cache state; inspect the credentials directory")
                })?;
                if record.name.len() != "struo-cache-".len() + 32
                    || !record.name.starts_with("struo-cache-")
                    || !record.name["struo-cache-".len()..]
                        .bytes()
                        .all(|b| b.is_ascii_hexdigit())
                    || record.token.len() != 64
                    || !record.token.bytes().all(|b| b.is_ascii_hexdigit())
                {
                    return Err(protocol("invalid retained cache identity"));
                }
                record
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Record {
                name: format!("struo-cache-{}", uuid::Uuid::now_v7().simple()),
                id: None,
                token: format!(
                    "{}{}",
                    uuid::Uuid::new_v4().simple(),
                    uuid::Uuid::new_v4().simple()
                ),
            },
            Err(source) => return Err(io(&path, source)),
        };
        let state = Self {
            path,
            _lock: lock,
            record,
        };
        state.save()?;
        Ok(state)
    }

    pub fn save(&self) -> Result<()> {
        let root = self.path.parent().unwrap();
        let mut file = tempfile::NamedTempFile::new_in(root).map_err(|source| io(root, source))?;
        serde_json::to_writer(&mut file, &self.record)
            .map_err(|_| protocol("encoding cache identity"))?;
        file.flush().map_err(|source| io(&self.path, source))?;
        file.as_file()
            .sync_all()
            .map_err(|source| io(&self.path, source))?;
        file.persist(&self.path)
            .map_err(|error| io(&self.path, error.error))?;
        #[cfg(unix)]
        File::open(root)
            .and_then(|dir| dir.sync_all())
            .map_err(|source| io(root, source))?;
        Ok(())
    }
}

fn io(path: &Path, source: std::io::Error) -> Error {
    Error::Io {
        path: path.to_path_buf(),
        source,
    }
}
