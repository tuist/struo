use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::sync::OnceLock;

use serde::Serialize;
use sha2::{Digest, Sha256};

use super::{evaluate_prelude_exports, PreludeExports};

mod format;
use format::{Record, RecordRef};
mod identity;

const MAX_RECORD_BYTES: u64 = 8 * 1024 * 1024;

pub(super) fn load(source: &str) -> PreludeExports {
    static EXECUTABLE: OnceLock<Option<String>> = OnceLock::new();
    let identity = EXECUTABLE.get_or_init(identity::current);
    let path = identity.as_ref().map(|identity| {
        struo_core::Xdg::from_env()
            .cache_home
            .join("struo/schemas")
            .join(key(source, identity))
    });
    load_at(source, path.as_deref())
}

fn key(source: &str, executable: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(b"struo.schemas.v3\0");
    hash.update((executable.len() as u64).to_le_bytes());
    hash.update(executable.as_bytes());
    hash.update(source.as_bytes());
    hex::encode(hash.finalize())
}

fn load_at(source: &str, path: Option<&Path>) -> PreludeExports {
    if let Some(record) = path.and_then(read) {
        tracing::debug!("reused cached prelude schemas");
        return record.into();
    }
    tracing::debug!("evaluating prelude schemas");
    let exports = evaluate_prelude_exports(source);
    if let Some(path) = path {
        if write(path, &exports).is_none() {
            tracing::debug!("prelude schemas were not persisted");
        }
    }
    exports
}

fn read(path: &Path) -> Option<Record> {
    let file = File::open(path).ok()?;
    if file.metadata().ok()?.len() > MAX_RECORD_BYTES {
        return None;
    }
    serde_json::from_reader(BufReader::new(file.take(MAX_RECORD_BYTES))).ok()
}

fn write(path: &Path, exports: &PreludeExports) -> Option<()> {
    let record = RecordRef::new(exports)?;
    let parent = path.parent()?;
    std::fs::create_dir_all(parent).ok()?;
    let mut temp = tempfile::NamedTempFile::new_in(parent).ok()?;
    {
        let mut writer = BufWriter::new(temp.as_file_mut());
        record
            .serialize(&mut serde_json::Serializer::new(&mut writer))
            .ok()?;
        writer.flush().ok()?;
    }
    if temp.as_file().metadata().ok()?.len() > MAX_RECORD_BYTES {
        return None;
    }
    temp.persist(path).ok()?;
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_exports_preserve_resolvers_examples_and_native_projects() {
        let temp = tempfile::tempdir().unwrap();
        let combined = format!(
            "{}\nfixture = target_kind(docs = \"Workspace kind\")\n",
            crate::modules::built_in_module_source()
        );
        let source = combined.as_str();
        let path = temp.path().join(key(source, "binary-a"));
        let evaluated = load_at(source, Some(&path));
        let cached: PreludeExports = read(&path).expect("persisted schemas").into();
        assert_eq!(
            cached.built_in_target_kinds,
            evaluated.built_in_target_kinds
        );
        let built_in = cached
            .built_in_target_kinds
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap();
        assert!(!built_in.iter().any(|kind| kind.kind == "fixture"));
        assert!(cached
            .target_kinds
            .as_ref()
            .unwrap()
            .iter()
            .any(|kind| kind.kind == "fixture"));
        assert_eq!(cached.target_kinds, evaluated.target_kinds);
        assert_eq!(cached.native_projects, evaluated.native_projects);
        assert!(cached
            .target_kinds
            .as_ref()
            .unwrap()
            .iter()
            .any(|kind| kind.has_resolver));
        assert!(cached
            .target_kinds
            .as_ref()
            .unwrap()
            .iter()
            .any(|kind| !kind.examples.is_empty()));
        assert_ne!(key(source, "binary-a"), key(source, "binary-b"));
        assert_ne!(
            key(source, "binary-a"),
            key(&format!("{source}\n"), "binary-a")
        );
        std::fs::write(&path, b"{truncated").unwrap();
        assert_eq!(
            load_at(source, Some(&path)).target_kinds,
            evaluated.target_kinds
        );
        assert_eq!(
            load_at(source, Some(temp.path())).native_projects,
            evaluated.native_projects
        );
    }

    #[test]
    fn evaluation_errors_are_not_persisted() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("invalid");
        let source = "bad = target_kind(";
        let evaluated = evaluate_prelude_exports(source);
        let cached = load_at(source, Some(&path));
        assert_eq!(cached.target_kinds, evaluated.target_kinds);
        assert_eq!(cached.native_projects, evaluated.native_projects);
        assert!(!path.exists());
    }
}
