use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use struo_core::WorkspacePath;
use struo_frontend::{AttrValue, GraphTarget};

pub(super) fn target_input_patterns(
    workspace: &Path,
    graph: &[GraphTarget],
) -> BTreeMap<String, TargetInputs> {
    let analyzer = struo_frontend::analysis::AnalysisEngine::for_workspace(workspace).ok();
    graph
        .iter()
        .map(|target| {
            let package_patterns = |patterns: &[String]| {
                patterns
                    .iter()
                    .filter_map(|pattern| {
                        InputPattern::package_relative(&target.label.package, pattern)
                    })
                    .collect::<Vec<_>>()
            };
            let string_list = |name: &str| match target.attrs.get(name) {
                Some(AttrValue::List(values)) => values
                    .iter()
                    .filter_map(AttrValue::as_str)
                    .map(str::to_string)
                    .collect(),
                _ => Vec::new(),
            };
            let srcs_exclude = string_list("srcs_exclude");
            // A build script's declared data reruns it wherever the data lives,
            // including inside another package's tree, and a literal path names
            // a directory as readily as a file.
            let build_script_data = string_list("build_script_data")
                .into_iter()
                .flat_map(|pattern| {
                    let subtree = (!has_glob_metacharacters(&pattern))
                        .then(|| format!("{}/**", pattern.trim_end_matches('/')));
                    std::iter::once(pattern).chain(subtree)
                })
                .collect::<Vec<_>>();
            let provider_inputs = metadata_provider(analyzer.as_ref(), workspace, target)
                .map(|provider| {
                    provider_string_list(&provider, "affected_inputs")
                        .into_iter()
                        .filter_map(|pattern| InputPattern::workspace_relative(&pattern))
                        .collect()
                })
                .unwrap_or_default();
            let affected_srcs = target
                .attrs
                .contains_key("_affected_srcs")
                .then(|| package_patterns(&string_list("_affected_srcs")));
            let mentions = string_list("_affected_mentions");
            let mentions_exclude = string_list("_affected_mentions_exclude");
            let excluded_trees = srcs_exclude
                .iter()
                .chain(&mentions_exclude)
                .filter_map(|pattern| pattern.strip_suffix("/**"))
                .filter_map(|tree| InputPattern::package_relative(&target.label.package, tree))
                .collect();
            let inputs = TargetInputs {
                srcs: package_patterns(&target.srcs),
                affected_srcs,
                srcs_exclude: package_patterns(&srcs_exclude),
                excluded_trees,
                build_script_data: package_patterns(&build_script_data),
                provider_inputs,
                mention_root: mention_root(&target.label.package, &mentions),
                mentions: package_patterns(&mentions),
                mentions_exclude: package_patterns(&mentions_exclude),
            };
            (target.label.id.clone(), inputs)
        })
        .collect()
}

/// The paths a target declares as its inputs: its `srcs`, or the
/// `_affected_srcs` its resolver narrowed them to, without the paths its
/// `srcs_exclude` attribute removes, plus its `build_script_data` and the
/// `affected_inputs` its metadata provider reports. A path these leave out is
/// still an input when one of its `_affected_mentions` files names it.
#[derive(Debug)]
pub(super) struct TargetInputs {
    /// The directory every `_affected_mentions` glob lies under, from which
    /// a mention may also name a path.
    pub(super) mention_root: String,
    srcs: Vec<InputPattern>,
    affected_srcs: Option<Vec<InputPattern>>,
    srcs_exclude: Vec<InputPattern>,
    /// Directories `srcs_exclude` or `_affected_mentions_exclude` removes
    /// whole, which a search for mentions never enters.
    excluded_trees: Vec<InputPattern>,
    build_script_data: Vec<InputPattern>,
    provider_inputs: Vec<InputPattern>,
    mentions: Vec<InputPattern>,
    mentions_exclude: Vec<InputPattern>,
}

impl TargetInputs {
    pub(super) fn matches(&self, path: &str) -> bool {
        (self.reads(path) && !self.excludes(path))
            || self
                .build_script_data
                .iter()
                .chain(&self.provider_inputs)
                .any(|pattern| pattern.matches(path))
    }

    /// Whether `path` is one of this target's `srcs` that it does not read:
    /// one `srcs_exclude` removes, or one outside its `_affected_srcs`.
    pub(super) fn excludes_from_srcs(&self, path: &str) -> bool {
        self.in_srcs(path) && !self.matches(path)
    }

    /// Whether a search for mentions of changed paths covers `file`.
    pub(super) fn searches(&self, file: &str) -> bool {
        self.mentions.iter().any(|pattern| pattern.matches(file))
            && !self
                .mentions_exclude
                .iter()
                .any(|pattern| pattern.matches(file))
            && !self.excludes(file)
    }

    pub(super) fn has_mentions(&self) -> bool {
        !self.mentions.is_empty()
    }

    /// Whether a search for mentions skips `directory`.
    pub(super) fn excludes_tree(&self, directory: &str) -> bool {
        self.excluded_trees
            .iter()
            .any(|tree| tree.matches(directory))
    }

    /// Targets with the same mention root and excluded trees walk the same
    /// directories.
    pub(super) fn walk_key(&self) -> (String, Vec<String>) {
        let trees = self
            .excluded_trees
            .iter()
            .map(|tree| tree.as_str().to_string());
        (self.mention_root.clone(), trees.collect())
    }

    fn reads(&self, path: &str) -> bool {
        self.affected_srcs.as_ref().map_or_else(
            || self.in_srcs(path),
            |patterns| patterns.iter().any(|pattern| pattern.matches(path)),
        )
    }

    fn in_srcs(&self, path: &str) -> bool {
        self.srcs.iter().any(|pattern| pattern.matches(path))
    }

    fn excludes(&self, path: &str) -> bool {
        self.srcs_exclude
            .iter()
            .any(|pattern| pattern.matches(path))
    }
}

/// The workspace-relative directory the literal parts of `patterns` share.
fn mention_root(package: &str, patterns: &[String]) -> String {
    let roots = patterns
        .iter()
        .map(|pattern| {
            let literal = pattern
                .trim_start_matches("./")
                .split('/')
                .take_while(|part| !has_glob_metacharacters(part))
                .collect::<Vec<_>>();
            // The last literal part may name a file rather than a directory.
            let literal = if has_glob_metacharacters(pattern) {
                literal.join("/")
            } else {
                literal[..literal.len().saturating_sub(1)].join("/")
            };
            [package, literal.as_str()]
                .into_iter()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join("/")
        })
        .collect::<Vec<_>>();
    let Some(first) = roots.first() else {
        return String::new();
    };
    let mut shared = first.split('/').collect::<Vec<_>>();
    for root in &roots[1..] {
        let parts = root.split('/').collect::<Vec<_>>();
        let common = shared
            .iter()
            .zip(&parts)
            .take_while(|(left, right)| left == right)
            .count();
        shared.truncate(common);
    }
    shared.join("/")
}

pub(super) fn workspace_graph_input_patterns(workspace: &Path) -> Result<Vec<InputPattern>> {
    let manifest_path = workspace.join(struo_frontend::TOML_BUILD_FILE_NAME);
    let source = match std::fs::read_to_string(&manifest_path) {
        Ok(source) => source,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).context("reading workspace manifest"),
    };
    let value: toml::Value = toml::from_str(&source).context("parsing workspace manifest")?;
    let module_patterns = value
        .get("modules")
        .or_else(|| value.get("rules"))
        .and_then(|modules| modules.get("paths"))
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(toml::Value::as_str)
        .filter_map(InputPattern::workspace_relative);
    Ok(std::iter::once(InputPattern::Exact(
        struo_frontend::TOML_BUILD_FILE_NAME.to_string(),
    ))
    .chain(module_patterns)
    .collect())
}

pub(super) fn is_graph_input(path: &str, configured_patterns: &[InputPattern]) -> bool {
    configured_patterns
        .iter()
        .any(|pattern| pattern.matches(path))
}

#[derive(Debug)]
pub(super) enum InputPattern {
    Exact(String),
    Glob(glob::Pattern),
}

impl InputPattern {
    fn package_relative(package: &str, pattern: &str) -> Option<Self> {
        // `./**/*` names the package directory itself, but `glob::Pattern`
        // compares it literally, so it would never match `src/lib.rs`.
        let pattern = pattern.trim_start_matches("./");
        if Path::new(pattern).is_absolute() || pattern.starts_with('/') {
            return None;
        }
        let mut parts = Vec::new();
        // A pattern may climb out of its package, as globs do, but not out
        // of the workspace.
        for part in package.split('/').chain(pattern.split('/')) {
            match part {
                "" | "." => {}
                ".." => {
                    parts.pop()?;
                }
                part => parts.push(part),
            }
        }
        Self::workspace_relative(&parts.join("/"))
    }

    fn workspace_relative(pattern: &str) -> Option<Self> {
        if !safe_pattern(pattern) {
            return None;
        }
        if has_glob_metacharacters(pattern) {
            glob::Pattern::new(pattern).ok().map(Self::Glob)
        } else {
            WorkspacePath::try_from(pattern)
                .ok()
                .map(|path| Self::Exact(path.to_string()))
        }
    }

    fn as_str(&self) -> &str {
        match self {
            Self::Exact(path) => path,
            Self::Glob(pattern) => pattern.as_str(),
        }
    }

    pub(super) fn matches(&self, path: &str) -> bool {
        match self {
            Self::Exact(expected) => expected == path,
            Self::Glob(pattern) => pattern.matches_with(path, GLOB_MATCH_OPTIONS),
        }
    }
}

/// The options target kinds expand `srcs` with, so a single `*` matches one
/// path segment here as it does when the graph is built.
const GLOB_MATCH_OPTIONS: glob::MatchOptions = glob::MatchOptions {
    case_sensitive: true,
    require_literal_separator: true,
    require_literal_leading_dot: false,
};

fn has_glob_metacharacters(pattern: &str) -> bool {
    pattern
        .chars()
        .any(|character| matches!(character, '?' | '*' | '['))
}

fn safe_pattern(pattern: &str) -> bool {
    let path = Path::new(pattern);
    !path.is_absolute()
        && path.components().all(|component| {
            matches!(
                component,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        })
}

fn metadata_provider(
    analyzer: Option<&struo_frontend::analysis::AnalysisEngine>,
    workspace: &Path,
    target: &GraphTarget,
) -> Option<serde_json::Value> {
    if !target
        .capabilities
        .iter()
        .any(|capability| capability.name == "metadata")
    {
        return None;
    }
    let analyzer = analyzer?;
    let analysis = analyzer
        .analyze_target_capability(target, workspace, &[], "metadata")
        .ok()?;
    Some(analysis.provider)
}

fn provider_string_list(provider: &serde_json::Value, key: &str) -> Vec<String> {
    provider
        .get(key)
        .and_then(serde_json::Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}
