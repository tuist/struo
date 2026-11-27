use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap, HashMap, HashSet};
use std::sync::Arc;

use anyhow::{Context, Result};
use struo_cas::Digest;
use struo_frontend::analysis::SharedProvider;
use struo_frontend::GraphTarget;

use super::ranking::critical_depths;
use super::{AvailableInput, BuildOutcome};

pub(in super::super) struct DependencyInputs {
    pub providers: Vec<SharedProvider>,
    pub providers_by_role: BTreeMap<String, Vec<SharedProvider>>,
    pub action_digests: Vec<(String, Digest)>,
    pub available_inputs: BTreeMap<String, AvailableInput>,
    pub early_dependencies: bool,
}

pub(super) struct BuildState {
    remaining_deps: HashMap<String, usize>,
    dependents: HashMap<String, Vec<String>>,
    critical_depths: HashMap<String, usize>,
    remaining_readers: HashMap<String, usize>,
    early: HashMap<String, super::super::pipeline::EarlyTarget>,
    released: HashSet<(String, String)>,
    pub(super) ready: BinaryHeap<(usize, Reverse<String>)>,
    pub(super) outcomes: HashMap<String, BuildOutcome>,
    pub(super) completed: usize,
}

impl BuildState {
    pub(super) fn new(
        targets: &HashMap<String, Arc<GraphTarget>>,
        reachable: &HashSet<String>,
        retained: &HashSet<String>,
    ) -> Result<Self> {
        let mut remaining_deps = HashMap::new();
        let mut dependents: HashMap<String, Vec<String>> = HashMap::new();
        for target_id in reachable {
            let target = targets
                .get(target_id)
                .with_context(|| format!("target `{target_id}` vanished from graph"))?;
            let mut dep_count = 0;
            for dep_id in target
                .dependency_ids()
                .filter(|dep_id| reachable.contains(*dep_id))
            {
                dep_count += 1;
                dependents
                    .entry(dep_id.clone())
                    .or_default()
                    .push(target_id.clone());
            }
            remaining_deps.insert(target_id.clone(), dep_count);
        }

        let mut remaining_readers = dependents
            .iter()
            .map(|(target_id, deps)| (target_id.clone(), deps.len()))
            .collect::<HashMap<_, _>>();
        for target_id in retained {
            *remaining_readers.entry(target_id.clone()).or_default() += 1;
        }

        let critical_depths = critical_depths(reachable, &dependents)?;
        let ready = remaining_deps
            .iter()
            .filter(|(_, count)| **count == 0)
            .map(|(target_id, _)| {
                (
                    critical_depths.get(target_id).copied().unwrap_or(1),
                    Reverse(target_id.clone()),
                )
            })
            .collect();

        Ok(Self {
            remaining_deps,
            dependents,
            critical_depths,
            remaining_readers,
            early: HashMap::new(),
            released: HashSet::new(),
            ready,
            outcomes: HashMap::new(),
            completed: 0,
        })
    }

    pub(super) fn record_completion(
        &mut self,
        target_id: &str,
        outcome: BuildOutcome,
    ) -> Result<()> {
        self.early.remove(target_id);
        self.outcomes.insert(target_id.to_string(), outcome);
        self.completed += 1;
        self.release_dependents(target_id, None)
    }

    pub(super) fn record_early(
        &mut self,
        event: super::super::pipeline::EarlyTarget,
        eligible: &HashSet<String>,
    ) -> Result<()> {
        if self.outcomes.contains_key(&event.target_id) {
            return Ok(());
        }
        let target_id = event.target_id.clone();
        self.early.insert(target_id.clone(), event);
        self.release_dependents(&target_id, Some(eligible))
    }

    fn release_dependents(
        &mut self,
        target_id: &str,
        eligible: Option<&HashSet<String>>,
    ) -> Result<()> {
        let mut released = Vec::new();
        if let Some(next_targets) = self.dependents.get(target_id) {
            for next_id in next_targets {
                if eligible.is_some_and(|eligible| !eligible.contains(next_id))
                    || self
                        .released
                        .contains(&(target_id.to_string(), next_id.clone()))
                {
                    continue;
                }
                released.push((target_id.to_string(), next_id.clone()));
                let remaining = self
                    .remaining_deps
                    .get_mut(next_id)
                    .with_context(|| format!("missing dependency count for `{next_id}`"))?;
                *remaining -= 1;
                if *remaining == 0 {
                    self.ready.push((
                        self.critical_depths.get(next_id).copied().unwrap_or(1),
                        Reverse(next_id.clone()),
                    ));
                }
            }
        }
        self.released.extend(released);

        Ok(())
    }

    pub(super) fn dependency_inputs(
        &mut self,
        target: &GraphTarget,
        reachable: &HashSet<String>,
    ) -> Result<DependencyInputs> {
        let early_dependencies = target
            .dependency_ids()
            .any(|id| self.early.contains_key(id));
        let mut providers = Vec::new();
        let mut providers_by_role = BTreeMap::new();
        let mut action_digests = Vec::new();
        let mut available_inputs = BTreeMap::new();
        for dep_id in target
            .deps
            .iter()
            .filter(|dep_id| reachable.contains(*dep_id))
        {
            let (provider, action_digest, inputs) = self.read_dependency(dep_id)?;
            providers.push(provider);
            action_digests.push((dep_id.clone(), action_digest));
            available_inputs.extend(inputs.into_iter().map(|(path, mut input)| {
                input.same_target = false;
                (path, input)
            }));
        }
        for (role, dep_ids) in &target.dependency_edges {
            let mut role_providers = Vec::new();
            for dep_id in dep_ids.iter().filter(|dep_id| reachable.contains(*dep_id)) {
                let (provider, action_digest, inputs) = self.read_dependency(dep_id)?;
                role_providers.push(provider);
                action_digests.push((dep_id.clone(), action_digest));
                available_inputs.extend(inputs.into_iter().map(|(path, mut input)| {
                    input.same_target = false;
                    (path, input)
                }));
            }
            providers_by_role.insert(role.clone(), role_providers);
        }
        Ok(DependencyInputs {
            providers,
            providers_by_role,
            action_digests,
            available_inputs,
            early_dependencies,
        })
    }

    fn read_dependency(
        &mut self,
        dep_id: &str,
    ) -> Result<(SharedProvider, Digest, BTreeMap<String, AvailableInput>)> {
        let remaining = self
            .remaining_readers
            .get_mut(dep_id)
            .with_context(|| format!("missing reader count for `{dep_id}`"))?;
        *remaining = remaining
            .checked_sub(1)
            .with_context(|| format!("dependency `{dep_id}` had no remaining readers"))?;

        if let Some(event) = self.early.get(dep_id) {
            return Ok((
                event.provider.clone(),
                Digest::of_bytes(dep_id.as_bytes()),
                event.available_inputs.clone(),
            ));
        }
        if *remaining == 0 {
            let outcome = self
                .outcomes
                .remove(dep_id)
                .with_context(|| format!("missing build outcome for dependency `{dep_id}`"))?;
            Ok((
                outcome.provider,
                outcome.action_digest,
                outcome.available_inputs,
            ))
        } else {
            let outcome = self
                .outcomes
                .get(dep_id)
                .with_context(|| format!("missing build outcome for dependency `{dep_id}`"))?;
            Ok((
                outcome.provider.clone(),
                outcome.action_digest,
                outcome.available_inputs.clone(),
            ))
        }
    }
}

#[cfg(test)]
mod tests;
