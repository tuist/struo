//! Inputs a command reports reading, checked against its cached result.
//!
//! An action that declares `input_discovery` names a broad set of candidate
//! inputs, such as a package's whole directory, and has its command write a
//! depfile naming the files it actually read. When the action runs locally
//! without a sandbox, the candidates leave its cache key. After a run, Struo
//! stores the digest of every workspace file the depfile names, or of every
//! candidate when the command wrote none, as an extra output of the action. A
//! cached result is reused only while each of those files still has its
//! recorded digest, so an edit to a candidate the command never read keeps the
//! hit, and an edit to one it did read, inside the candidates or not, misses.
//!
//! The record lives in the cache entry it describes and is checked on every
//! reuse, so what an action depends on never comes from another run's history.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};
use struo_cas::{ActionResult, CacheProvider, Digest};
use struo_core::{SandboxMode, WorkspacePath};
use struo_frontend::analysis::{DeclaredAction, DeclaredInputDiscovery};

use super::lowering::effective_sandbox;
use super::recovery::Producers;
use crate::commands::graph::analysis::source_digest_cache::SourceDigestCache;

const ABSENT: &[u8] = b"struo.discovered_input.absent.v1\0";

/// The discovery the action declares, when this run can honor it: a command
/// that runs locally with the whole workspace visible, so it can read files
/// outside the inputs left in its key.
pub(super) fn active<'a>(
    declared: &'a DeclaredAction,
    sandbox_override: SandboxMode,
    producers: &Producers,
) -> Result<Option<&'a DeclaredInputDiscovery>> {
    let Some(discovery) = &declared.input_discovery else {
        return Ok(None);
    };
    let local = match producers.execution() {
        Some(execution) => {
            struo_core::Executor::from_binding(execution)? == struo_core::Executor::Local
        }
        None => true,
    };
    let unsandboxed =
        effective_sandbox(declared.sandbox.as_deref(), sandbox_override)? == SandboxMode::Off;
    Ok(
        (declared.cacheable && declared.operation.is_none() && local && unsandboxed)
            .then_some(discovery),
    )
}

/// The declaration the cache key is built from: the candidates removed, and
/// the discovery itself named so that a keyed result and a discovered one
/// never share an entry.
pub(super) fn keyed(
    declared: &DeclaredAction,
    discovery: &DeclaredInputDiscovery,
) -> DeclaredAction {
    let candidates = discovery
        .candidates
        .iter()
        .collect::<std::collections::HashSet<_>>();
    let mut keyed = declared.clone();
    keyed.inputs = declared
        .inputs
        .iter()
        .filter(|input| !candidates.contains(input))
        .cloned()
        .collect();
    let mut identity = declared.toolchain_identity.clone().unwrap_or_default();
    identity.push_str("\0struo.input_discovery.v1\0");
    identity.push_str(&discovery.depfile);
    identity.push('\0');
    identity.push_str(&discovery.record);
    keyed.toolchain_identity = Some(identity);
    keyed
}

/// Remove a depfile a previous run left, so a command that writes none this
/// time is not credited with the last one's reads.
pub(super) fn clear_depfile(workspace: &Path, discovery: &DeclaredInputDiscovery) -> Result<()> {
    let path = workspace_path(&discovery.depfile)?.resolve(workspace);
    match std::fs::remove_file(&path) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            Err(error).with_context(|| format!("removing depfile `{}`", path.display()))
        }
        _ => Ok(()),
    }
}

/// Whether every file a cached result recorded still has its recorded digest.
pub(super) async fn still_valid(
    workspace: &Path,
    cache: &CacheProvider,
    source_digest_cache: Option<&SourceDigestCache>,
    discovery: &DeclaredInputDiscovery,
    result: &ActionResult,
) -> Result<bool> {
    let Some(record) = result.outputs.get(&discovery.record) else {
        return Ok(false);
    };
    let Some(recorded) = read_record(cache, record).await? else {
        return Ok(false);
    };
    for (path, digest) in recorded {
        if current_digest(workspace, source_digest_cache, &path) != digest {
            tracing::debug!(path = %path, "discovered input changed");
            return Ok(false);
        }
    }
    Ok(true)
}

/// The keys of earlier results stored for `action`, newest first. The
/// candidates are not in an action's key, so runs that read different file
/// contents share one key; each result is also stored under a key of its own,
/// and this index lets reverting an edit reuse the result from before it.
pub(super) async fn variants(cache: &CacheProvider, action: &Digest) -> Vec<Digest> {
    let index = match cache.lookup_action_result(&variant_index_key(action)).await {
        Ok(Some(hit)) => hit.result,
        _ => return Vec::new(),
    };
    let Some(blob) = index.stdout else {
        return Vec::new();
    };
    match cache.get_blob(&blob).await {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

/// Store `result` under its own variant key and put that key first in the
/// index of `action`, keeping the most recent few.
pub(super) async fn remember(
    cache: &CacheProvider,
    action: &Digest,
    discovery: &DeclaredInputDiscovery,
    result: &ActionResult,
) -> Result<()> {
    let Some(record) = result.outputs.get(&discovery.record) else {
        return Ok(());
    };
    let variant = Digest::of_bytes(
        format!("struo.input_discovery.variant.v1\0{action}\0{record}").as_bytes(),
    );
    cache
        .put_action_result(&variant, result)
        .await
        .context("storing a discovered-input variant")?;
    let mut keys = vec![variant];
    keys.extend(
        variants(cache, action)
            .await
            .into_iter()
            .filter(|key| *key != variant),
    );
    keys.truncate(VARIANTS);
    let bytes = serde_json::to_vec(&keys).context("encoding discovered-input variants")?;
    let index = ActionResult {
        exit_code: 0,
        stdout: Some(
            cache
                .put_blob(&bytes)
                .await
                .context("storing variant index")?,
        ),
        stderr: None,
        outputs: BTreeMap::new(),
    };
    cache
        .put_action_result(&variant_index_key(action), &index)
        .await
        .context("storing variant index")
}

const VARIANTS: usize = 8;

fn variant_index_key(action: &Digest) -> Digest {
    Digest::of_bytes(format!("struo.input_discovery.variants.v1\0{action}").as_bytes())
}

/// Record what the command read into `result`, as the output `record`.
pub(super) async fn record(
    workspace: &Path,
    cache: &CacheProvider,
    source_digest_cache: Option<&SourceDigestCache>,
    declared: &DeclaredAction,
    discovery: &DeclaredInputDiscovery,
    result: &mut ActionResult,
) -> Result<()> {
    let depfile = workspace_path(&discovery.depfile)?.resolve(workspace);
    let paths = match std::fs::read_to_string(&depfile) {
        Ok(text) => {
            let cwd = match &declared.cwd {
                Some(cwd) => workspace_path(cwd)?.resolve(workspace),
                None => workspace.to_path_buf(),
            };
            let canonical = std::fs::canonicalize(workspace).unwrap_or(workspace.to_path_buf());
            depfile_dependencies(&text)
                .into_iter()
                .filter_map(|path| workspace_relative(&cwd, workspace, &canonical, &path))
                .collect::<Vec<_>>()
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => discovery.candidates.clone(),
        Err(error) => {
            return Err(error).with_context(|| format!("reading depfile `{}`", depfile.display()))
        }
    };
    let recorded = paths
        .into_iter()
        .filter(|path| !path.starts_with(".struo/") && path != ".struo")
        .map(|path| {
            let digest = current_digest(workspace, source_digest_cache, &path);
            (path, digest)
        })
        .collect::<BTreeMap<_, _>>();
    let bytes = serde_json::to_vec(&recorded).context("encoding discovered inputs")?;
    let path = workspace_path(&discovery.record)?.resolve(workspace);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating `{}`", parent.display()))?;
    }
    std::fs::write(&path, &bytes).with_context(|| format!("writing `{}`", path.display()))?;
    let digest = cache
        .put_blob(&bytes)
        .await
        .context("storing discovered inputs")?;
    result.outputs.insert(discovery.record.clone(), digest);
    Ok(())
}

/// The workspace files a cached record names, or `None` when its blob is gone.
pub(in crate::commands::graph::analysis) async fn recorded_paths(
    cache: &CacheProvider,
    record: &Digest,
) -> Result<Option<Vec<String>>> {
    Ok(read_record(cache, record)
        .await?
        .map(|recorded| recorded.into_keys().collect()))
}

async fn read_record(
    cache: &CacheProvider,
    record: &Digest,
) -> Result<Option<BTreeMap<String, Digest>>> {
    let bytes = match cache.get_blob(record).await {
        Ok(bytes) => bytes,
        Err(error) if error.is_unavailable() || error.is_not_found() => return Ok(None),
        Err(error) => return Err(error).context("reading discovered inputs"),
    };
    Ok(serde_json::from_slice(&bytes).ok())
}

fn current_digest(
    workspace: &Path,
    source_digest_cache: Option<&SourceDigestCache>,
    path: &str,
) -> Digest {
    let digest = match source_digest_cache {
        Some(cache) => cache.digest(workspace, path),
        None => struo_core::digest_source_path(workspace, path),
    };
    digest.unwrap_or_else(|_| Digest::of_bytes(ABSENT))
}

fn workspace_path(path: &str) -> Result<WorkspacePath> {
    WorkspacePath::try_from(path).with_context(|| format!("invalid workspace path `{path}`"))
}

/// The prerequisites of every rule in a Make-syntax depfile, as compilers
/// write them: continuation lines joined, `\ ` and `$$` unescaped, and
/// comments such as rustc's `# env-dep:` lines skipped.
pub(super) fn depfile_dependencies(text: &str) -> Vec<String> {
    let joined = text.replace("\\\r\n", " ").replace("\\\n", " ");
    let mut dependencies = Vec::new();
    for line in joined.lines() {
        if line.trim_start().starts_with('#') {
            continue;
        }
        let Some(separator) = rule_separator(line) else {
            continue;
        };
        let mut current = String::new();
        let mut characters = line[separator + 1..].chars().peekable();
        while let Some(character) = characters.next() {
            match character {
                '\\' if characters.peek() == Some(&' ') => {
                    current.push(' ');
                    characters.next();
                }
                '$' if characters.peek() == Some(&'$') => {
                    current.push('$');
                    characters.next();
                }
                ' ' | '\t' => {
                    if !current.is_empty() {
                        dependencies.push(std::mem::take(&mut current));
                    }
                }
                _ => current.push(character),
            }
        }
        if !current.is_empty() {
            dependencies.push(current);
        }
    }
    dependencies.sort();
    dependencies.dedup();
    dependencies
}

/// The `:` that ends a rule's targets: the first one followed by whitespace
/// or the end of the line, which skips a Windows drive letter.
fn rule_separator(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    (0..bytes.len()).find(|&index| {
        bytes[index] == b':'
            && (index > 0 && bytes[index - 1] != b'\\')
            && bytes.get(index + 1).is_none_or(u8::is_ascii_whitespace)
    })
}

fn workspace_relative(
    cwd: &Path,
    workspace: &Path,
    canonical: &Path,
    path: &str,
) -> Option<String> {
    let path = normalize(&cwd.join(path));
    let relative = path
        .strip_prefix(workspace)
        .or_else(|_| path.strip_prefix(canonical))
        .ok()?;
    let text = relative.to_str()?.replace('\\', "/");
    (!text.is_empty()).then_some(text)
}

fn normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                normalized.pop();
            }
            Component::CurDir => {}
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depfiles_name_every_prerequisite_once() {
        let rustc = "target/libapp.rlib: src/lib.rs src/config/../../dev/example\\ file.toml \\\n  src/a.rs\n\nsrc/lib.rs:\n\n# env-dep:CARGO_PKG_NAME=app\n";
        assert_eq!(
            depfile_dependencies(rustc),
            [
                "src/a.rs",
                "src/config/../../dev/example file.toml",
                "src/lib.rs"
            ]
        );
        assert_eq!(
            depfile_dependencies("C:\\out\\app.d: C:\\src\\lib.rs\n"),
            ["C:\\src\\lib.rs"]
        );
        assert!(depfile_dependencies("build-script:\n").is_empty());
    }

    #[test]
    fn dependencies_resolve_to_workspace_paths() {
        let workspace = Path::new("/w");
        let resolve =
            |cwd: &str, path: &str| workspace_relative(Path::new(cwd), workspace, workspace, path);
        assert_eq!(
            resolve("/w", "src/config/../../dev/x.toml").as_deref(),
            Some("dev/x.toml")
        );
        assert_eq!(
            resolve("/w/crates/a", "../../proto").as_deref(),
            Some("proto")
        );
        assert_eq!(resolve("/w", "/usr/include/stdio.h"), None);
        assert_eq!(resolve("/w", "/w"), None);
    }
}
