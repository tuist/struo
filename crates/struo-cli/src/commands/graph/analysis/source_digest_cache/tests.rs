use std::collections::BTreeSet;
use std::sync::atomic::Ordering;

use super::{KnownChanges, SourceDigestCache};

#[test]
fn cache_position_round_trips_without_republishing_a_stale_position() {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::write(workspace.path().join("source.rs"), "one").unwrap();
    let position = super::ChangePosition {
        instance_id: "watcher".into(),
        source_generation: 7,
        output_generation: 19,
    };
    let cache = SourceDigestCache::open(workspace.path());
    let digest = cache.digest(workspace.path(), "source.rs").unwrap();
    cache.record_output(workspace.path(), "source.rs", digest);
    cache.set_position(Some(position.clone()));
    drop(cache);

    assert_eq!(
        super::stored_position(workspace.path(), 0),
        Some(position.clone())
    );
    let (reopened, stored) = SourceDigestCache::open_with_position(workspace.path());
    assert_eq!(stored, Some(position));
    assert_eq!(reopened.recorded_output_digest("source.rs"), Some(digest));
    assert_eq!(
        reopened.digest(workspace.path(), "source.rs").unwrap(),
        digest
    );
    std::fs::write(workspace.path().join("other.rs"), "two").unwrap();
    reopened.digest(workspace.path(), "other.rs").unwrap();
    drop(reopened);
    assert_eq!(super::stored_position(workspace.path(), 0), None);
}

#[test]
fn position_reads_validate_entries_and_the_complete_document() {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::write(workspace.path().join("source.rs"), "one").unwrap();
    let cache = SourceDigestCache::open(workspace.path());
    let digest = cache.digest(workspace.path(), "source.rs").unwrap();
    cache.record_output(workspace.path(), "source.rs", digest);
    let position = super::ChangePosition {
        instance_id: "watcher".into(),
        source_generation: 7,
        output_generation: 19,
    };
    cache.set_position(Some(position.clone()));
    drop(cache);
    let path = workspace.path().join(".struo/source-digests.json");
    let assert_position = |expected: Option<super::ChangePosition>| {
        let length = std::fs::metadata(&path).unwrap().len();
        for budget in [0, 1, length, length + 1] {
            assert_eq!(
                super::stored_position(workspace.path(), budget),
                expected,
                "budget {budget}"
            );
        }
    };
    let valid: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_position(Some(position.clone()));
    for field in ["entries", "output_entries"] {
        for (pointer, value) in [
            ("/digest", serde_json::json!("invalid")),
            ("/digest", serde_json::json!("A".repeat(64))),
            ("/fingerprint/len", serde_json::json!(-1)),
            ("/fingerprint/modified_nanos", serde_json::json!(u64::MAX)),
            ("/fingerprint", serde_json::json!({})),
        ] {
            let mut corrupt = valid.clone();
            *corrupt[field]["source.rs"].pointer_mut(pointer).unwrap() = value;
            std::fs::write(&path, serde_json::to_vec(&corrupt).unwrap()).unwrap();
            assert_position(None);
            assert_eq!(
                SourceDigestCache::open_with_position(workspace.path()).1,
                None
            );
        }
    }
    let json = serde_json::to_string(&valid).unwrap();
    for invalid in [
        json[..json.len() - 1].to_string(),
        format!("{json} trailing"),
        format!("{json}{json}"),
        json.replacen("\"entries\":", "\"entries\":{},\"entries\":", 1),
        json.replace(super::SCHEMA, "unknown"),
    ] {
        std::fs::write(&path, invalid).unwrap();
        assert_position(None);
    }
    std::fs::write(&path, format!("{json}{}", " ".repeat(4 * 1024 * 1024))).unwrap();
    assert_position(Some(position));
    std::fs::write(&path, format!("{json}{}x", " ".repeat(4 * 1024 * 1024))).unwrap();
    assert_position(None);
    let mut legacy = valid;
    legacy.as_object_mut().unwrap().remove("output_entries");
    legacy.as_object_mut().unwrap().remove("position");
    std::fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
    let cache = SourceDigestCache::open(workspace.path());
    assert_eq!(cache.digest(workspace.path(), "source.rs").unwrap(), digest);
    assert_position(None);
}

#[tokio::test]
async fn restoring_a_tree_preserves_a_matching_newer_child() {
    use struo_cas::{CacheProvider, Cas};
    use struo_core::{Action, CopyPathMode, WorkspacePath};

    let workspace = tempfile::tempdir().unwrap();
    let cache = CacheProvider::Local(Cas::open(workspace.path().join("cache")));
    let digests = SourceDigestCache::open(workspace.path());
    std::fs::create_dir(workspace.path().join("seed")).unwrap();
    std::fs::write(workspace.path().join("seed/file"), "old").unwrap();
    let tree = Action::CopyPath {
        sources: vec![WorkspacePath::try_from("seed").unwrap()],
        destination: WorkspacePath::try_from("out/tree").unwrap(),
        mode: CopyPathMode::Tree,
        input_digest: None,
    };
    let mut result = struo_core::run_uncached(&tree, workspace.path(), &cache, false)
        .await
        .unwrap();
    let original_tree = result.clone();
    digests.record_outputs(&result, workspace.path());
    let child = Action::WriteFile {
        path: WorkspacePath::try_from("out/tree/file").unwrap(),
        bytes: bytes::Bytes::from_static(b"new"),
        input_digest: None,
    };
    let updated = struo_core::run_uncached(&child, workspace.path(), &cache, false)
        .await
        .unwrap();
    digests.record_outputs(&updated, workspace.path());
    result.outputs.extend(updated.outputs.clone());
    digests
        .materialize_outputs(&result, workspace.path(), &cache, None)
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("out/tree/file")).unwrap(),
        "new"
    );
    digests
        .materialize_outputs(&original_tree, workspace.path(), &cache, None)
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("out/tree/file")).unwrap(),
        "old"
    );
    digests.with_known_changes(KnownChanges::Since {
        sources: BTreeSet::new(),
        outputs: BTreeSet::new(),
    });
    digests
        .materialize_outputs(&updated, workspace.path(), &cache, None)
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("out/tree/file")).unwrap(),
        "new"
    );
}

#[test]
fn unchanged_files_reuse_the_persisted_digest() {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::write(workspace.path().join("source.rs"), "one").unwrap();

    let first = SourceDigestCache::open(workspace.path());
    let digest = first.digest(workspace.path(), "source.rs").unwrap();
    drop(first);

    let second = SourceDigestCache::open(workspace.path());
    assert_eq!(
        second.inner.entries.read().unwrap().len(),
        1,
        "the reopened cache should load its persisted entry"
    );
    assert!(!second.inner.dirty.load(Ordering::Relaxed));
    assert_eq!(
        second.digest(workspace.path(), "source.rs").unwrap(),
        digest
    );
    assert!(!second.inner.dirty.load(Ordering::Relaxed));
}

#[test]
fn same_length_content_changes_invalidate_the_digest() {
    let workspace = tempfile::tempdir().unwrap();
    let path = workspace.path().join("source.rs");
    std::fs::write(&path, "one").unwrap();
    let cache = SourceDigestCache::open(workspace.path());
    let first = cache.digest(workspace.path(), "source.rs").unwrap();

    std::fs::write(&path, "two").unwrap();

    assert_ne!(cache.digest(workspace.path(), "source.rs").unwrap(), first);
}

// The change tracker does not follow symbolic links, so it never reports an
// edit made behind a linked directory, such as a package linked from outside
// the workspace.
#[cfg(unix)]
#[test]
fn watcher_cannot_vouch_for_sources_behind_linked_directories() {
    let workspace = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    std::fs::write(elsewhere.path().join("index.js"), "one").unwrap();
    std::fs::create_dir(workspace.path().join("node_modules")).unwrap();
    std::os::unix::fs::symlink(
        elsewhere.path(),
        workspace.path().join("node_modules/linked"),
    )
    .unwrap();
    std::fs::write(workspace.path().join("local.js"), "one").unwrap();
    let cache = SourceDigestCache::open(workspace.path());
    let linked = cache
        .digest(workspace.path(), "node_modules/linked/index.js")
        .unwrap();
    let local = cache.digest(workspace.path(), "local.js").unwrap();
    cache.with_known_changes(KnownChanges::Since {
        sources: BTreeSet::new(),
        outputs: BTreeSet::new(),
    });

    std::fs::write(elsewhere.path().join("index.js"), "two").unwrap();

    assert_ne!(
        cache
            .digest(workspace.path(), "node_modules/linked/index.js")
            .unwrap(),
        linked
    );
    assert_eq!(cache.digest(workspace.path(), "local.js").unwrap(), local);
}

#[test]
fn malformed_cache_files_are_ignored() {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::create_dir(workspace.path().join(".struo")).unwrap();
    std::fs::write(
        workspace.path().join(".struo/source-digests.json"),
        "not json",
    )
    .unwrap();
    std::fs::write(workspace.path().join("source.rs"), "content").unwrap();

    let cache = SourceDigestCache::open(workspace.path());

    assert!(cache.digest(workspace.path(), "source.rs").is_ok());
}

#[test]
fn changed_outputs_do_not_reuse_the_recorded_digest() {
    let workspace = tempfile::tempdir().unwrap();
    let relative = ".struo/out/library.rlib";
    std::fs::create_dir_all(workspace.path().join(".struo/out")).unwrap();
    std::fs::write(workspace.path().join(relative), "first").unwrap();
    std::thread::sleep(struo_host_tree::SETTLE_WINDOW * 2);
    let expected = struo_cas::Digest::of_bytes(b"encoded output");
    let cache = SourceDigestCache::open(workspace.path());
    cache.record_output(workspace.path(), relative, expected);

    assert!(cache.output_matches(workspace.path(), relative, expected));

    std::fs::write(workspace.path().join(relative), "other").unwrap();

    assert!(!cache.output_matches(workspace.path(), relative, expected));
}

#[test]
fn removed_outputs_do_not_reuse_watcher_trusted_digests() {
    let workspace = tempfile::tempdir().unwrap();
    let relative = ".struo/out/library.rlib";
    std::fs::create_dir_all(workspace.path().join(".struo/out")).unwrap();
    std::fs::write(workspace.path().join(relative), "first").unwrap();
    let expected = struo_cas::Digest::of_bytes(b"encoded output");
    let cache = SourceDigestCache::open(workspace.path());
    cache.record_output(workspace.path(), relative, expected);
    cache.with_known_changes(KnownChanges::Since {
        sources: BTreeSet::new(),
        outputs: BTreeSet::new(),
    });

    std::fs::remove_file(workspace.path().join(relative)).unwrap();

    assert!(!cache.output_matches(workspace.path(), relative, expected));
}

#[test]
fn receipt_observations_include_only_reported_changes() {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::write(workspace.path().join("one.rs"), "one").unwrap();
    std::fs::write(workspace.path().join("two.rs"), "two").unwrap();
    let cache = SourceDigestCache::open(workspace.path());
    let one = cache.digest(workspace.path(), "one.rs").unwrap();
    cache.digest(workspace.path(), "two.rs").unwrap();

    assert_eq!(
        cache.observed_digests_for(Some(&["one.rs".to_string()])),
        std::collections::BTreeMap::from([("one.rs".to_string(), one)])
    );
    assert!(cache.observed_digests_for(None).is_empty());
}

#[test]
fn changes_match_ignores_disappeared_unobserved_temporary_files() {
    let workspace = tempfile::tempdir().unwrap();
    let cache = SourceDigestCache::open(workspace.path());

    assert!(cache.changes_match(workspace.path(), Some(&["temporary.rmeta".to_string()])));

    std::fs::write(workspace.path().join("temporary.rmeta"), "metadata").unwrap();

    assert!(!cache.changes_match(workspace.path(), Some(&["temporary.rmeta".to_string()])));
}

#[test]
fn changes_match_requires_observed_inputs_to_retain_their_digest() {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::write(workspace.path().join("source.rs"), "one").unwrap();
    let cache = SourceDigestCache::open(workspace.path());
    cache.digest(workspace.path(), "source.rs").unwrap();

    assert!(cache.changes_match(workspace.path(), Some(&["source.rs".to_string()])));

    std::fs::write(workspace.path().join("source.rs"), "two").unwrap();

    assert!(!cache.changes_match(workspace.path(), Some(&["source.rs".to_string()])));
}
