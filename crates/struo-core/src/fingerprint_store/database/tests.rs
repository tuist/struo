use super::*;
use crate::{InputFingerprintComponent, InputFingerprintManifest};

fn legacy(workspace: &Path) -> (Connection, InputFingerprintManifest, serde_json::Value) {
    std::fs::create_dir_all(path(workspace).parent().unwrap()).unwrap();
    let connection = Connection::open(path(workspace)).unwrap();
    connection
        .execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE nodes (digest BLOB PRIMARY KEY NOT NULL, data BLOB NOT NULL)
             WITHOUT ROWID;",
        )
        .unwrap();
    let manifest = InputFingerprintManifest::new(
        Digest::of_bytes(b"historical input"),
        (0..1024)
            .map(|index: u32| {
                InputFingerprintComponent::new(
                    "source",
                    format!("src/{index:04}/{}", "x".repeat(512)),
                    Digest::of_bytes(&index.to_le_bytes()),
                )
            })
            .collect(),
    );
    let stored = super::super::encode_stored(&manifest, |digest, bytes| {
        connection.execute(
            "INSERT OR IGNORE INTO nodes (digest, data) VALUES (?1, ?2)",
            rusqlite::params![digest.as_bytes().as_slice(), bytes],
        )?;
        Ok(())
    })
    .unwrap();
    (connection, manifest, stored)
}

fn count(connection: &Connection) -> usize {
    connection
        .query_row("SELECT count(*) FROM nodes", [], |row| row.get(0))
        .unwrap()
}

#[test]
fn migration_preserves_history_and_live_legacy_connections() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (old, manifest, stored) = legacy(tmp.path());
    let unreferenced = b"historical node with no current target outcome";
    let digest = Digest::of_bytes(unreferenced);
    old.execute(
        "INSERT INTO nodes (digest, data) VALUES (?1, ?2)",
        rusqlite::params![digest.as_bytes().as_slice(), unreferenced.as_slice()],
    )
    .unwrap();
    old.execute_batch("BEGIN").unwrap();
    let before = count(&old);
    let mut cached_insert = old
        .prepare_cached("INSERT INTO nodes (digest, data) VALUES (?1, ?2)")
        .unwrap();

    let migrated = open(tmp.path(), false).unwrap();
    assert_eq!(count(&migrated), before);
    assert_eq!(count(&old), before);
    assert_eq!(read_node(&migrated, digest).unwrap(), unreferenced);
    let _: i64 = migrated
        .query_row("SELECT rowid FROM nodes LIMIT 1", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        super::super::decode_fingerprint(tmp.path(), stored).unwrap(),
        manifest
    );
    assert!(migrated
        .execute("INSERT INTO nodes VALUES (NULL, x'00')", [])
        .is_err());
    old.execute_batch("COMMIT").unwrap();
    let bytes = b"written through a previously prepared legacy statement";
    let digest = Digest::of_bytes(bytes);
    cached_insert
        .execute(rusqlite::params![
            digest.as_bytes().as_slice(),
            bytes.as_slice()
        ])
        .unwrap();
    assert_eq!(read_node(&migrated, digest).unwrap(), bytes);
}

#[test]
fn migration_rolls_back_on_full_database_and_can_be_retried() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (mut connection, manifest, stored) = legacy(tmp.path());
    let before = count(&connection);
    let maximum: u32 = connection
        .pragma_query_value(None, "max_page_count", |row| row.get(0))
        .unwrap();
    let pages: u32 = connection
        .pragma_query_value(None, "page_count", |row| row.get(0))
        .unwrap();
    connection
        .pragma_update(None, "max_page_count", pages + 2)
        .unwrap();
    let error = migrate(&mut connection).unwrap_err();
    assert_eq!(
        error
            .downcast_ref::<rusqlite::Error>()
            .unwrap()
            .sqlite_error_code(),
        Some(ErrorCode::DiskFull)
    );
    assert!(without_rowid(&connection).unwrap());
    assert_eq!(count(&connection), before);
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name='nodes_rowid'",
                [],
                |row| row.get::<_, usize>(0),
            )
            .unwrap(),
        0
    );
    connection
        .pragma_update(None, "max_page_count", maximum)
        .unwrap();
    migrate(&mut connection).unwrap();
    assert!(!without_rowid(&connection).unwrap());
    assert_eq!(count(&connection), before);
    assert_eq!(
        super::super::decode_fingerprint(tmp.path(), stored).unwrap(),
        manifest
    );
}

#[test]
fn concurrent_migrations_preserve_existing_and_new_fingerprints() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (connection, manifest, stored) = legacy(tmp.path());
    drop(connection);
    let start = std::sync::Barrier::new(4);
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..4)
            .map(|index: u32| {
                let start = &start;
                let workspace = tmp.path();
                let manifest = &manifest;
                let stored = &stored;
                scope.spawn(move || {
                    start.wait();
                    let store = super::super::FingerprintStore::new(workspace);
                    assert_eq!(store.decode(stored.clone()).unwrap(), *manifest);
                    let new = InputFingerprintManifest::new(
                        Digest::of_bytes(&index.to_le_bytes()),
                        vec![InputFingerprintComponent::new(
                            "source",
                            format!("new/{index}"),
                            Digest::of_bytes(&index.to_le_bytes()),
                        )],
                    );
                    (store.encode(&new).unwrap(), new)
                })
            })
            .collect();
        for handle in handles {
            let (new_stored, expected) = handle.join().unwrap();
            assert_eq!(
                super::super::decode_fingerprint(tmp.path(), new_stored).unwrap(),
                expected
            );
        }
    });
    assert_eq!(
        super::super::decode_fingerprint(tmp.path(), stored).unwrap(),
        manifest
    );
}
