use std::collections::BTreeMap;
use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use struo_cas::Digest;

mod cache;
pub(super) use cache::LockPlanCache;

pub(super) struct OutputLock {
    file: File,
    exclusive: bool,
}

impl OutputLock {
    #[cfg(test)]
    pub(super) fn open_shared<'a>(
        workspace: &Path,
        outputs: impl IntoIterator<Item = &'a str>,
    ) -> Result<Vec<Self>> {
        let mut locks = Self::open_all(workspace, Digest::of_bytes(b"output-validation"), outputs)?;
        for lock in &mut locks {
            lock.exclusive = false;
        }
        Ok(locks)
    }

    pub(super) fn open_all<'a>(
        workspace: &Path,
        digest: Digest,
        outputs: impl IntoIterator<Item = &'a str>,
    ) -> Result<Vec<Self>> {
        LockPlan::new(workspace, digest, outputs)?.open(false)
    }

    /// Named slots stand for host resources, such as a simulator, so they
    /// must also exclude other Struo processes: `struo test` runs each batch
    /// in its own process with its own resource pool.
    pub(super) fn open_slots(request: &struo_core::ResourceRequest) -> Result<Vec<Self>> {
        if request.slots.is_empty() {
            return Ok(Vec::new());
        }
        let directory = struo_core::Xdg::from_env()
            .struo_runtime()
            .join("locks/slots");
        let paths = request
            .slots
            .keys()
            .map(|name| {
                let key = Digest::of_bytes(name.as_bytes());
                (directory.join(format!("{key}.lock")), true)
            })
            .collect();
        LockPlan { directory, paths }.open(false)
    }

    pub(super) fn try_lock(&self) -> std::result::Result<(), TryLockError> {
        if self.exclusive {
            self.file.try_lock()
        } else {
            self.file.try_lock_shared()
        }
    }

    pub(super) fn unlock(&self) -> std::io::Result<()> {
        self.file.unlock()
    }
}

#[derive(Debug)]
struct LockPlan {
    directory: PathBuf,
    paths: Vec<(PathBuf, bool)>,
}

impl LockPlan {
    fn new<'a>(
        workspace: &Path,
        digest: Digest,
        outputs: impl IntoIterator<Item = &'a str>,
    ) -> Result<Self> {
        let directory = workspace.join(".struo/locks/actions");
        let normalized = outputs
            .into_iter()
            .map(|output| super::workspace_path(output, "action lock path"))
            .collect::<Result<Vec<_>>>()?;
        let mut paths = BTreeMap::<&Path, bool>::new();
        for output in &normalized {
            let path = Path::new(output.as_str());
            paths.insert(path, true);
            // Shared ancestor locks let siblings proceed, but exclude writers
            // replacing a whole tree while one of its children is in use.
            for ancestor in path
                .ancestors()
                .skip(1)
                .filter(|p| !p.as_os_str().is_empty())
            {
                paths.entry(ancestor).or_insert(false);
            }
        }
        let mut paths = paths
            .into_iter()
            .map(|(path, exclusive)| {
                let key = Digest::of_bytes(path.as_os_str().as_encoded_bytes());
                (directory.join(format!("{key}.lock")), exclusive)
            })
            .collect::<Vec<_>>();
        if paths.is_empty() {
            paths.push((directory.join(format!("{digest}.lock")), true));
        }
        Ok(Self { directory, paths })
    }

    fn open(&self, shared: bool) -> Result<Vec<OutputLock>> {
        std::fs::create_dir_all(&self.directory).with_context(|| {
            format!(
                "creating action lock directory `{}`",
                self.directory.display()
            )
        })?;
        self.paths
            .iter()
            .map(|(path, exclusive)| {
                let file = OpenOptions::new()
                    .create(true)
                    .truncate(false)
                    .read(true)
                    .write(true)
                    .open(path)
                    .with_context(|| format!("opening action lock `{}`", path.display()))?;
                Ok(OutputLock {
                    file,
                    exclusive: *exclusive && !shared,
                })
            })
            .collect()
    }

    fn storage_bytes(&self) -> usize {
        self.directory.capacity()
            + self.paths.capacity() * size_of::<(PathBuf, bool)>()
            + self
                .paths
                .iter()
                .map(|(path, _)| path.capacity())
                .sum::<usize>()
    }
}
