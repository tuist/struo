//! Retained slots under a caller-owned runtime-state directory.
//! Locks are never unlinked. Keep a Slot alive through execution and cleanup;
//! persist intent before creating a worker and dirty state before using it.

use std::fs::{File, TryLockError};
use std::path::{Path, PathBuf};

use crate::{Error, Result};

mod record;
mod storage;
#[cfg(test)]
mod tests;

pub(super) use record::Record;

const MAX_SLOTS: usize = 4096;

#[derive(Debug)]
pub(super) struct Pool {
    root: PathBuf,
    max_workers: usize,
}

#[derive(Debug)]
pub(super) struct Slot {
    root: PathBuf,
    index: usize,
    record: Option<Record>,
    _lock: Lease,
}

#[derive(Debug)]
struct Lease(File);

impl Drop for Lease {
    fn drop(&mut self) {
        // Explicit unlock also releases transient descriptors inherited by a
        // concurrent fork before its exec closes them.
        let _ = self.0.unlock();
    }
}

impl Pool {
    /// `root` must be dedicated to this pool, not an environment generation.
    /// A decreased cap fails closed if more slots already exist, including
    /// empty slots. Reopen with the previous cap to reconcile retained state.
    pub(super) fn new(root: &Path, max_workers: usize) -> Result<Self> {
        if !(1..=MAX_SLOTS).contains(&max_workers) {
            return Err(error(
                root,
                storage::invalid("invalid worker pool capacity"),
            ));
        }
        storage::create_root(root).map_err(|source| error(root, source))?;
        Ok(Self {
            root: root.to_owned(),
            max_workers,
        })
    }

    /// Nonblocking, including the pool-wide allocation lock. An incompatible
    /// or dirty record is returned unchanged for caller-owned reconciliation.
    pub(super) fn try_claim(&self, environment: &str) -> Result<Option<Slot>> {
        self.claim(environment)
            .map_err(|source| error(&self.root, source))
    }

    fn claim(&self, environment: &str) -> std::io::Result<Option<Slot>> {
        record::validate_text(environment)?;
        let Some(_allocation) = try_lock(&self.root.join("capacity.lock"))? else {
            return Ok(None);
        };
        let indices = storage::indices(&self.root)?;
        if indices.len() > self.max_workers {
            return Err(storage::invalid(
                "existing worker slots exceed requested capacity",
            ));
        }
        let mut fallback: Option<Slot> = None;
        for &index in &indices {
            let Some(lock) = try_lock(&self.root.join(format!("{index}.lock")))? else {
                continue;
            };
            let record = storage::read(&self.root.join(format!("{index}.json")))?;
            let compatible = record
                .as_ref()
                .is_some_and(|record| record.environment == environment && !record.dirty);
            let slot = Slot {
                root: self.root.clone(),
                index,
                record,
                _lock: lock,
            };
            if compatible {
                return Ok(Some(slot));
            }
            if fallback.is_none()
                || (slot.record.is_none()
                    && fallback.as_ref().is_some_and(|slot| slot.record.is_some()))
            {
                fallback = Some(slot);
            }
        }
        if fallback.is_some() {
            return Ok(fallback);
        }
        if indices.len() == self.max_workers {
            return Ok(None);
        }
        let index = (0..MAX_SLOTS)
            .find(|index| !indices.contains(index))
            .ok_or_else(|| storage::invalid("worker pool is full"))?;
        let Some(lock) = try_lock(&self.root.join(format!("{index}.lock")))? else {
            return Ok(None);
        };
        let slot = Slot {
            root: self.root.clone(),
            index,
            record: None,
            _lock: lock,
        };
        slot.save().map_err(std::io::Error::other)?;
        Ok(Some(slot))
    }
}

impl Slot {
    pub(super) fn record(&self) -> Option<&Record> {
        self.record.as_ref()
    }

    pub(super) fn record_mut(&mut self) -> Option<&mut Record> {
        self.record.as_mut()
    }

    /// Journal a new creation intent. Only valid for an empty slot.
    pub(super) fn begin(&mut self, environment: &str) -> Result<()> {
        if self.record.is_some() {
            return Err(error(
                &self.root,
                storage::invalid("worker slot is not empty"),
            ));
        }
        self.record = Some(Record::new(environment).map_err(|source| error(&self.root, source))?);
        self.save()
    }

    /// Call after mutating provider identity, checkpoint, or dirty state and
    /// before any external operation that relies on the changed state.
    pub(super) fn save(&self) -> Result<()> {
        storage::write(&self.root, self.index, self.record.as_ref())
            .map_err(|source| error(&self.root, source))
    }

    /// The caller must confirm deletion, including uncertain creates, first.
    /// Retains the same lease and slot lock inode.
    pub(super) fn reset(&mut self) -> Result<()> {
        self.record = None;
        self.save()
    }
}

fn try_lock(path: &Path) -> std::io::Result<Option<Lease>> {
    let file = storage::open_lock(path)?;
    match file.try_lock() {
        Ok(()) => Ok(Some(Lease(file))),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(error)) => Err(error),
    }
}

fn error(root: &Path, source: std::io::Error) -> Error {
    Error::FileAction {
        action: "worker_pool_state",
        path: root.display().to_string(),
        source,
    }
}
