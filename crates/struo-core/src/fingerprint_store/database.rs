use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{ensure, Context, Result};
use rusqlite::{Connection, ErrorCode, OpenFlags, OptionalExtension, TransactionBehavior};
use struo_cas::Digest;

use super::MAX_NODE_BYTES;

pub(super) fn path(workspace: &Path) -> PathBuf {
    workspace.join(".struo/fingerprints/v2.sqlite")
}

pub(super) fn open(workspace: &Path, create: bool) -> Result<Connection> {
    let path = path(workspace);
    let mut flags = OpenFlags::SQLITE_OPEN_READ_WRITE;
    if create {
        std::fs::create_dir_all(
            path.parent()
                .context("fingerprint database has no parent")?,
        )?;
        flags |= OpenFlags::SQLITE_OPEN_CREATE;
    }
    let mut connection = Connection::open_with_flags(&path, flags)
        .with_context(|| format!("opening fingerprint database `{}`", path.display()))?;
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.set_prepared_statement_cache_capacity(4);
    connection.execute_batch(
        "PRAGMA cache_size=-2048;
         PRAGMA mmap_size=0;
         PRAGMA synchronous=NORMAL;
         PRAGMA wal_autocheckpoint=256;
         PRAGMA temp_store=FILE;",
    )?;
    if create {
        let started = Instant::now();
        loop {
            match connection.pragma_update(None, "journal_mode", "WAL") {
                Ok(()) => break,
                // Concurrent journal-mode changes can bypass SQLite's busy handler.
                Err(rusqlite::Error::SqliteFailure(error, _))
                    if error.code == ErrorCode::DatabaseBusy
                        && started.elapsed() < Duration::from_secs(5) =>
                {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => return Err(error).context("enabling fingerprint WAL journal"),
            }
        }
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS nodes (
                digest BLOB PRIMARY KEY NOT NULL,
                data BLOB NOT NULL
             );",
        )?;
    }
    migrate(&mut connection).context("migrating fingerprint storage")?;
    Ok(connection)
}

fn without_rowid(connection: &Connection) -> Result<bool> {
    Ok(connection
        .query_row(
            "SELECT wr FROM pragma_table_list('nodes') WHERE schema='main'",
            [],
            |row| row.get::<_, bool>(0),
        )
        .optional()?
        .unwrap_or(false))
}

fn migrate(connection: &mut Connection) -> Result<()> {
    if without_rowid(connection)? {
        let started = Instant::now();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if without_rowid(&transaction)? {
            transaction.execute_batch(
                "CREATE TABLE nodes_rowid (
                    digest BLOB PRIMARY KEY NOT NULL,
                    data BLOB NOT NULL
                 );
                 INSERT INTO nodes_rowid (digest, data) SELECT digest, data FROM nodes;
                 DROP TABLE nodes;
                 ALTER TABLE nodes_rowid RENAME TO nodes;",
            )?;
            transaction.commit()?;
            tracing::debug!(
                duration_ms = started.elapsed().as_millis(),
                "migrated fingerprint storage to rowid"
            );
        }
    }
    Ok(())
}

pub(super) fn read_node(connection: &Connection, digest: Digest) -> Result<Vec<u8>> {
    let mut query = connection.prepare_cached(
        "SELECT CASE WHEN length(data) <= ?2 THEN data END, length(data)
         FROM nodes WHERE digest = ?1",
    )?;
    let node: Option<(Option<Vec<u8>>, usize)> = query
        .query_row(
            rusqlite::params![digest.as_bytes().as_slice(), MAX_NODE_BYTES],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let (bytes, size) = node.with_context(|| format!("missing fingerprint node {digest}"))?;
    ensure!(
        size <= MAX_NODE_BYTES,
        "fingerprint node {digest} exceeds size limit"
    );
    let bytes = bytes.with_context(|| format!("invalid fingerprint node {digest}"))?;
    ensure!(
        Digest::of_bytes(&bytes) == digest,
        "corrupt fingerprint node {digest}: digest mismatch"
    );
    Ok(bytes)
}

#[cfg(test)]
mod tests;
