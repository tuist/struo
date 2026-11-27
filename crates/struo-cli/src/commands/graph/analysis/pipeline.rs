use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

use anyhow::{Context, Result};
use struo_cas::Digest;
use struo_frontend::analysis::{DeclaredAction, SharedProvider};
use tokio::sync::{mpsc, watch};

use super::{AvailableInput, BuildOutcome};

pub(super) struct EarlyTarget {
    pub target_id: String,
    pub provider: SharedProvider,
    pub available_inputs: BTreeMap<String, AvailableInput>,
}

struct Target {
    provider: SharedProvider,
    available_inputs: BTreeMap<String, AvailableInput>,
    outputs: BTreeMap<String, watch::Sender<Option<AvailableInput>>>,
    announced: bool,
}

pub(super) struct Pipeline {
    targets: Mutex<HashMap<String, Target>>,
    events: mpsc::Sender<EarlyTarget>,
}

impl Pipeline {
    pub fn new(capacity: usize) -> (Self, mpsc::Receiver<EarlyTarget>) {
        let (events, receiver) = mpsc::channel(capacity.max(1));
        (
            Self {
                targets: Mutex::new(HashMap::new()),
                events,
            },
            receiver,
        )
    }

    pub fn register(
        &self,
        id: &str,
        provider: SharedProvider,
        actions: &[DeclaredAction],
        available_inputs: &BTreeMap<String, AvailableInput>,
    ) -> Result<()> {
        if !actions
            .iter()
            .any(|action| !action.ready_outputs.is_empty())
        {
            return Ok(());
        }
        anyhow::ensure!(
            !actions.iter().any(|action| matches!(
                action.operation,
                Some(struo_frontend::analysis::DeclaredActionOperation::ExpandActions { .. })
            )),
            "ready-output producers cannot contain deferred action expansion"
        );
        for (index, action) in actions.iter().enumerate() {
            for ready in &action.ready_outputs {
                anyhow::ensure!(
                    !action.intermediate && action.operation.is_none(),
                    "ready outputs require a non-intermediate command action"
                );
                for (other_index, other) in actions.iter().enumerate() {
                    let writes = other
                        .outputs
                        .iter()
                        .chain(&other.clean_paths)
                        .chain(other.stdout.iter())
                        .chain(other.stderr.iter())
                        .chain(other.arg_files.iter().map(|file| &file.path));
                    for path in writes {
                        if other_index == index
                            && (path == &ready.path || other.clean_paths.contains(path))
                        {
                            continue;
                        }
                        let path = std::path::Path::new(path);
                        let ready_path = std::path::Path::new(&ready.path);
                        anyhow::ensure!(
                            !path.starts_with(ready_path) && !ready_path.starts_with(path),
                            "ready output `{}` overlaps another action write",
                            ready.path
                        );
                    }
                }
            }
        }
        let outputs = actions
            .iter()
            .flat_map(|action| &action.outputs)
            .map(|path| (path.clone(), watch::channel(None).0))
            .collect();
        self.targets.lock().unwrap().insert(
            id.to_string(),
            Target {
                provider,
                available_inputs: available_inputs.clone(),
                outputs,
                announced: false,
            },
        );
        Ok(())
    }

    pub async fn ready(&self, id: &str, path: &str, digest: Digest) -> Result<()> {
        let event = {
            let mut targets = self.targets.lock().unwrap();
            let target = targets
                .get_mut(id)
                .context("missing early-output producer")?;
            let output = target.outputs.get(path).context("unknown early output")?;
            output.send_replace(Some(AvailableInput {
                blob_digest: digest,
                same_target: false,
                materialized: true,
            }));
            if target.announced {
                None
            } else {
                target.announced = true;
                let mut available_inputs = std::mem::take(&mut target.available_inputs);
                available_inputs.insert(
                    path.to_string(),
                    AvailableInput {
                        blob_digest: digest,
                        same_target: false,
                        materialized: true,
                    },
                );
                Some(EarlyTarget {
                    target_id: id.to_string(),
                    provider: target.provider.clone(),
                    available_inputs,
                })
            }
        };
        if let Some(event) = event {
            self.events
                .send(event)
                .await
                .context("graph scheduler stopped")?;
        }
        Ok(())
    }

    pub async fn inputs<'a>(
        &self,
        owner: &str,
        paths: impl Iterator<Item = &'a str>,
    ) -> Result<BTreeMap<String, AvailableInput>> {
        let waiting = {
            let targets = self.targets.lock().unwrap();
            paths
                .flat_map(|path| {
                    targets
                        .iter()
                        .filter(|(id, _)| id.as_str() != owner)
                        .flat_map(|(_, target)| &target.outputs)
                        .filter(|(output, _)| {
                            let path = std::path::Path::new(path);
                            let output = std::path::Path::new(output);
                            path.starts_with(output) || output.starts_with(path)
                        })
                        .map(|(output, state)| (output.clone(), state.subscribe()))
                        .collect::<Vec<_>>()
                })
                .collect::<BTreeMap<_, _>>()
        };
        let mut inputs = BTreeMap::new();
        for (path, mut state) in waiting {
            loop {
                if let Some(input) = *state.borrow_and_update() {
                    inputs.insert(path, input);
                    break;
                }
                state
                    .changed()
                    .await
                    .context("output producer disappeared")?;
            }
        }
        Ok(inputs)
    }

    pub fn validate(&self, id: &str, outputs: &BTreeMap<String, Digest>) -> Result<()> {
        let targets = self.targets.lock().unwrap();
        if let Some(target) = targets.get(id) {
            for (path, state) in &target.outputs {
                if let Some(early) = *state.borrow() {
                    if let Some(digest) = outputs.get(path) {
                        anyhow::ensure!(
                            early.blob_digest == *digest,
                            "early output `{path}` changed after readiness"
                        );
                    }
                }
            }
        }
        Ok(())
    }

    pub fn complete(&self, id: &str, outcome: &BuildOutcome) -> Result<()> {
        self.validate(id, &outcome.result.outputs)?;
        let mut targets = self.targets.lock().unwrap();
        if let Some(target) = targets.get_mut(id) {
            target.available_inputs.clear();
            for (path, state) in &target.outputs {
                anyhow::ensure!(
                    state.borrow().is_none() || outcome.result.outputs.contains_key(path),
                    "ready output `{path}` missing at target completion"
                );
                if let Some(input) = outcome.available_inputs.get(path) {
                    let mut input = *input;
                    input.same_target = false;
                    if outcome.outputs.contains(path) {
                        input.materialized = true;
                    }
                    state.send_replace(Some(input));
                }
            }
        }
        Ok(())
    }
}
