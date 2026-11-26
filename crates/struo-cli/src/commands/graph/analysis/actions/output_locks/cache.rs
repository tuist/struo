use std::sync::{Arc, Mutex, PoisonError};

use struo_core::{EvictableCache, ResourcePermit, ResourcePool};

use super::{BTreeMap, Digest, LockPlan, OutputLock, Path, Result};

#[derive(Debug)]
pub(in crate::commands::graph::analysis::actions) struct LockPlanCache {
    resources: Arc<ResourcePool>,
    current: Mutex<Option<Arc<CachedPlan>>>,
}

#[derive(Debug)]
struct CachedPlan {
    key: blake3::Hash,
    plan: LockPlan,
    _memory: ResourcePermit,
}

impl LockPlanCache {
    pub(in crate::commands::graph::analysis::actions) fn new(
        resources: Arc<ResourcePool>,
    ) -> Arc<Self> {
        let cache = Arc::new(Self {
            resources,
            current: Mutex::new(None),
        });
        cache
            .resources
            .register_cache(&(cache.clone() as Arc<dyn EvictableCache>));
        cache
    }

    pub(in crate::commands::graph::analysis::actions) fn open_shared(
        &self,
        workspace: &Path,
        outputs: &BTreeMap<String, Digest>,
    ) -> Result<Vec<OutputLock>> {
        let mut key = blake3::Hasher::new();
        for bytes in std::iter::once(workspace.as_os_str().as_encoded_bytes())
            .chain(outputs.keys().map(String::as_bytes))
        {
            key.update(&bytes.len().to_le_bytes());
            key.update(bytes);
        }
        let key = key.finalize();
        let cached = self
            .current
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .filter(|cached| cached.key == key)
            .cloned();
        if let Some(cached) = cached {
            tracing::trace!("reused output lock plan");
            return cached.plan.open(true);
        }
        let plan = LockPlan::new(
            workspace,
            Digest::of_bytes(b"output-validation"),
            outputs.keys().map(String::as_str),
        )?;
        let locks = plan.open(true)?;
        let bytes = plan.storage_bytes() + size_of::<CachedPlan>() + 2 * size_of::<usize>();
        let previous = {
            let mut current = self.current.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(memory) = self.resources.try_reserve_memory(bytes as u64) {
                tracing::trace!(plan_bytes = bytes, "retained output lock plan");
                current.replace(Arc::new(CachedPlan {
                    key,
                    plan,
                    _memory: memory,
                }))
            } else {
                tracing::trace!(
                    plan_bytes = bytes,
                    "output lock plan exceeds available budget"
                );
                None
            }
        };
        drop(previous);
        Ok(locks)
    }
}

impl EvictableCache for LockPlanCache {
    fn evict(&self) {
        let previous = self
            .current
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if previous.is_some() {
            tracing::trace!("evicted output lock plan");
        }
        drop(previous);
    }
}

#[cfg(test)]
mod tests {
    use std::fs::TryLockError;
    use std::time::Duration;

    use struo_core::{ResourceLimits, ResourceRequest};

    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_plan_publication_cannot_strand_full_budget_actions() {
        let workspace = tempfile::tempdir().unwrap();
        let resources = Arc::new(ResourcePool::new(ResourceLimits::new(4, 4096)));
        let cache = LockPlanCache::new(resources.clone());
        let mut tasks = Vec::new();
        for worker in 0..3 {
            let cache = cache.clone();
            let workspace = workspace.path().to_owned();
            tasks.push(tokio::spawn(async move {
                for iteration in 0..100 {
                    let outputs = BTreeMap::from([(
                        format!("out/{worker}/{}", iteration % 2),
                        Digest::of_bytes(b"output"),
                    )]);
                    drop(cache.open_shared(&workspace, &outputs).unwrap());
                    tokio::task::yield_now().await;
                }
            }));
        }
        tokio::time::timeout(Duration::from_secs(10), async {
            for _ in 0..100 {
                let permit = resources.acquire(ResourceRequest::new(4, 4096)).await;
                tokio::task::yield_now().await;
                drop(permit);
            }
            for task in tasks {
                task.await.unwrap();
            }
            let _permit = resources.acquire(ResourceRequest::new(4, 4096)).await;
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn reused_plans_keep_lock_owners_independent_and_evict_for_full_budget_actions() {
        let workspace = tempfile::tempdir().unwrap();
        let resources = Arc::new(ResourcePool::new(ResourceLimits::new(4, 1024 * 1024)));
        let cache = LockPlanCache::new(resources.clone());
        let digest = Digest::of_bytes(b"output");
        let outputs = BTreeMap::from([("out/tree/file".into(), digest)]);
        let first = cache.open_shared(workspace.path(), &outputs).unwrap();
        let second = cache.open_shared(workspace.path(), &outputs).unwrap();
        for lock in first.iter().chain(&second) {
            lock.try_lock().unwrap();
        }
        let writer = OutputLock::open_all(workspace.path(), digest, ["out/tree"]).unwrap();
        drop(first);
        let busy = writer.iter().find(|lock| lock.exclusive).unwrap();
        assert!(matches!(busy.try_lock(), Err(TryLockError::WouldBlock)));
        let full = tokio::time::timeout(
            Duration::from_secs(1),
            resources.acquire(ResourceRequest::new(4, 1024 * 1024)),
        )
        .await
        .unwrap();
        assert!(matches!(busy.try_lock(), Err(TryLockError::WouldBlock)));
        drop(second);
        busy.try_lock().unwrap();
        drop(writer);
        drop(full);
        std::fs::remove_dir_all(workspace.path().join(".struo/locks")).unwrap();
        for lock in cache.open_shared(workspace.path(), &outputs).unwrap() {
            lock.try_lock().unwrap();
        }
    }

    #[test]
    fn insufficient_budget_and_different_workspaces_still_use_the_correct_locks() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        for budget in [1, 1024 * 1024] {
            let cache =
                LockPlanCache::new(Arc::new(ResourcePool::new(ResourceLimits::new(4, budget))));
            let digest = Digest::of_bytes(b"output");
            let outputs = BTreeMap::from([("out/file".into(), digest)]);
            let readers = cache.open_shared(first.path(), &outputs).unwrap();
            for lock in &readers {
                lock.try_lock().unwrap();
            }
            let writer = OutputLock::open_all(second.path(), digest, ["out/file"]).unwrap();
            for lock in &writer {
                lock.try_lock().unwrap();
            }
            let other = cache.open_shared(second.path(), &outputs).unwrap();
            assert!(other
                .iter()
                .any(|lock| matches!(lock.try_lock(), Err(TryLockError::WouldBlock))));
        }
    }
}
