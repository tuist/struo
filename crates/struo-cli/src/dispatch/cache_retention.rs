//! Keeps the shared local cache within its size cap without user action.
//!
//! Commands that run actions grow the local CAS. When one finishes and no
//! check ran within [`CHECK_INTERVAL`] on this host, it starts a detached
//! `struo __cache-gc` process and exits without waiting for it. That process
//! takes the lock file in the cache root, stamps it, measures the store and,
//! when it is over the cap, collects it down with the same reference-aware
//! pass `struo cache gc` runs. Anything used within
//! [`struo_cas::AUTOMATIC_GC_GRACE`] is kept, so builds running alongside it
//! keep their work. The lock file's modification time records the last
//! check, and holding its lock keeps two collections from running at once.

use std::fs::{File, OpenOptions};
use std::io::ErrorKind;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, SystemTime};

use anyhow::{anyhow, Context, Result};
use struo_cas::CacheProvider;
use struo_core::Xdg;

use crate::cli::{self, Format, Output};

const CHECK_INTERVAL: Duration = Duration::from_hours(1);
const MAX_SIZE_ENV: &str = "STRUO_CACHE_MAX_SIZE";
const LOCK_FILE: &str = "gc.lock";

/// The local cache cap: `STRUO_CACHE_MAX_SIZE` when set, else
/// `[local_cache] max_size` in the root `struo.toml`, else the default. `0`
/// disables automatic collection. The manifest checks its own value when it
/// is parsed; a malformed environment value is an error naming the variable.
pub(super) fn cap_bytes(workspace: &Path) -> Result<u64> {
    let configured = struo_frontend::load_local_cache_max_size(workspace)
        .context("loading local cache settings")?;
    cap_from(std::env::var(MAX_SIZE_ENV).ok().as_deref(), configured)
}

fn cap_from(env: Option<&str>, configured: Option<u64>) -> Result<u64> {
    match env {
        Some(raw) => raw
            .parse::<cli::CacheSize>()
            .map(cli::CacheSize::bytes)
            .map_err(|error| anyhow!("invalid {MAX_SIZE_ENV} `{raw}`: {error}")),
        None => Ok(configured.unwrap_or(cli::DEFAULT_CACHE_SIZE_CAP_BYTES)),
    }
}

/// Start a detached collection when one is due. Costs one `stat` when it is
/// not, and never waits for the collection. A cap that cannot be read skips
/// the check with one warning, and counts as a check so the warning repeats
/// at most once an interval rather than on every command.
pub(super) fn spawn_if_due(xdg: &Xdg, workspace: &Path, output: Output) {
    let root = xdg.struo_cas();
    let now = SystemTime::now();
    if !root.is_dir() || !check_due(&root, now) {
        return;
    }
    match cap_bytes(workspace) {
        Ok(0) => return,
        Ok(_) => {}
        Err(error) => {
            drop(claim_check(&root, now));
            tracing::warn!(
                error = format!("{error:#}"),
                "automatic local cache collection skipped"
            );
            if output.format == Format::Human && !output.quiet {
                eprintln!("struo: warning: skipped automatic cache cleanup: {error:#}");
            }
            return;
        }
    }
    let spawned = crate::commands::change_tracker::helper_launcher(xdg).and_then(|exe| {
        std::process::Command::new(exe)
            .arg("-C")
            .arg(workspace)
            .arg("__cache-gc")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
    });
    match spawned {
        Ok(child) => tracing::debug!(pid = child.id(), "started background cache collection"),
        Err(error) => tracing::debug!(%error, "could not start background cache collection"),
    }
}

/// Collect the local cache when it is over its cap and no other process
/// checked within the interval. Failures are logged and ignored: retention
/// is housekeeping.
pub(super) async fn collect(xdg: &Xdg, workspace: &Path) {
    let root = xdg.struo_cas();
    let cap = match cap_bytes(workspace) {
        Ok(cap) => cap,
        Err(error) => {
            tracing::warn!(
                error = format!("{error:#}"),
                "automatic local cache collection skipped"
            );
            return;
        }
    };
    if cap == 0 || !root.is_dir() {
        return;
    }
    let Some(_lock) = claim_check(&root, SystemTime::now()) else {
        return;
    };
    match CacheProvider::open_local(root)
        .maybe_evict_over_cap(cap, struo_cas::AUTOMATIC_GC_GRACE)
        .await
    {
        Ok(Some(report)) => tracing::info!(
            removed = report.removed,
            bytes_before = report.bytes_before,
            bytes_after = report.bytes_after,
            cap_bytes = cap,
            "local cache auto-evicted over cap"
        ),
        Ok(None) => tracing::debug!(cap_bytes = cap, "local cache within cap"),
        Err(error) => tracing::warn!(%error, "automatic local cache collection failed"),
    }
}

/// Whether a check is due. A cache without a stamp gets one and is checked
/// an interval later, so a fresh cache, such as each test's, never starts a
/// collection, and an existing cache waits one interval after an upgrade.
fn check_due(root: &Path, now: SystemTime) -> bool {
    let path = root.join(LOCK_FILE);
    match std::fs::metadata(&path).and_then(|metadata| metadata.modified()) {
        Ok(last) => is_due(last, now),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            if let Ok(file) = OpenOptions::new().write(true).create_new(true).open(&path) {
                let _ = file.set_modified(now);
            }
            false
        }
        Err(_) => false,
    }
}

/// Take the retention lock and stamp it when a check is due. Returns the
/// locked file, which releases the lock on drop, or `None` when another
/// process holds it or the last check is recent.
fn claim_check(root: &Path, now: SystemTime) -> Option<File> {
    let path = root.join(LOCK_FILE);
    let (file, created) = match OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(file) => (file, true),
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            (OpenOptions::new().write(true).open(&path).ok()?, false)
        }
        Err(_) => return None,
    };
    file.try_lock().ok()?;
    let due = created
        || file
            .metadata()
            .and_then(|metadata| metadata.modified())
            .map_or(true, |last| is_due(last, now));
    if !due {
        return None;
    }
    file.set_modified(now).ok()?;
    Some(file)
}

fn is_due(last: SystemTime, now: SystemTime) -> bool {
    // A stamp in the future means the clock moved back; check rather than
    // wait for it to catch up.
    now.duration_since(last)
        .map_or(true, |elapsed| elapsed >= CHECK_INTERVAL)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn the_environment_overrides_struo_toml_which_overrides_the_default() {
        assert_eq!(cap_from(Some("5GB"), Some(50)).unwrap(), 5_000_000_000);
        assert_eq!(cap_from(Some("0"), Some(50)).unwrap(), 0);
        assert_eq!(cap_from(None, Some(50)).unwrap(), 50);
        assert_eq!(cap_from(None, Some(0)).unwrap(), 0);
        assert_eq!(
            cap_from(None, None).unwrap(),
            cli::DEFAULT_CACHE_SIZE_CAP_BYTES
        );
        let error = cap_from(Some("lots"), Some(50)).unwrap_err().to_string();
        assert!(error.contains(MAX_SIZE_ENV), "{error}");
    }

    #[test]
    fn a_check_runs_once_per_interval_and_never_concurrently() {
        let root = TempDir::new().unwrap();
        let start = SystemTime::now();

        assert!(
            !check_due(root.path(), start),
            "a fresh cache is stamped, not checked"
        );
        assert!(!check_due(
            root.path(),
            start + CHECK_INTERVAL - Duration::from_secs(1)
        ));
        let checked = start + CHECK_INTERVAL;
        assert!(check_due(root.path(), checked));
        let first = claim_check(root.path(), checked);
        assert!(first.is_some());
        assert!(!check_due(root.path(), checked + Duration::from_secs(1)));
        assert!(
            claim_check(root.path(), checked + CHECK_INTERVAL * 2).is_none(),
            "a held lock keeps a second process out"
        );
        drop(first);

        assert!(
            claim_check(
                root.path(),
                checked + CHECK_INTERVAL - Duration::from_secs(1)
            )
            .is_none(),
            "a check within the interval is skipped"
        );
        assert!(check_due(root.path(), checked + CHECK_INTERVAL));
        assert!(claim_check(root.path(), checked + CHECK_INTERVAL).is_some());
    }

    #[test]
    fn a_stamp_from_the_future_does_not_block_checks() {
        let now = SystemTime::now();
        assert!(is_due(now + Duration::from_hours(24), now));
    }
}
