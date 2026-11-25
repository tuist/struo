use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Component, Path};

use anyhow::{bail, ensure, Context, Result};
use struo_core::{RemoteExecution, WorkerPoolSpec};

const MAX_BYTES: usize = 1024 * 1024;

pub(super) fn bind(workspace: &Path, mut binding: RemoteExecution) -> Result<RemoteExecution> {
    if binding.executor.as_deref().unwrap_or(&binding.provider) != "sprites" {
        return Ok(binding);
    }
    let Some(value) = binding.properties.get("worker_pool") else {
        return Ok(binding);
    };
    ensure!(
        value.get("files").is_none(),
        "worker_pool.files is CLI-owned; remove it and configure workspace mise.toml and mise.lock"
    );
    let mut spec: WorkerPoolSpec =
        serde_json::from_value(value.clone()).context("parsing sprites worker_pool")?;
    reject_other_configs(workspace)?;
    let mut snapshot = Snapshot {
        workspace,
        files: BTreeMap::new(),
        bytes: 0,
    };
    let config: toml::Value =
        toml::from_str(&snapshot.read("mise.toml")?).context("parsing worker_pool mise.toml")?;
    let lock: toml::Value = toml::from_str(&snapshot.read("mise.lock")?)
        .context("parsing worker_pool mise.lock; run `mise lock` to regenerate it")?;
    validate_config(&config)?;
    validate_lock(&config, &lock, &mut snapshot)?;
    for (file, tool) in [
        (".nvmrc", "node"),
        (".node-version", "node"),
        (".python-version", "python"),
        (".ruby-version", "ruby"),
        (".go-version", "go"),
        (".java-version", "java"),
        (".bun-version", "bun"),
        (".swift-version", "swift"),
    ] {
        if exists(workspace, file)? {
            let version = snapshot.read(file)?;
            ensure!(config.get("tools").and_then(|v| v.get(tool)).and_then(|v| v.as_str()) == Some(version.trim()),
                "worker_pool native version file {file} must match an explicit tools.{tool} pin in mise.toml; consolidate the version there");
        }
    }
    spec.files = snapshot.files;
    spec.validate().context("validating sprites worker_pool")?;
    binding
        .properties
        .insert("worker_pool".into(), serde_json::to_value(spec)?);
    Ok(binding)
}

fn exists(root: &Path, path: &str) -> Result<bool> {
    match std::fs::symlink_metadata(root.join(path)) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).with_context(|| format!("checking worker_pool input {path}")),
    }
}

fn reject_other_configs(workspace: &Path) -> Result<()> {
    for dir in ["", ".mise", ".config", ".config/mise"] {
        let path = workspace.join(dir);
        if !exists(workspace, dir)? {
            continue;
        }
        ensure!(
            !std::fs::symlink_metadata(&path)?.file_type().is_symlink(),
            "worker_pool rejects symlink configuration directory {dir}"
        );
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let mise_config = (name.starts_with("mise.") || name.starts_with(".mise."))
                && (name.ends_with(".toml") || name.ends_with(".lock"));
            let alternate = matches!(dir, ".mise" | ".config/mise")
                && (name == "config.toml" || name == "config.local.toml");
            if dir.is_empty() && name.starts_with('.') && name.ends_with("-version") {
                ensure!(matches!(name.as_ref(), ".node-version" | ".python-version" | ".ruby-version" | ".go-version" | ".java-version" | ".bun-version" | ".swift-version"),
                    "worker_pool does not support native configuration {name}; move its tools to root mise.toml and regenerate mise.lock");
            }
            if (mise_config || alternate)
                && !(dir.is_empty() && matches!(name.as_ref(), "mise.toml" | "mise.lock"))
            {
                bail!("worker_pool does not support additional mise configuration {dir}/{name}; consolidate it into root mise.toml and regenerate mise.lock");
            }
        }
    }
    for file in [
        ".tool-versions",
        "rust-toolchain",
        "rust-toolchain.toml",
        ".sdkmanrc",
        ".terraform-version",
        ".elixir-version",
        ".erlang-version",
        ".deno-version",
        ".zig-version",
    ] {
        ensure!(!exists(workspace, file)?,
            "worker_pool does not support native configuration {file}; move its tools to root mise.toml and regenerate mise.lock");
    }
    Ok(())
}

fn validate_config(config: &toml::Value) -> Result<()> {
    let table = config
        .as_table()
        .context("worker_pool mise.toml must be a table")?;
    for key in table.keys() {
        ensure!(matches!(key.as_str(), "tools" | "settings"),
            "worker_pool does not support mise.toml `{key}` (including includes, hooks, tasks, env, plugins and local paths); use explicit tools and worker_pool.setup argv instead");
    }
    if let Some(settings) = config.get("settings") {
        let settings = settings
            .as_table()
            .context("worker_pool mise settings must be a table")?;
        ensure!(settings.iter().all(|(key, value)| key == "lockfile" && value.as_bool() == Some(true)),
            "worker_pool supports only settings.lockfile = true; remove other mise settings to keep bootstrap isolated");
    }
    Ok(())
}

fn validate_lock(
    config: &toml::Value,
    lock: &toml::Value,
    snapshot: &mut Snapshot<'_>,
) -> Result<()> {
    let format = lock
        .get("lockfile_version")
        .map(|v| {
            v.as_integer()
                .context("mise.lock lockfile_version must be an integer")
        })
        .transpose()?
        .unwrap_or(0);
    ensure!((0..=4).contains(&format), "worker_pool unsupported mise.lock format {format}; regenerate with a supported mise version");
    for key in lock.as_table().context("mise.lock must be a table")?.keys() {
        ensure!(matches!(key.as_str(), "tools" | "lockfile_version"),
            "worker_pool unsupported mise.lock field `{key}`; tool stubs and conda dependencies must be consolidated or removed");
    }
    let tools = config
        .get("tools")
        .and_then(|v| v.as_table())
        .context("worker_pool requires [tools] in mise.toml")?;
    let locked = lock
        .get("tools")
        .and_then(|v| v.as_table())
        .context("worker_pool requires [tools] in mise.lock; run `mise lock`")?;
    ensure!(
        tools.len() == locked.len(),
        "worker_pool mise.lock tools differ from mise.toml; run `mise lock`"
    );
    for (tool, request) in tools {
        let request = request.as_str().with_context(|| format!("worker_pool tools.{tool} must be one literal version string; option tables and multiple versions are unsupported"))?;
        ensure!(
            !request.contains(['{', '}', '/', '\\'])
                && !request.starts_with("path:")
                && !request.starts_with("ref:"),
            "worker_pool tools.{tool} requires a literal version, not a template, path or ref"
        );
        let records = locked
            .get(tool)
            .and_then(|v| v.as_array())
            .with_context(|| {
                format!("worker_pool mise.lock missing [[tools.{tool}]]; run `mise lock`")
            })?;
        ensure!(records.len() == 1, "worker_pool requires exactly one lock record for {tool}; multiple versions/options are unsupported, consolidate and run `mise lock`");
        let record = &records[0];
        let record_table = record
            .as_table()
            .context("mise.lock tool entry must be a table")?;
        for key in record_table.keys() {
            ensure!(matches!(key.as_str(), "version" | "backend" | "specifiers" | "options" | "uv" | "aube") || key.starts_with("platforms."),
                "worker_pool unsupported mise.lock entry field `{key}`; use a supported lock format without extra inputs");
        }
        let platforms: Vec<_> = record_table
            .keys()
            .filter(|key| key.starts_with("platforms."))
            .collect();
        ensure!(platforms.is_empty() || platforms.iter().any(|key| key.starts_with("platforms.linux-x64")),
            "worker_pool mise.lock has no linux-x64 artifact for {tool}; run `mise lock --platform linux-x64`");
        let version = record
            .get("version")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .context("mise.lock entry requires a version")?;
        ensure!(
            record
                .get("backend")
                .and_then(|v| v.as_str())
                .is_some_and(|s| !s.trim().is_empty()),
            "mise.lock entry requires a backend; run `mise lock`"
        );
        ensure!(record.get("options").is_none_or(|v| v.as_table().is_some_and(toml::map::Map::is_empty)), "worker_pool does not support lock entry options; consolidate tools and run `mise lock`");
        if format == 0 {
            ensure!(request == version, "worker_pool legacy mise.lock requires exact pin {tool} = {version:?}; pin it or upgrade the lockfile with `mise lock --upgrade`");
        } else {
            let specifiers = record
                .get("specifiers")
                .and_then(|v| v.as_array())
                .context("mise.lock entry requires specifiers; run `mise lock`")?;
            ensure!(specifiers.iter().any(|v| v.as_str() == Some(request)), "worker_pool mise.lock does not match request {tool} = {request:?}; run `mise lock`");
        }
        ensure!(record.get("aube").is_none(), "worker_pool does not yet support aube YAML sidecars; use a standalone pinned tool/backend without an aube dependency graph");
        if let Some(sidecar) = record.get("uv") {
            ensure!(sidecar.as_table().is_some_and(|table| table.keys().all(|key| matches!(key.as_str(), "path" | "digest"))),
                "worker_pool supports only uv sidecar path and digest fields; regenerate mise.lock without additional sidecar inputs");
            let path = sidecar
                .get("path")
                .and_then(|v| v.as_str())
                .context("mise.lock sidecar requires path")?;
            ensure!(
                sidecar
                    .get("digest")
                    .and_then(|v| v.as_str())
                    .is_some_and(|s| s.starts_with("sha256:") && s.len() == 71),
                "mise.lock sidecar requires sha256 digest; run `mise lock`"
            );
            for file in ["pyproject.toml", "uv.lock"] {
                let content = snapshot.read(&format!("{path}/{file}"))?;
                let parsed: toml::Value = toml::from_str(&content)
                    .with_context(|| format!("parsing worker_pool sidecar {path}/{file}"))?;
                reject_local_dependencies(&parsed).with_context(|| format!("worker_pool sidecar {path}/{file}; publish/pin local dependencies and regenerate mise.lock"))?;
            }
        }
    }
    Ok(())
}

fn reject_local_dependencies(value: &toml::Value) -> Result<()> {
    match value {
        toml::Value::Table(table) => {
            for (key, value) in table {
                ensure!(
                    !matches!(
                        key.as_str(),
                        "path"
                            | "directory"
                            | "editable"
                            | "virtual"
                            | "workspace"
                            | "include"
                            | "includes"
                            | "find-links"
                            | "backend-path"
                            | "file"
                            | "readme"
                            | "license-files"
                            | "dynamic"
                    ),
                    "unsupported local dependency field `{key}`"
                );
                reject_local_dependencies(value)?;
            }
        }
        toml::Value::Array(values) => {
            for value in values {
                reject_local_dependencies(value)?;
            }
        }
        toml::Value::String(value) => ensure!(
            !["file:", "link:", "workspace:", "{{"]
                .iter()
                .any(|needle| value.contains(needle))
                && !["/", "./", "../", "~/"]
                    .iter()
                    .any(|prefix| value.starts_with(prefix)),
            "unsupported local or dynamic dependency"
        ),
        _ => {}
    }
    Ok(())
}

struct Snapshot<'a> {
    workspace: &'a Path,
    files: BTreeMap<String, String>,
    bytes: usize,
}

impl Snapshot<'_> {
    fn read(&mut self, relative: &str) -> Result<String> {
        let path = Path::new(relative);
        ensure!(!path.is_absolute() && path.components().all(|c| matches!(c, Component::Normal(_))),
            "worker_pool input {relative} must be a relative path inside the workspace without parent traversal");
        if let Some(content) = self.files.get(relative) {
            return Ok(content.clone());
        }
        let mut full = self.workspace.to_path_buf();
        for component in path.components() {
            full.push(component);
            let metadata = std::fs::symlink_metadata(&full).with_context(|| format!("worker_pool requires {relative}; generate and commit mise.toml, mise.lock and referenced sidecars with `mise lock`"))?;
            ensure!(
                !metadata.file_type().is_symlink(),
                "worker_pool rejects symlink input {relative}"
            );
        }
        ensure!(
            std::fs::metadata(&full)?.is_file(),
            "worker_pool input {relative} must be a regular file"
        );
        let remaining = MAX_BYTES - self.bytes;
        let mut content = String::new();
        std::fs::File::open(&full)?
            .take(remaining as u64 + 1)
            .read_to_string(&mut content)
            .with_context(|| format!("reading worker_pool input {relative} as UTF-8"))?;
        ensure!(content.len() <= remaining, "worker_pool environment snapshot exceeds 1 MiB; reduce mise configuration and lock sidecars");
        self.bytes += content.len();
        self.files.insert(relative.into(), content.clone());
        Ok(content)
    }
}

#[cfg(test)]
mod tests;
