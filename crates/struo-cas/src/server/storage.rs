use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use tokio::fs;

use super::protocol::valid_key;

pub(super) fn charge(length: u64) -> u64 {
    length.max(1).div_ceil(4096).saturating_mul(4096)
}

pub(super) struct Storage {
    pub directory: PathBuf,
    capacity: u64,
    used: u64,
    last_access: SystemTime,
}

impl Storage {
    pub async fn open(directory: &Path, capacity: u64) -> io::Result<Self> {
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        builder.mode(0o700);
        builder.create(directory).await?;
        let mut storage = Self {
            directory: directory.to_owned(),
            capacity,
            used: 0,
            last_access: SystemTime::UNIX_EPOCH,
        };
        let mut entries = fs::read_dir(directory).await?;
        while let Some(entry) = entries.next_entry().await? {
            if !entry.file_type().await?.is_file() {
                continue;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(".upload-") {
                fs::remove_file(entry.path()).await?;
            } else if valid_key(&name) {
                let metadata = entry.metadata().await?;
                storage.last_access = storage.last_access.max(metadata.accessed()?);
                storage.used = storage
                    .used
                    .checked_add(charge(metadata.len()))
                    .ok_or_else(|| io::Error::other("cache usage exceeds u64"))?;
            }
        }
        storage.make_room(0, "").await?;
        Ok(storage)
    }

    pub async fn prepare(&mut self, key: &str, length: u64) -> io::Result<bool> {
        let old = self.object_charge(key).await?;
        let required = charge(length);
        if old > self.capacity.saturating_sub(required) || required > self.capacity {
            return Ok(false);
        }
        self.make_room(required, key).await
    }

    async fn object_charge(&self, key: &str) -> io::Result<u64> {
        match fs::symlink_metadata(self.directory.join(key)).await {
            Ok(metadata) if metadata.is_file() => Ok(charge(metadata.len())),
            Ok(_) => Err(io::Error::other("cache object is not a regular file")),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(0),
            Err(error) => Err(error),
        }
    }

    async fn make_room(&mut self, required: u64, protected: &str) -> io::Result<bool> {
        while required > self.capacity || self.used > self.capacity - required {
            let mut oldest: Option<(SystemTime, String, u64)> = None;
            let mut entries = fs::read_dir(&self.directory).await?;
            while let Some(entry) = entries.next_entry().await? {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name == protected || !valid_key(&name) || !entry.file_type().await?.is_file() {
                    continue;
                }
                let metadata = entry.metadata().await?;
                let candidate = (metadata.accessed()?, name, charge(metadata.len()));
                if oldest.as_ref().is_none_or(|old| &candidate < old) {
                    oldest = Some(candidate);
                }
            }
            let Some((_, name, charge)) = oldest else {
                return Ok(false);
            };
            // Keep unlink and accounting indivisible when the server is cancelled.
            std::fs::remove_file(self.directory.join(name))?;
            self.used -= charge;
        }
        Ok(true)
    }

    pub fn publish(
        &mut self,
        temporary: tempfile::NamedTempFile,
        key: &str,
        length: u64,
    ) -> io::Result<()> {
        let path = self.directory.join(key);
        let old = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => charge(metadata.len()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
            Err(error) => return Err(error),
        };
        temporary.persist(&path).map_err(|error| error.error)?;
        self.used = self.used - old + charge(length);
        std::fs::File::open(&self.directory)?.sync_all()
    }

    pub fn touch(&mut self, file: &std::fs::File) -> io::Result<()> {
        // Preserve access order across clock ties, rollback, and server restarts.
        let next = self
            .last_access
            .checked_add(Duration::from_nanos(1))
            .ok_or_else(|| io::Error::other("cache access timestamp overflow"))?
            .max(SystemTime::now());
        file.set_times(std::fs::FileTimes::new().set_accessed(next))?;
        self.last_access = next;
        Ok(())
    }

    pub async fn reader(&mut self, key: &str) -> io::Result<Option<(fs::File, u64)>> {
        if self.object_charge(key).await? == 0 {
            return Ok(None);
        }
        let source = fs::File::open(self.directory.join(key)).await?;
        let length = source.metadata().await?.len();
        let source = source.into_std().await;
        self.touch(&source)?;
        Ok(Some((fs::File::from_std(source), length)))
    }
}
