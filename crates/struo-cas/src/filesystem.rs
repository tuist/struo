use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use tokio::fs::{self, File};
use tokio::io::AsyncWriteExt;

use crate::{Error, Result};

pub(crate) static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// How stale an entry's modification time may be before a use refreshes it.
/// Garbage collection reads the time as last use, so this only has to be
/// much finer than its grace period; skipping fresher entries keeps a build
/// that reuses thousands of entries from writing each one every time.
const USE_RESOLUTION: Duration = Duration::from_mins(1);

/// Record a use of the entry at `path` for garbage collection. Best effort:
/// a missing entry or a store this process cannot write is left alone.
pub(crate) fn mark_used_blocking(path: &Path) {
    let now = SystemTime::now();
    let stale = std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .is_ok_and(|modified| {
            now.duration_since(modified)
                .is_ok_and(|age| age >= USE_RESOLUTION)
        });
    if stale {
        // Windows sets file times only through a handle opened for writing.
        if let Ok(file) = std::fs::OpenOptions::new().write(true).open(path) {
            let _ = file.set_modified(now);
        }
    }
}

pub(crate) async fn mark_used(paths: Vec<PathBuf>) {
    let _ = tokio::task::spawn_blocking(move || {
        for path in &paths {
            mark_used_blocking(path);
        }
    })
    .await;
}

#[derive(Clone, Copy)]
enum Durability {
    Atomic,
    Contents,
    ContentsAndDirectory,
}

pub(crate) async fn write_durably(path: &Path, bytes: &[u8]) -> Result<()> {
    write(path, bytes, Durability::ContentsAndDirectory).await
}

pub(crate) async fn write_atomically(path: &Path, bytes: &[u8]) -> Result<()> {
    write(path, bytes, Durability::Atomic).await
}

pub(crate) async fn write_with_durable_contents(path: &Path, bytes: &[u8]) -> Result<()> {
    write(path, bytes, Durability::Contents).await
}

async fn write(path: &Path, bytes: &[u8], durability: Durability) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    ensure_dir(parent).await?;
    let pid = process::id();
    let seq = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let basename = path.file_name().map_or_else(
        || std::borrow::Cow::Borrowed(""),
        |name| name.to_string_lossy(),
    );
    let tmp = parent.join(format!(".tmp-{basename}-{pid}-{seq}"));

    let result = async {
        let mut file = File::create(&tmp).await.map_err(|source| Error::Io {
            path: tmp.clone(),
            source,
        })?;
        file.write_all(bytes).await.map_err(|source| Error::Io {
            path: tmp.clone(),
            source,
        })?;
        file.flush().await.map_err(|source| Error::Io {
            path: tmp.clone(),
            source,
        })?;
        if !matches!(durability, Durability::Atomic) {
            file.sync_all().await.map_err(|source| Error::Io {
                path: tmp.clone(),
                source,
            })?;
        }
        drop(file);
        fs::rename(&tmp, path).await.map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })?;
        if matches!(durability, Durability::ContentsAndDirectory) {
            fsync_dir(parent).await?;
        }
        Ok(())
    }
    .await;

    if result.is_err() {
        let _ = fs::remove_file(&tmp).await;
    }
    result
}

#[cfg(unix)]
async fn fsync_dir(dir: &Path) -> Result<()> {
    let file = File::open(dir).await.map_err(|source| Error::Io {
        path: dir.to_path_buf(),
        source,
    })?;
    file.sync_all().await.map_err(|source| Error::Io {
        path: dir.to_path_buf(),
        source,
    })
}

#[cfg(not(unix))]
async fn fsync_dir(_: &Path) -> Result<()> {
    Ok(())
}

pub(crate) async fn rename_into_place(tmp: &Path, final_path: &Path) -> Result<()> {
    let parent = final_path.parent().expect("shard path has parent");
    ensure_dir(parent).await?;
    fs::rename(tmp, final_path)
        .await
        .map_err(|source| Error::Io {
            path: final_path.to_path_buf(),
            source,
        })?;
    fsync_dir(parent).await
}

pub(crate) async fn ensure_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path).await.map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn every_durability_policy_replaces_complete_contents() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("entry");
        for durability in [
            Durability::Atomic,
            Durability::Contents,
            Durability::ContentsAndDirectory,
        ] {
            fs::write(&path, b"old content").await.unwrap();
            write(&path, b"new", durability).await.unwrap();
            assert_eq!(fs::read(&path).await.unwrap(), b"new");
            assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 1);
        }
    }

    #[tokio::test]
    async fn atomic_writes_are_complete_when_they_return() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("entry");
        let bytes = vec![0x5a; 4 * 1024 * 1024];
        for _ in 0..4 {
            write_atomically(&path, &bytes).await.unwrap();
            assert_eq!(fs::read(&path).await.unwrap(), bytes);
        }
    }

    #[tokio::test]
    async fn failed_rename_cleans_up_the_temporary_file_for_every_policy() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("entry");
        fs::create_dir(&path).await.unwrap();
        fs::write(path.join("keep"), b"existing").await.unwrap();
        for durability in [
            Durability::Atomic,
            Durability::Contents,
            Durability::ContentsAndDirectory,
        ] {
            assert!(write(&path, b"replacement", durability).await.is_err());
            assert_eq!(fs::read(path.join("keep")).await.unwrap(), b"existing");
            assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 1);
        }
    }
}
