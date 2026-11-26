use anyhow::{Context, Result};
use struo_cas::Digest;
use struo_core::Action;

use super::{lock, Producers};

/// Compressed rather than cloned: an action carries its flattened input closure
/// and recovery needs it only rarely.
pub(in crate::commands::graph::analysis) struct StoredAction {
    pub(super) digest: Digest,
    pub(super) log_paths: Vec<String>,
    json: Box<[u8]>,
}

impl Producers {
    pub(in crate::commands::graph::analysis) fn store_action(
        &self,
        action: &Action,
        digest: Digest,
    ) -> Result<StoredAction> {
        let mut context = lock(&self.inner.snapshot_compressor)
            .take()
            .unwrap_or_default();
        let stored = StoredAction::new(action, digest, &mut context);
        // Keep one idle context per invocation, without serializing concurrent
        // snapshots or retaining a context for every worker's lifetime.
        let mut idle = lock(&self.inner.snapshot_compressor);
        if idle.is_none() {
            *idle = Some(context);
        }
        stored
    }
}

impl StoredAction {
    fn new(
        action: &Action,
        digest: Digest,
        context: &mut zstd::zstd_safe::CCtx<'static>,
    ) -> Result<Self> {
        let mut encoder = zstd::stream::write::Encoder::with_context(Vec::new(), context);
        encoder.set_parameter(zstd::zstd_safe::CParameter::CompressionLevel(1))?;
        serde_json::to_writer(&mut encoder, action)?;
        let log_paths = match action {
            Action::RunCommand {
                stdout_path,
                stderr_path,
                ..
            } => stdout_path
                .iter()
                .chain(stderr_path)
                .map(|path| path.as_str().to_string())
                .collect(),
            _ => Vec::new(),
        };
        Ok(Self {
            digest,
            log_paths,
            json: encoder.finish()?.into_boxed_slice(),
        })
    }

    pub(super) fn load(&self) -> Result<Action> {
        let action: Action =
            serde_json::from_reader(zstd::stream::read::Decoder::new(&*self.json)?)
                .with_context(|| format!("reading back action {}", self.digest))?;
        anyhow::ensure!(
            action.digest() == self.digest,
            "action {} read back with a different digest",
            self.digest
        );
        Ok(action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshots_remain_independent_after_context_reuse_and_registry_drop() {
        let producers = Producers::default();
        let actions = [0, 1, 131_073, 17].map(|length| Action::WriteFile {
            path: struo_core::WorkspacePath::try_from(format!("out/{length}")).unwrap(),
            bytes: (0_u8..251).cycle().take(length).collect::<Vec<_>>().into(),
            input_digest: None,
        });
        let snapshots = actions
            .each_ref()
            .map(|action| producers.store_action(action, action.digest()).unwrap());
        drop(producers);
        for (action, snapshot) in actions.iter().zip(&snapshots).rev() {
            assert_eq!(snapshot.load().unwrap(), *action);
        }
        let producers = Producers::default();
        std::thread::scope(|scope| {
            for action in &actions {
                let producers = &producers;
                scope.spawn(move || {
                    for _ in 0..8 {
                        let snapshot = producers.store_action(action, action.digest()).unwrap();
                        assert_eq!(snapshot.load().unwrap(), *action);
                    }
                });
            }
        });
    }
}
