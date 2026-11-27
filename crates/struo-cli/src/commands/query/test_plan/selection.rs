use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;

use anyhow::Result;
use struo_core::{TestSelectionPolicy, TestSelectionReport, WorkspacePath, TEST_SELECTION_SCHEMA};
use struo_frontend::{AttrValue, GraphTarget};

use super::inputs::{
    is_graph_input, target_input_patterns, workspace_graph_input_patterns, TargetInputs,
};
use super::mentions::mention_owners;
use super::AffectedTestRecord;

#[allow(clippy::too_many_lines)]
pub(super) fn selection_report(
    workspace: &Path,
    graph: &[GraphTarget],
    changed_paths: &[String],
) -> Result<TestSelectionReport> {
    let changed_paths = normalize_changed_paths(changed_paths);
    if changed_paths.is_empty() {
        return Ok(TestSelectionReport {
            schema: TEST_SELECTION_SCHEMA.to_string(),
            policy: TestSelectionPolicy {
                mode: "full".to_string(),
                safety: "exact".to_string(),
                evidence: "complete_test_scope".to_string(),
            },
            changed_paths,
            unmatched_paths: Vec::new(),
            tests: all_tests(graph, "no changed paths supplied; include test target"),
        });
    }

    let graph_inputs = workspace_graph_input_patterns(workspace)?;
    let target_patterns = target_input_patterns(workspace, graph);
    let reverse_dependencies = reverse_dependencies(graph);
    let targets = graph
        .iter()
        .map(|target| (target.label.id.as_str(), target))
        .collect::<BTreeMap<_, _>>();
    let mut reasons = BTreeMap::<String, BTreeSet<String>>::new();
    let mut unmatched_paths = Vec::new();

    let owners_of = |path: &str| {
        target_patterns
            .iter()
            .filter_map(|(target_id, inputs)| inputs.matches(path).then_some(target_id.as_str()))
            .collect::<Vec<_>>()
    };
    // A path a target leaves out of its inputs may still be one its sources
    // name, so those are searched for together, reading each file once.
    let unread = changed_paths
        .iter()
        .filter(|path| !is_graph_input(path, &graph_inputs))
        .filter(|path| {
            target_patterns
                .values()
                .any(|inputs| inputs.excludes_from_srcs(path))
        })
        .cloned()
        .collect::<Vec<_>>();
    let mut mentioned = mention_owners(workspace, &target_patterns, &unread);

    for path in &changed_paths {
        if is_graph_input(path, &graph_inputs) {
            add_full_test_scope(
                graph,
                &format!("changed graph definition `{path}`; include test target"),
                &mut reasons,
            );
            continue;
        }

        add_provider_selections(
            path,
            &target_patterns,
            &targets,
            &reverse_dependencies,
            &mut reasons,
        );
        let mut owners = owners_of(path);
        if unread.contains(path) {
            // A target took this path out of its inputs, such as a build
            // tool's output directory or a file outside the code its resolver
            // narrowed it to, so of those targets only one whose sources name
            // the path reads it.
            for owner in mentioned.remove(path).unwrap_or_default() {
                if !owners.contains(&owner) {
                    owners.push(owner);
                }
            }
            if owners.is_empty() {
                continue;
            }
        }
        if owners.is_empty() {
            owners = nearest_package_owners(path, graph);
        }
        if owners.is_empty() {
            // Deliberately conservative: a path no target claims may still affect
            // any test (for example documentation compiled into doc-tests), so we
            // select the whole test scope rather than risk skipping a regression.
            // The path is also recorded in `unmatched_paths` so callers can see why.
            unmatched_paths.push(path.clone());
            add_full_test_scope(
                graph,
                &format!("changed path `{path}` has no declared owner; include test target"),
                &mut reasons,
            );
            continue;
        }

        add_owner_tests(path, &owners, &targets, &reverse_dependencies, &mut reasons);
    }

    let tests = graph
        .iter()
        .filter(|target| has_capability(target, "test"))
        .filter_map(|target| {
            reasons
                .remove(&target.label.id)
                .map(|reasons| AffectedTestRecord {
                    id: target.label.id.clone(),
                    kind: target.kind.clone(),
                    reasons: reasons.into_iter().collect(),
                })
        })
        .collect();

    Ok(TestSelectionReport {
        schema: TEST_SELECTION_SCHEMA.to_string(),
        policy: TestSelectionPolicy {
            mode: "affected".to_string(),
            safety: "conservative".to_string(),
            evidence: "declared_graph_and_package_ownership".to_string(),
        },
        changed_paths,
        unmatched_paths,
        tests,
    })
}

pub(super) fn default_selection_report(
    graph: &[GraphTarget],
    resolver_kinds: &BTreeSet<String>,
) -> TestSelectionReport {
    let mut has_declared_test_roots = false;
    let declared_test_roots = graph
        .iter()
        .filter(|target| resolver_kinds.contains(&target.kind))
        .flat_map(|target| {
            let roots = default_test_roots(target);
            has_declared_test_roots |= roots.is_some();
            roots.unwrap_or_default()
        })
        .collect::<BTreeSet<_>>();
    if has_declared_test_roots {
        return TestSelectionReport {
            schema: TEST_SELECTION_SCHEMA.to_string(),
            policy: TestSelectionPolicy {
                mode: "default".to_string(),
                safety: "exact".to_string(),
                evidence: "resolver_test_roots".to_string(),
            },
            changed_paths: Vec::new(),
            unmatched_paths: Vec::new(),
            tests: graph
                .iter()
                .filter(|target| declared_test_roots.contains(&target.label.id))
                .filter(|target| has_capability(target, "test"))
                .map(|target| AffectedTestRecord {
                    id: target.label.id.clone(),
                    kind: target.kind.clone(),
                    reasons: vec![
                        "first-party test reported by a discovered workspace root".to_string()
                    ],
                })
                .collect(),
        };
    }
    let targets = graph
        .iter()
        .map(|target| (target.label.id.as_str(), target))
        .collect::<BTreeMap<_, _>>();
    let roots = graph
        .iter()
        .filter(|target| resolver_kinds.contains(&target.kind))
        .flat_map(GraphTarget::dependency_ids)
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let mut tests = graph
        .iter()
        .filter(|target| has_capability(target, "test"))
        .filter(|target| {
            resolver_kinds.contains(&target.kind) || depends_on_any_root(target, &targets, &roots)
        })
        .map(|target| AffectedTestRecord {
            id: target.label.id.clone(),
            kind: target.kind.clone(),
            reasons: vec!["belongs to a discovered workspace root".to_string()],
        })
        .collect::<Vec<_>>();
    if roots.is_empty() {
        tests = all_tests(graph, "no discovered workspace root; include test target");
    }
    TestSelectionReport {
        schema: TEST_SELECTION_SCHEMA.to_string(),
        policy: TestSelectionPolicy {
            mode: "default".to_string(),
            safety: "exact".to_string(),
            evidence: "discovered_workspace_roots".to_string(),
        },
        changed_paths: Vec::new(),
        unmatched_paths: Vec::new(),
        tests,
    }
}

/// The first-party test target ids a resolver seed reports through
/// `_default_test_roots`, resolved inside the seed's package.
fn default_test_roots(seed: &GraphTarget) -> Option<Vec<String>> {
    let Some(AttrValue::List(values)) = seed.attrs.get("_default_test_roots") else {
        return None;
    };
    let roots = values.iter().filter_map(AttrValue::as_str).map(|name| {
        if seed.label.package.is_empty() {
            name.to_string()
        } else {
            format!("{}/{}", seed.label.package, name)
        }
    });
    Some(roots.collect())
}

fn depends_on_any_root<'a>(
    target: &'a GraphTarget,
    targets: &BTreeMap<&'a str, &'a GraphTarget>,
    roots: &BTreeSet<&'a str>,
) -> bool {
    let mut pending = VecDeque::from([target.label.id.as_str()]);
    let mut visited = BTreeSet::new();
    while let Some(target_id) = pending.pop_front() {
        if roots.contains(target_id) {
            return true;
        }
        if !visited.insert(target_id) {
            continue;
        }
        if let Some(target) = targets.get(target_id) {
            pending.extend(target.dependency_ids().map(String::as_str));
        }
    }
    false
}

fn nearest_package_owners<'a>(path: &str, graph: &'a [GraphTarget]) -> Vec<&'a str> {
    let manifest_suffix = format!("/{}", struo_frontend::TOML_BUILD_FILE_NAME);
    let path_package = path
        .strip_suffix(&manifest_suffix)
        .unwrap_or_else(|| path.rsplit_once('/').map_or("", |(package, _)| package));
    let longest = graph
        .iter()
        .filter(|target| package_contains_path(&target.label.package, path_package))
        .map(|target| target.label.package.len())
        .max();
    let Some(longest) = longest else {
        return Vec::new();
    };
    graph
        .iter()
        .filter(|target| {
            target.label.package.len() == longest
                && package_contains_path(&target.label.package, path_package)
        })
        .map(|target| target.label.id.as_str())
        .collect()
}

fn package_contains_path(package: &str, path_package: &str) -> bool {
    if package.is_empty() {
        return path_package.is_empty();
    }
    path_package == package
        || path_package
            .strip_prefix(package)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn normalize_changed_paths(paths: &[String]) -> Vec<String> {
    // Normalize to workspace-relative form when possible, but tolerate inputs that
    // cannot be (absolute paths, `..`, Windows separators) rather than failing the
    // whole command. A path we cannot normalize simply won't match a declared
    // owner and falls through to conservative selection.
    let mut normalized = paths
        .iter()
        .map(|path| {
            WorkspacePath::try_from(path.as_str()).map_or_else(
                |_| path.trim_start_matches("./").to_string(),
                |workspace_path| workspace_path.to_string(),
            )
        })
        .collect::<Vec<_>>();
    normalized.sort();
    normalized.dedup();
    normalized
}

fn all_tests(graph: &[GraphTarget], reason: &str) -> Vec<AffectedTestRecord> {
    graph
        .iter()
        .filter(|target| has_capability(target, "test"))
        .map(|target| AffectedTestRecord {
            id: target.label.id.clone(),
            kind: target.kind.clone(),
            reasons: vec![reason.to_string()],
        })
        .collect()
}

fn add_full_test_scope(
    graph: &[GraphTarget],
    reason: &str,
    reasons: &mut BTreeMap<String, BTreeSet<String>>,
) {
    for test in graph.iter().filter(|target| has_capability(target, "test")) {
        reasons
            .entry(test.label.id.clone())
            .or_default()
            .insert(reason.to_string());
    }
}

// A resolver seed's own sources are the native manifest it derives its targets
// from. When none of those targets claims the manifest itself, a change to it
// selects the first-party tests the seed reports, since it can change any of
// them.
fn add_owner_tests(
    path: &str,
    owners: &[&str],
    targets: &BTreeMap<&str, &GraphTarget>,
    reverse_dependencies: &BTreeMap<String, Vec<&str>>,
    reasons: &mut BTreeMap<String, BTreeSet<String>>,
) {
    let seed_roots = owners
        .iter()
        .map(|owner| {
            targets
                .get(owner)
                .and_then(|target| default_test_roots(target))
        })
        .collect::<Option<Vec<_>>>();
    for test in seed_roots.into_iter().flatten().flatten() {
        reasons.entry(test).or_default().insert(format!(
            "changed workspace manifest `{path}` of `{}`",
            owners.join("`, `")
        ));
    }
    for owner in owners {
        add_affected_tests(path, owner, targets, reverse_dependencies, reasons);
    }
}

/// Selects the targets whose metadata provider reports `path` among the files
/// their runs read, in addition to the owners `selection_report` finds.
fn add_provider_selections(
    path: &str,
    target_patterns: &BTreeMap<String, TargetInputs>,
    targets: &BTreeMap<&str, &GraphTarget>,
    reverse_dependencies: &BTreeMap<String, Vec<&str>>,
    reasons: &mut BTreeMap<String, BTreeSet<String>>,
) {
    for (target_id, inputs) in target_patterns {
        if inputs.provider_matches(path) {
            add_affected_tests(path, target_id, targets, reverse_dependencies, reasons);
        }
    }
}

fn add_affected_tests(
    path: &str,
    owner: &str,
    targets: &BTreeMap<&str, &GraphTarget>,
    reverse_dependencies: &BTreeMap<String, Vec<&str>>,
    reasons: &mut BTreeMap<String, BTreeSet<String>>,
) {
    let mut queue = VecDeque::from([owner]);
    let mut visited = BTreeSet::new();
    while let Some(target_id) = queue.pop_front() {
        if !visited.insert(target_id) {
            continue;
        }
        if let Some(target) = targets.get(target_id) {
            if has_capability(target, "test") {
                let reason = if target_id == owner {
                    format!("changed test input `{path}`")
                } else {
                    format!("changed dependency `{owner}` input `{path}`")
                };
                reasons
                    .entry(target.label.id.clone())
                    .or_default()
                    .insert(reason);
            }
        }
        if let Some(dependents) = reverse_dependencies.get(target_id) {
            queue.extend(dependents.iter().copied());
        }
    }
}

fn reverse_dependencies(graph: &[GraphTarget]) -> BTreeMap<String, Vec<&str>> {
    let mut reverse = BTreeMap::<String, Vec<&str>>::new();
    for target in graph {
        // Include named dependency roles, not just the legacy `deps` role, so a
        // changed input reaches tests that depend on it through any role.
        // `_affected_deps` names targets, relative to this target's package,
        // whose changes reach its tests without being build prerequisites,
        // such as a native project graph's dependency on another project.
        let affected_deps = match target.attrs.get("_affected_deps") {
            Some(AttrValue::List(values)) => values
                .iter()
                .filter_map(AttrValue::as_str)
                .map(|name| {
                    let name = name.strip_prefix("./").unwrap_or(name);
                    if target.label.package.is_empty() {
                        name.to_string()
                    } else {
                        format!("{}/{name}", target.label.package)
                    }
                })
                .collect(),
            _ => Vec::new(),
        };
        for dependency in target.dependency_ids().cloned().chain(affected_deps) {
            reverse
                .entry(dependency)
                .or_default()
                .push(target.label.id.as_str());
        }
    }
    reverse
}

fn has_capability(target: &GraphTarget, name: &str) -> bool {
    target
        .capabilities
        .iter()
        .any(|capability| capability.name == name)
}

#[cfg(test)]
mod tests;
