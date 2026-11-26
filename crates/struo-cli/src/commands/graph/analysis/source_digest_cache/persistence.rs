use std::collections::BTreeMap;
use std::io::{BufReader, Read, Seek};
use std::path::Path;

use serde::de::{IgnoredAny, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

use super::{ChangePosition, Entry, SCHEMA};

#[derive(Deserialize)]
pub(super) struct CacheFile<Entries = BTreeMap<String, Entry>> {
    schema: String,
    pub(super) entries: Entries,
    #[serde(default)]
    pub(super) output_entries: Entries,
    #[serde(default)]
    pub(super) position: Option<ChangePosition>,
}

#[derive(Serialize)]
pub(super) struct CacheFileRef<'a> {
    pub(super) schema: &'static str,
    pub(super) entries: &'a BTreeMap<String, Entry>,
    pub(super) output_entries: &'a BTreeMap<String, Entry>,
    pub(super) position: &'a Option<ChangePosition>,
}

pub(super) fn load<Entries: Default + for<'de> Deserialize<'de>>(
    path: &Path,
) -> Option<CacheFile<Entries>> {
    let reader = BufReader::new(std::fs::File::open(path).ok()?);
    let cache: CacheFile<Entries> = serde_json::from_reader(reader).ok()?;
    (cache.schema == SCHEMA).then_some(cache)
}

/// Read the watcher position without retaining the source and output maps.
pub(crate) fn stored_position(workspace: &Path, memory_budget: u64) -> Option<ChangePosition> {
    const MAX_BUFFER_BYTES: u64 = 4 * 1024 * 1024;
    let buffer_limit = match memory_budget {
        0 => MAX_BUFFER_BYTES,
        budget => budget.min(MAX_BUFFER_BYTES),
    };
    let mut file = std::fs::File::open(workspace.join(".struo/source-digests.json")).ok()?;
    let length = file.metadata().ok()?.len();
    if length < buffer_limit {
        let mut bytes = Vec::with_capacity(usize::try_from(length + 1).ok()?);
        file.by_ref()
            .take(length + 1)
            .read_to_end(&mut bytes)
            .ok()?;
        if bytes.len() as u64 <= length {
            let cache: CacheFile<DiscardedEntries> = serde_json::from_slice(&bytes).ok()?;
            return (cache.schema == SCHEMA).then_some(cache.position).flatten();
        }
        // A growing file must be validated in full, not just its original prefix.
        file.rewind().ok()?;
    }
    let cache: CacheFile<DiscardedEntries> = serde_json::from_reader(BufReader::new(file)).ok()?;
    (cache.schema == SCHEMA).then_some(cache.position).flatten()
}

#[derive(Default)]
struct DiscardedEntries;

impl<'de> Deserialize<'de> for DiscardedEntries {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct EntriesVisitor;
        impl<'de> Visitor<'de> for EntriesVisitor {
            type Value = DiscardedEntries;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a map of source digest entries")
            }

            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                // Validate each entry as before, but drop it instead of building a map.
                while map.next_entry::<IgnoredAny, Entry>()?.is_some() {}
                Ok(DiscardedEntries)
            }
        }
        deserializer.deserialize_map(EntriesVisitor)
    }
}
