use std::collections::BTreeMap;
use std::sync::{Arc, PoisonError, Weak};

use futures::FutureExt;

use super::{Reservation, ResourcePermit, ResourcePool, ResourceRequest};

/// Optional storage that releases its idle memory reservations under pressure.
pub trait EvictableCache: std::fmt::Debug + Send + Sync {
    /// Drop cache-owned reservations without waiting for admission or readers.
    fn evict(&self);
}

impl ResourcePool {
    /// Register before reserving cache memory. Registration does not own the cache.
    pub fn register_cache(&self, cache: &Arc<dyn EvictableCache>) {
        let mut caches = self.caches.lock().unwrap_or_else(PoisonError::into_inner);
        caches.retain(|cache| cache.strong_count() != 0);
        caches.push(Arc::downgrade(cache));
    }

    /// Reserve exact optional memory plus reservation bookkeeping, without CPUs,
    /// waiting, clamping, or evicting other entries. Publish while holding the
    /// cache's eviction mutex so admission cannot miss a newly retained entry.
    /// Readers must not retain these reservations while waiting for admission.
    pub fn try_reserve_memory(self: &Arc<Self>, bytes: u64) -> Option<ResourcePermit> {
        let overhead = size_of::<Reservation>() + 2 * size_of::<usize>();
        let memory_bytes = bytes.checked_add(overhead as u64)?;
        if self.limits.memory_bytes != 0 && memory_bytes > self.limits.memory_bytes {
            return None;
        }
        self.admit(
            ResourceRequest {
                cpu_slots: 0,
                memory_bytes,
                slots: BTreeMap::new(),
            },
            false,
        )
        .now_or_never()
    }

    pub(super) fn evict_caches(&self) {
        let caches: Vec<_> = self
            .caches
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter_map(Weak::upgrade)
            .collect();
        for cache in caches {
            cache.evict();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::Duration;

    use crate::ResourceLimits;

    use super::*;

    #[derive(Debug, Default)]
    struct Cache(Mutex<Option<ResourcePermit>>);

    impl EvictableCache for Cache {
        fn evict(&self) {
            self.0.lock().unwrap().take();
        }
    }

    #[tokio::test]
    async fn optional_memory_does_not_reduce_action_concurrency_or_block_full_budget_work() {
        let pool = Arc::new(ResourcePool::new(ResourceLimits::new(4, 4096)));
        let cache = Arc::new(Cache::default());
        pool.register_cache(&(cache.clone() as Arc<dyn EvictableCache>));
        *cache.0.lock().unwrap() = Some(pool.try_reserve_memory(512).unwrap());
        let mut actions = Vec::new();
        for _ in 0..4 {
            actions.push(
                tokio::time::timeout(
                    Duration::from_secs(1),
                    pool.acquire(ResourceRequest::new(1, 1024)),
                )
                .await
                .unwrap(),
            );
        }
        assert!(cache.0.lock().unwrap().is_none());
        assert!(pool.try_reserve_memory(1).is_none());
        drop(actions);
        *cache.0.lock().unwrap() = Some(pool.try_reserve_memory(512).unwrap());
        let full = tokio::time::timeout(
            Duration::from_secs(1),
            pool.acquire(ResourceRequest::new(4, 4096)),
        )
        .await
        .unwrap();
        drop(full);
        assert!(pool.is_idle());
    }

    #[tokio::test]
    async fn active_readers_keep_their_charge_and_optional_requests_do_not_jump_the_queue() {
        let pool = Arc::new(ResourcePool::new(ResourceLimits::new(2, 4096)));
        let cache = Arc::new(Cache::default());
        pool.register_cache(&(cache.clone() as Arc<dyn EvictableCache>));
        let reader = pool.try_reserve_memory(512).unwrap();
        *cache.0.lock().unwrap() = Some(reader.clone());
        let mut full = Box::pin(pool.acquire(ResourceRequest::new(1, 4096)));
        assert!(full.as_mut().now_or_never().is_none());
        assert!(cache.0.lock().unwrap().is_none());
        assert!(pool.try_reserve_memory(1).is_none());
        drop(reader);
        let full = tokio::time::timeout(Duration::from_secs(1), full)
            .await
            .unwrap();
        drop(full);
        assert!(pool.try_reserve_memory(4097).is_none());
        assert!(pool.is_idle());
    }

    #[tokio::test]
    async fn queued_named_slot_work_cannot_hide_memory_pressure() {
        let pool = Arc::new(ResourcePool::new(ResourceLimits::new(2, 4096)));
        let cache = Arc::new(Cache::default());
        pool.register_cache(&(cache.clone() as Arc<dyn EvictableCache>));
        *cache.0.lock().unwrap() = Some(pool.try_reserve_memory(512).unwrap());
        let gate = pool
            .acquire(ResourceRequest::new(1, 1).with_slot("gate", 1))
            .await;
        let mut first = Box::pin(pool.acquire(ResourceRequest::new(1, 2048).with_slot("gate", 1)));
        assert!(first.as_mut().now_or_never().is_none());
        let mut second = Box::pin(pool.acquire(ResourceRequest::new(1, 2048)));
        assert!(second.as_mut().now_or_never().is_none());
        drop(gate);
        let permits = tokio::time::timeout(Duration::from_secs(1), async {
            tokio::join!(first, second)
        })
        .await
        .unwrap();
        drop(permits);
        assert!(pool.is_idle());
    }
}
