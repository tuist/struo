use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::{InfrastructureConfig, Result};

tokio::task_local! {
    static INFRASTRUCTURE: InfrastructureSnapshot;
}

/// Immutable workspace infrastructure for one command. Spawned tasks must
/// receive resolved configuration explicitly rather than relying on this scope.
pub struct InfrastructureSnapshot {
    root: PathBuf,
    config: Arc<InfrastructureConfig>,
}

impl InfrastructureSnapshot {
    pub fn load(root: &Path) -> Result<Self> {
        Ok(Self {
            root: root.to_path_buf(),
            config: Arc::new(crate::workspace::read_infrastructure_config(root)?),
        })
    }

    pub async fn scope<F: Future>(self, future: F) -> F::Output {
        INFRASTRUCTURE.scope(self, future).await
    }
}

pub(crate) fn current(root: &Path) -> Option<Arc<InfrastructureConfig>> {
    INFRASTRUCTURE
        .try_with(|snapshot| (snapshot.root == root).then(|| Arc::clone(&snapshot.config)))
        .ok()
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{load_infrastructure_config, resolve_cache_provider, ResolvedCacheProviderConfig};

    fn configure(root: &Path, name: &str) {
        std::fs::write(
            root.join("struo.toml"),
            format!("[infrastructure.execution]\nprovider = '{name}'\n"),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn invocation_snapshot_survives_edits_and_next_invocation_reloads() {
        let root = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        configure(root.path(), "first");
        configure(other.path(), "other");
        let first = InfrastructureSnapshot::load(root.path()).unwrap();
        configure(root.path(), "second");
        let second = InfrastructureSnapshot::load(root.path()).unwrap();
        std::fs::write(root.path().join("struo.toml"), "invalid = [").unwrap();
        let check = async |name: &'static str| {
            tokio::task::yield_now().await;
            assert_eq!(
                load_infrastructure_config(root.path())
                    .unwrap()
                    .execution
                    .as_ref()
                    .unwrap()
                    .name,
                name
            );
            assert_eq!(
                load_infrastructure_config(other.path())
                    .unwrap()
                    .execution
                    .as_ref()
                    .unwrap()
                    .name,
                "other"
            );
            assert_eq!(
                resolve_cache_provider(
                    root.path(),
                    &root.path().join("user.toml"),
                    Some("local".into())
                )
                .unwrap(),
                ResolvedCacheProviderConfig::Local
            );
        };
        tokio::join!(first.scope(check("first")), second.scope(check("second")));
        assert!(load_infrastructure_config(root.path()).is_err());
        assert!(InfrastructureSnapshot::load(root.path()).is_err());
        configure(root.path(), "third");
        InfrastructureSnapshot::load(root.path())
            .unwrap()
            .scope(check("third"))
            .await;
    }
}
