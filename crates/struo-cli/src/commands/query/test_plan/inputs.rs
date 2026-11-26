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
            let inputs = TargetInputs {
                srcs: package_patterns(&target.srcs),
                srcs_exclude: package_patterns(&srcs_exclude),
                build_script_data: package_patterns(&build_script_data),
                provider_inputs,
            };
            (target.label.id.clone(), inputs)
        })
        .collect()
}

/// The paths a target declares as its inputs: its `srcs` without the paths its
/// `srcs_exclude` attribute removes, its `build_script_data`, and the
/// `affected_inputs` its metadata provider reports.
#[derive(Debug)]
pub(super) struct TargetInputs {
    srcs: Vec<InputPattern>,
    srcs_exclude: Vec<InputPattern>,
    build_script_data: Vec<InputPattern>,
    provider_inputs: Vec<InputPattern>,
}

impl TargetInputs {
    pub(super) fn matches(&self, path: &str) -> bool {
        (self.in_srcs(path) && !self.excludes(path))
            || self
                .build_script_data
                .iter()
                .chain(&self.provider_inputs)
                .any(|pattern| pattern.matches(path))
    }

    /// Whether `srcs_exclude` takes `path` out of this target's sources.
    pub(super) fn excludes_from_srcs(&self, path: &str) -> bool {
        self.in_srcs(path) && self.excludes(path)
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
        let path = if package.is_empty() {
            pattern.to_string()
        } else {
            format!("{package}/{pattern}")
        };
        Self::workspace_relative(&path)
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

    pub(super) fn matches(&self, path: &str) -> bool {
        match self {
            Self::Exact(expected) => expected == path,
            Self::Glob(pattern) => pattern.matches(path),
        }
    }
}

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
