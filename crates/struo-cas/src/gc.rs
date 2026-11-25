use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use tokio::fs;

use crate::{ActionResult, Cas, Digest, Error, Result};

/// How long automatic collection leaves anything used alone. A build that
/// runs alongside a collection finishes well within it.
pub const AUTOMATIC_GC_GRACE: Duration = Duration::from_hours(1);

impl Cas {
    /// Reclaim disk space until the store fits within `max_bytes`.
    ///
    /// Eviction is reference-aware and runs in priority order:
    /// 1. Orphan blobs - content no surviving action result points at,
    ///    oldest first. These are pure waste, so they always go first.
    /// 2. Action results, least recently used first. Evicting an action
    ///    releases the blobs that only it referenced, which then become
    ///    collectable.
    ///
    /// Recency is a file's modification time. Writing an action result,
    /// looking it up, or reusing it through
    /// [`mark_actions_used`](Self::mark_actions_used) refreshes it, and so
    /// does putting content a blob already holds. Blobs referenced by a
    /// surviving action survive with it, so a large, stable input shared by a
    /// recent action stays cached even when it was written long ago.
    ///
    /// Nothing used within `grace` of the pass is deleted. That protects the
    /// work of a build running alongside it: outputs it wrote but has not
    /// recorded in an action result yet, and results it is about to restore.
    /// Each file is checked again just before it is deleted, so a use that
    /// lands after the store was scanned keeps the entry, and an action kept
    /// that way keeps its blobs. A use refreshes an entry at most once a
    /// minute, so with a zero grace an entry used in the minute before the
    /// scan can still be deleted even if it is used again during the pass;
    /// its next lookup then misses and the action runs again.
    ///
    /// With `dry_run`, nothing is deleted and the report describes what a
    /// real pass would reclaim.
    pub async fn gc(&self, max_bytes: u64, grace: Duration, dry_run: bool) -> Result<GcReport> {
        let started = SystemTime::now();
        let cutoff = started.checked_sub(grace).unwrap_or(SystemTime::UNIX_EPOCH);
        let blobs_dir = self.blobs_dir();
        let actions_dir = self.actions_dir();
        let (blobs, actions) = tokio::try_join!(
            collect_blob_files(&blobs_dir),
            collect_action_files(&actions_dir)
        )?;

        let bytes_before: u64 =
            blobs.iter().map(|b| b.size).sum::<u64>() + actions.iter().map(|a| a.size).sum::<u64>();
        if bytes_before <= max_bytes {
            return Ok(GcReport {
                bytes_before,
                bytes_after: bytes_before,
                removed: 0,
                dry_run,
            });
        }
        let expired = |modified: SystemTime| modified < cutoff;

        // How many surviving action results reference each blob digest.
        let mut refcount: HashMap<Digest, usize> = HashMap::new();
        for action in &actions {
            for digest in &action.refs {
                *refcount.entry(*digest).or_insert(0) += 1;
            }
        }
        let blob_index: HashMap<Digest, usize> = blobs
            .iter()
            .enumerate()
            .filter_map(|(i, b)| b.digest.map(|d| (d, i)))
            .collect();

        let mut blob_removed = vec![false; blobs.len()];
        let mut action_removed = vec![false; actions.len()];
        let mut bytes_after = bytes_before;

        // Tier 1: orphan blobs, oldest first. A file that is not a valid
        // content address (a stray) references nothing, so it counts as
        // an orphan too.
        let mut orphans: Vec<usize> = blobs
            .iter()
            .enumerate()
            .filter(|(_, b)| expired(b.modified))
            .filter(|(_, b)| match b.digest {
                Some(d) => refcount.get(&d).copied().unwrap_or(0) == 0,
                None => true,
            })
            .map(|(i, _)| i)
            .collect();
        orphans.sort_by_key(|&i| blobs[i].modified);
        for i in orphans {
            if bytes_after <= max_bytes {
                break;
            }
            blob_removed[i] = true;
            bytes_after = bytes_after.saturating_sub(blobs[i].size);
        }

        // Tier 2: action results, least recently used first, cascading the
        // blobs they release once no surviving action references them.
        if bytes_after > max_bytes {
            let mut order: Vec<usize> = (0..actions.len())
                .filter(|&i| expired(actions[i].modified))
                .collect();
            order.sort_by_key(|&i| actions[i].modified);
            for ai in order {
                if bytes_after <= max_bytes {
                    break;
                }
                action_removed[ai] = true;
                bytes_after = bytes_after.saturating_sub(actions[ai].size);
                for digest in &actions[ai].refs {
                    let Some(count) = refcount.get_mut(digest) else {
                        continue;
                    };
                    *count = count.saturating_sub(1);
                    // A blob whose last surviving action just went away is
                    // unreachable garbage once it is outside the grace period;
                    // collect it unconditionally rather than gating on the
                    // byte budget, so evicting an action never leaves its
                    // private blobs (including zero-byte ones) stranded.
                    if *count == 0 {
                        if let Some(&bi) = blob_index.get(digest) {
                            if !blob_removed[bi] && expired(blobs[bi].modified) {
                                blob_removed[bi] = true;
                                bytes_after = bytes_after.saturating_sub(blobs[bi].size);
                            }
                        }
                    }
                }
            }
        }

        let removed = if dry_run {
            (action_removed.iter().filter(|&&r| r).count()
                + blob_removed.iter().filter(|&&r| r).count()) as u64
        } else {
            let plan = Plan {
                actions: &actions,
                action_removed: &action_removed,
                blobs: &blobs,
                blob_removed: &blob_removed,
            };
            plan.delete(cutoff, &mut refcount, &mut bytes_after).await?
        };

        Ok(GcReport {
            bytes_before,
            bytes_after,
            removed,
            dry_run,
        })
    }
}

/// What one pass decided to evict.
struct Plan<'a> {
    actions: &'a [ActionFile],
    action_removed: &'a [bool],
    blobs: &'a [BlobFile],
    blob_removed: &'a [bool],
}

impl Plan<'_> {
    /// Delete the planned entries, keeping any used since the scan and the
    /// blobs of an action kept that way. Returns how many were removed and
    /// adds what was kept back to `bytes_after`.
    async fn delete(
        &self,
        cutoff: SystemTime,
        refcount: &mut HashMap<Digest, usize>,
        bytes_after: &mut u64,
    ) -> Result<u64> {
        let mut removed = 0u64;
        // Remove owners first so interruption cannot leave an evicted
        // owner's result pointing at blobs already removed by this pass.
        for (action, _) in self
            .actions
            .iter()
            .zip(self.action_removed)
            .filter(|(_, &r)| r)
        {
            if remove_if_expired(&action.path, cutoff).await? {
                removed += 1;
            } else {
                *bytes_after += action.size;
                for digest in &action.refs {
                    *refcount.entry(*digest).or_insert(0) += 1;
                }
            }
        }
        for (blob, _) in self.blobs.iter().zip(self.blob_removed).filter(|(_, &r)| r) {
            let kept_by_action = blob
                .digest
                .is_some_and(|d| refcount.get(&d).copied().unwrap_or(0) > 0);
            if !kept_by_action && remove_if_expired(&blob.path, cutoff).await? {
                removed += 1;
            } else {
                *bytes_after += blob.size;
            }
        }
        Ok(removed)
    }
}

/// Delete `path` unless it was used at or after `cutoff` since the scan.
/// Returns whether it is gone.
async fn remove_if_expired(path: &Path, cutoff: SystemTime) -> Result<bool> {
    match fs::metadata(path).await.and_then(|m| m.modified()) {
        Ok(modified) if modified >= cutoff => return Ok(false),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(true),
        _ => {}
    }
    match fs::remove_file(path).await {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(true),
        Err(source) => Err(Error::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

/// Outcome of a [`Cas::gc`] pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GcReport {
    /// Total on-disk size before collection.
    pub bytes_before: u64,
    /// Total on-disk size after collection (projected, when `dry_run`).
    pub bytes_after: u64,
    /// Number of blobs and action results removed (or that would be).
    pub removed: u64,
    /// Whether this pass only reported and left the store untouched.
    pub dry_run: bool,
}

/// A stored blob and the metadata `gc` ranks it by. `digest` is `None`
/// for a file whose path is not a valid content address.
struct BlobFile {
    path: PathBuf,
    digest: Option<Digest>,
    size: u64,
    modified: SystemTime,
}

/// A stored action result plus the blob digests it references (its
/// outputs, stdout, and stderr).
struct ActionFile {
    path: PathBuf,
    size: u64,
    modified: SystemTime,
    refs: Vec<Digest>,
}

async fn collect_blob_files(root: &Path) -> Result<Vec<BlobFile>> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let mut entries = match fs::read_dir(&dir).await {
            Ok(e) => e,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(source) => return Err(Error::Io { path: dir, source }),
        };
        while let Some(entry) = entries.next_entry().await.map_err(|source| Error::Io {
            path: dir.clone(),
            source,
        })? {
            let Some(ft) = present(entry.path(), entry.file_type().await)? else {
                continue;
            };
            if ft.is_dir() {
                stack.push(entry.path());
            } else if ft.is_file() {
                let name = entry.file_name();
                if name.to_string_lossy().starts_with(".tmp-") {
                    continue;
                }
                let path = entry.path();
                let Some(metadata) = present(path.clone(), entry.metadata().await)? else {
                    continue;
                };
                let digest = digest_from_blob_path(&path);
                out.push(BlobFile {
                    digest,
                    size: metadata.len(),
                    modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                    path,
                });
            }
        }
    }
    Ok(out)
}

async fn collect_action_files(root: &Path) -> Result<Vec<ActionFile>> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let mut entries = match fs::read_dir(&dir).await {
            Ok(e) => e,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(source) => return Err(Error::Io { path: dir, source }),
        };
        while let Some(entry) = entries.next_entry().await.map_err(|source| Error::Io {
            path: dir.clone(),
            source,
        })? {
            let Some(ft) = present(entry.path(), entry.file_type().await)? else {
                continue;
            };
            if ft.is_dir() {
                stack.push(entry.path());
            } else if ft.is_file() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with(".tmp-") || !name.ends_with(".json") {
                    continue;
                }
                let path = entry.path();
                let Some(metadata) = present(path.clone(), entry.metadata().await)? else {
                    continue;
                };
                let refs = match fs::read(&path).await {
                    Ok(bytes) => parse_action_refs(&bytes),
                    Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
                    Err(source) => return Err(Error::Io { path, source }),
                };
                out.push(ActionFile {
                    size: metadata.len(),
                    modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                    refs,
                    path,
                });
            }
        }
    }
    Ok(out)
}

fn present<T>(path: PathBuf, result: io::Result<T>) -> Result<Option<T>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(Error::Io { path, source }),
    }
}

/// The blob digests an action result depends on. A corrupt or foreign
/// file references nothing, so it protects no blob from collection.
fn parse_action_refs(bytes: &[u8]) -> Vec<Digest> {
    let Ok(result) = serde_json::from_slice::<ActionResult>(bytes) else {
        return Vec::new();
    };
    let mut refs: Vec<Digest> = result.outputs.into_values().collect();
    refs.extend(result.stdout);
    refs.extend(result.stderr);
    refs
}

/// Reconstruct a blob digest from its sharded path (`.../aa/<rest>`).
/// Returns `None` for any path that is not a 64-hex content address.
fn digest_from_blob_path(path: &Path) -> Option<Digest> {
    let file = path.file_name()?.to_str()?;
    let shard = path.parent()?.file_name()?.to_str()?;
    if shard.len() != 2 {
        return None;
    }
    let mut hex = String::with_capacity(shard.len() + file.len());
    hex.push_str(shard);
    hex.push_str(file);
    Digest::from_hex(&hex)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn a_disappeared_scan_entry_is_skipped_but_other_io_errors_are_preserved() {
        assert_eq!(present(PathBuf::from("entry"), Ok(7)).unwrap(), Some(7));
        assert!(
            present::<u8>(PathBuf::from("entry"), Err(io::ErrorKind::NotFound.into()))
                .unwrap()
                .is_none()
        );
        assert!(
            matches!(present::<u8>(PathBuf::from("entry"), Err(io::ErrorKind::PermissionDenied.into())),
            Err(Error::Io { source, .. }) if source.kind() == io::ErrorKind::PermissionDenied)
        );
    }

    #[tokio::test]
    async fn gc_on_empty_cas_reports_nothing_removed() {
        let tmp = TempDir::new().unwrap();
        let cas = Cas::open(tmp.path());
        let report = cas.gc(0, Duration::ZERO, false).await.unwrap();
        assert_eq!(report.bytes_before, 0);
        assert_eq!(report.bytes_after, 0);
        assert_eq!(report.removed, 0);
    }

    #[tokio::test]
    async fn gc_under_budget_removes_nothing() {
        let tmp = TempDir::new().unwrap();
        let cas = Cas::open(tmp.path());
        cas.put_blob(b"keep me").await.unwrap();
        let before = cas.stats().await.unwrap().blob_bytes;

        let report = cas.gc(u64::MAX, Duration::ZERO, false).await.unwrap();

        assert_eq!(report.removed, 0);
        assert_eq!(report.bytes_before, before);
        assert_eq!(report.bytes_after, before);
        assert_eq!(cas.stats().await.unwrap().blob_count, 1);
    }

    #[tokio::test]
    async fn gc_evicts_oldest_orphan_blobs_until_within_budget() {
        let tmp = TempDir::new().unwrap();
        let cas = Cas::open(tmp.path());

        // Three distinct, incompressible orphan blobs (no action result
        // references them) written oldest-first. Explicit mtimes make the
        // ordering deterministic regardless of filesystem resolution.
        let payloads: [&[u8]; 3] = [b"first-blob-aaaa", b"second-blob-bbb", b"third-blob-cccc"];
        let mut digests = Vec::new();
        for (index, payload) in payloads.iter().enumerate() {
            let digest = cas.put_blob(payload).await.unwrap();
            let when = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1000 + index as u64);
            filetime_set(&cas.blob_path(&digest), when);
            digests.push(digest);
        }

        let total = cas.stats().await.unwrap().blob_bytes;
        let per_blob = total / 3;

        let report = cas
            .gc(per_blob * 2 + 1, Duration::ZERO, false)
            .await
            .unwrap();

        assert_eq!(report.bytes_before, total);
        assert!(report.bytes_after <= per_blob * 2 + 1);
        assert_eq!(report.removed, 1);
        // The oldest blob is gone; the two newest survive.
        assert!(!cas.has_blob(&digests[0]).await.unwrap());
        assert!(cas.has_blob(&digests[1]).await.unwrap());
        assert!(cas.has_blob(&digests[2]).await.unwrap());
    }

    #[tokio::test]
    async fn gc_keeps_a_blob_a_surviving_action_references_even_if_oldest() {
        let tmp = TempDir::new().unwrap();
        let cas = Cas::open(tmp.path());

        // An OLD blob referenced by a recent action (think: a toolchain
        // blob), and a NEWER orphan blob referenced by nothing.
        let referenced = cas.put_blob(b"toolchain-blob-xxxxxxxx").await.unwrap();
        filetime_set(
            &cas.blob_path(&referenced),
            SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1000),
        );
        let orphan = cas.put_blob(b"stale-output-blob-yyyyyy").await.unwrap();
        filetime_set(
            &cas.blob_path(&orphan),
            SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(5000),
        );

        let action = Digest::of_bytes(b"toolchain-consumer");
        cas.put_action_result(
            &action,
            &ActionResult {
                exit_code: 0,
                stdout: None,
                stderr: None,
                outputs: std::collections::BTreeMap::from([("out/bin".to_string(), referenced)]),
            },
        )
        .await
        .unwrap();

        let total = cas.stats().await.unwrap().blob_bytes + cas.stats().await.unwrap().action_bytes;
        // Budget forces roughly one blob's worth out. A naive oldest-first
        // sweep would evict the referenced (older) blob; reference-aware
        // GC must evict the orphan instead.
        let per_blob = cas.blob_size(&orphan).await.unwrap();
        cas.gc(total - per_blob, Duration::ZERO, false)
            .await
            .unwrap();

        assert!(
            cas.has_blob(&referenced).await.unwrap(),
            "a blob a surviving action references must be kept"
        );
        assert!(
            !cas.has_blob(&orphan).await.unwrap(),
            "the unreferenced orphan should be evicted first"
        );
    }

    #[tokio::test]
    async fn gc_keeps_an_old_action_that_was_just_looked_up() {
        let tmp = TempDir::new().unwrap();
        let cas = Cas::open(tmp.path());
        let result = ActionResult {
            exit_code: 0,
            stdout: None,
            stderr: None,
            outputs: std::collections::BTreeMap::new(),
        };
        let hit = Digest::of_bytes(b"used-every-build");
        let idle = Digest::of_bytes(b"written-once");
        cas.put_action_result(&hit, &result).await.unwrap();
        cas.put_action_result(&idle, &result).await.unwrap();
        filetime_set(
            &cas.action_path(&hit),
            SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1000),
        );
        filetime_set(
            &cas.action_path(&idle),
            SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(5000),
        );

        assert!(cas.get_action_result(&hit).await.unwrap().is_some());
        let stats = cas.stats().await.unwrap();
        cas.gc(stats.action_bytes - 1, Duration::ZERO, false)
            .await
            .unwrap();

        assert!(cas.get_action_result(&hit).await.unwrap().is_some());
        assert_eq!(cas.get_action_result(&idle).await.unwrap(), None);
    }

    #[tokio::test]
    async fn gc_keeps_everything_used_within_the_grace_period() {
        let tmp = TempDir::new().unwrap();
        let cas = Cas::open(tmp.path());
        let long_ago = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1000);
        let idle = cas.put_blob(b"nobody-wants-this-anymore").await.unwrap();
        let rewritten = cas.put_blob(b"another-build-just-produced").await.unwrap();
        let in_flight = cas.put_blob(b"output-not-recorded-yet").await.unwrap();
        let action = Digest::of_bytes(b"recent-action");
        cas.put_action_result(
            &action,
            &ActionResult {
                exit_code: 0,
                stdout: None,
                stderr: None,
                outputs: std::collections::BTreeMap::from([("out".to_string(), idle)]),
            },
        )
        .await
        .unwrap();
        for digest in [idle, rewritten] {
            filetime_set(&cas.blob_path(&digest), long_ago);
        }
        filetime_set(&cas.action_path(&action), long_ago);
        cas.put_blob(b"another-build-just-produced").await.unwrap();
        cas.mark_actions_used([action]).await;

        let report = cas.gc(0, Duration::from_hours(1), false).await.unwrap();

        assert_eq!(report.removed, 0, "{report:?}");
        assert!(cas.has_blob(&idle).await.unwrap(), "kept by a used action");
        assert!(
            cas.has_blob(&rewritten).await.unwrap(),
            "a repeated put is a use"
        );
        assert!(
            cas.has_blob(&in_flight).await.unwrap(),
            "fresh output is never an orphan"
        );

        filetime_set(&cas.action_path(&action), long_ago);
        cas.gc(0, Duration::from_hours(1), false).await.unwrap();
        assert_eq!(cas.get_action_result(&action).await.unwrap(), None);
        assert!(!cas.has_blob(&idle).await.unwrap());
    }

    #[tokio::test]
    async fn a_use_between_the_scan_and_the_delete_keeps_the_action_and_its_blobs() {
        let tmp = TempDir::new().unwrap();
        let cas = Cas::open(tmp.path());
        let long_ago = SystemTime::UNIX_EPOCH + Duration::from_secs(1000);
        let output = cas.put_blob(b"output-of-the-action").await.unwrap();
        let orphan = cas.put_blob(b"nothing-points-here").await.unwrap();
        let action = Digest::of_bytes(b"used-mid-pass");
        cas.put_action_result(
            &action,
            &ActionResult {
                exit_code: 0,
                stdout: None,
                stderr: None,
                outputs: std::collections::BTreeMap::from([("out".to_string(), output)]),
            },
        )
        .await
        .unwrap();
        for path in [
            cas.blob_path(&output),
            cas.blob_path(&orphan),
            cas.action_path(&action),
        ] {
            filetime_set(&path, long_ago);
        }
        let blobs = collect_blob_files(&cas.blobs_dir()).await.unwrap();
        let actions = collect_action_files(&cas.actions_dir()).await.unwrap();
        let mut refcount = HashMap::from([(output, 0)]);
        let plan = Plan {
            actions: &actions,
            action_removed: &vec![true; actions.len()],
            blobs: &blobs,
            blob_removed: &vec![true; blobs.len()],
        };
        let cutoff = SystemTime::now() - Duration::from_hours(1);

        cas.mark_actions_used([action]).await;
        let mut bytes_after = 0;
        let removed = plan
            .delete(cutoff, &mut refcount, &mut bytes_after)
            .await
            .unwrap();

        assert_eq!(removed, 1, "only the orphan goes");
        assert!(cas.get_action_result(&action).await.unwrap().is_some());
        assert!(
            cas.has_blob(&output).await.unwrap(),
            "kept by its kept action"
        );
        assert!(!cas.has_blob(&orphan).await.unwrap());
        assert_eq!(
            bytes_after,
            actions[0].size
                + blobs
                    .iter()
                    .find(|b| b.digest == Some(output))
                    .unwrap()
                    .size
        );
    }

    #[tokio::test]
    async fn gc_cascades_a_blob_when_its_only_action_is_evicted() {
        let tmp = TempDir::new().unwrap();
        let cas = Cas::open(tmp.path());
        let blob = cas.put_blob(b"exclusively-owned-output").await.unwrap();
        // A zero-byte blob (an empty stderr capture) that only this action
        // references: it must be cascaded out even though removing it
        // reclaims no bytes.
        let empty = cas.put_blob(b"").await.unwrap();
        let action = Digest::of_bytes(b"sole-owner");
        cas.put_action_result(
            &action,
            &ActionResult {
                exit_code: 0,
                stdout: None,
                stderr: Some(empty),
                outputs: std::collections::BTreeMap::from([("out/x".to_string(), blob)]),
            },
        )
        .await
        .unwrap();

        // A zero budget evicts the action and, with it, every blob only
        // it referenced - leaving nothing stranded.
        let report = cas.gc(0, Duration::ZERO, false).await.unwrap();

        assert_eq!(report.bytes_after, 0);
        assert!(!cas.has_blob(&blob).await.unwrap());
        assert!(!cas.has_blob(&empty).await.unwrap());
        assert_eq!(cas.get_action_result(&action).await.unwrap(), None);
        assert_eq!(cas.stats().await.unwrap().blob_count, 0);
    }

    #[tokio::test]
    async fn gc_dry_run_reports_without_deleting() {
        let tmp = TempDir::new().unwrap();
        let cas = Cas::open(tmp.path());
        let blob = cas.put_blob(b"still here after dry run").await.unwrap();

        let report = cas.gc(0, Duration::ZERO, true).await.unwrap();

        assert!(report.dry_run);
        assert!(report.removed >= 1);
        assert_eq!(report.bytes_after, 0);
        // Nothing was actually deleted.
        assert!(cas.has_blob(&blob).await.unwrap());
        assert_eq!(cas.stats().await.unwrap().blob_count, 1);
    }

    #[tokio::test]
    async fn gc_ignores_orphaned_tmp_files() {
        let tmp = TempDir::new().unwrap();
        let cas = Cas::open(tmp.path());
        cas.put_blob(b"real").await.unwrap();
        let orphan = cas.blobs_dir().join("zz").join(".tmp-leftover-1234-5");
        fs::create_dir_all(orphan.parent().unwrap()).await.unwrap();
        fs::write(&orphan, b"junk").await.unwrap();

        // A zero budget removes the real blob but must leave the tmp
        // file alone - it is not durable cache state.
        cas.gc(0, Duration::ZERO, false).await.unwrap();

        assert!(orphan.exists());
    }

    /// Set the modified time on `path`. Kept local to the test module so
    /// production code carries no timestamp-mutation API.
    fn filetime_set(path: &Path, when: SystemTime) {
        let file = std::fs::File::options()
            .write(true)
            .open(path)
            .expect("open blob to set mtime");
        file.set_modified(when).expect("set mtime");
    }
}
