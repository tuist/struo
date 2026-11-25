use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use super::{Pool, Slot};

fn ready(slot: &mut Slot, environment: &str, identity: &str) {
    slot.begin(environment).unwrap();
    let record = slot.record_mut().unwrap();
    record.provider_identity = Some(identity.to_owned());
    record.checkpoint = Some("checkpoint".to_owned());
    record.dirty = false;
    slot.save().unwrap();
}

#[test]
fn independently_opened_pools_have_exclusive_leases() {
    let root = tempfile::tempdir().unwrap();
    let first = Pool::new(root.path(), 1).unwrap();
    let second = Pool::new(root.path(), 1).unwrap();
    let slot = first.try_claim("a").unwrap().unwrap();
    assert!(second.try_claim("a").unwrap().is_none());
    drop(slot);
    assert!(second.try_claim("a").unwrap().is_some());
}

#[test]
fn persisted_records_survive_reopening() {
    let root = tempfile::tempdir().unwrap();
    let pool = Pool::new(root.path(), 1).unwrap();
    let mut slot = pool.try_claim("a").unwrap().unwrap();
    ready(&mut slot, "a", "worker");
    let expected = slot.record().unwrap().clone();
    drop(slot);
    drop(pool);
    let pool = Pool::new(root.path(), 1).unwrap();
    let mut slot = pool.try_claim("a").unwrap().unwrap();
    assert_eq!(slot.record(), Some(&expected));
    slot.record_mut().unwrap().dirty = true;
    slot.save().unwrap();
    drop(slot);
    let slot = pool.try_claim("b").unwrap().unwrap();
    assert!(slot.record().unwrap().dirty);
    assert_eq!(
        slot.record().unwrap().provider_identity,
        expected.provider_identity
    );
}

#[test]
fn generation_rotation_reuses_capacity_and_journals_uncertainty() {
    let root = tempfile::tempdir().unwrap();
    let pool = Pool::new(root.path(), 1).unwrap();
    let mut slot = pool.try_claim("old").unwrap().unwrap();
    slot.begin("old").unwrap();
    let old = slot.record().unwrap().clone();
    assert!(old.dirty);
    assert!(old.provider_identity.is_none());
    drop(slot);
    let mut slot = pool.try_claim("new").unwrap().unwrap();
    assert_eq!(slot.record(), Some(&old));
    assert!(slot.begin("new").is_err());
    slot.reset().unwrap();
    slot.begin("new").unwrap();
    assert_ne!(slot.record().unwrap().nonce, old.nonce);
    assert!(pool.try_claim("new").unwrap().is_none());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 3);
}

#[test]
fn compatible_free_slots_win_over_incompatible_and_empty_slots() {
    let root = tempfile::tempdir().unwrap();
    let pool = Pool::new(root.path(), 3).unwrap();
    let mut old = pool.try_claim("old").unwrap().unwrap();
    ready(&mut old, "old", "old-worker");
    let empty = pool.try_claim("new").unwrap().unwrap();
    let mut new = pool.try_claim("new").unwrap().unwrap();
    ready(&mut new, "new", "new-worker");
    drop((old, empty, new));
    let claimed = pool.try_claim("new").unwrap().unwrap();
    assert_eq!(
        claimed.record().unwrap().provider_identity.as_deref(),
        Some("new-worker")
    );
}

#[test]
fn corrupt_and_oversized_records_fail_closed_without_replacement() {
    for bytes in [
        b"broken".to_vec(),
        br#"{"version":1}"#.to_vec(),
        br#"{"version":2,"record":null}"#.to_vec(),
        vec![b' '; 16 * 1024 + 1],
    ] {
        let root = tempfile::tempdir().unwrap();
        let pool = Pool::new(root.path(), 2).unwrap();
        drop(pool.try_claim("a").unwrap().unwrap());
        fs::write(root.path().join("0.json"), &bytes).unwrap();
        assert!(pool.try_claim("a").is_err());
        assert_eq!(fs::read(root.path().join("0.json")).unwrap(), bytes);
        assert!(!root.path().join("1.json").exists());
    }
}

#[test]
fn decreased_cap_preserves_locked_and_idle_state() {
    let root = tempfile::tempdir().unwrap();
    let pool = Pool::new(root.path(), 2).unwrap();
    let first = pool.try_claim("a").unwrap().unwrap();
    let mut second = pool.try_claim("b").unwrap().unwrap();
    ready(&mut second, "b", "retained");
    let bytes = fs::read(root.path().join("1.json")).unwrap();
    let small = Pool::new(root.path(), 1).unwrap();
    assert!(small.try_claim("a").is_err());
    drop(first);
    assert!(small.try_claim("a").is_err());
    drop(second);
    assert!(small.try_claim("b").is_err());
    assert_eq!(fs::read(root.path().join("1.json")).unwrap(), bytes);
    assert!(pool.try_claim("b").unwrap().is_some());
}

#[test]
fn missing_record_is_uncertain_not_empty() {
    let root = tempfile::tempdir().unwrap();
    let pool = Pool::new(root.path(), 2).unwrap();
    drop(pool.try_claim("a").unwrap().unwrap());
    fs::remove_file(root.path().join("0.json")).unwrap();
    assert!(pool.try_claim("b").is_err());
    assert!(!root.path().join("1.json").exists());
}

#[test]
fn failed_save_preserves_previous_durable_record() {
    let root = tempfile::tempdir().unwrap();
    let pool = Pool::new(root.path(), 1).unwrap();
    let mut slot = pool.try_claim("a").unwrap().unwrap();
    slot.begin("a").unwrap();
    let expected = slot.record().unwrap().clone();
    slot.record_mut().unwrap().provider_identity = Some(String::new());
    assert!(slot.save().is_err());
    drop(slot);
    assert_eq!(
        pool.try_claim("a").unwrap().unwrap().record(),
        Some(&expected)
    );
}

#[test]
fn child_process_lease() {
    let Some(root) = std::env::var_os("STRUO_WORKER_POOL_TEST_ROOT") else {
        return;
    };
    let pool = Pool::new(std::path::Path::new(&root), 1).unwrap();
    let mut slot = pool.try_claim("a").unwrap().unwrap();
    slot.begin("a").unwrap();
    println!("worker-pool-ready");
    std::io::stdout().flush().unwrap();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).unwrap();
}

#[test]
fn process_exit_releases_lock_and_retains_dirty_intent() {
    let root = tempfile::tempdir().unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "remote::worker_pool::tests::child_process_lease",
            "--nocapture",
        ])
        .env("STRUO_WORKER_POOL_TEST_ROOT", root.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    loop {
        line.clear();
        assert!(
            output.read_line(&mut line).unwrap() > 0,
            "child exited before claiming"
        );
        if line.trim() == "worker-pool-ready" {
            break;
        }
    }
    let pool = Pool::new(root.path(), 1).unwrap();
    assert!(pool.try_claim("b").unwrap().is_none());
    child.kill().unwrap();
    child.wait().unwrap();
    let slot = pool.try_claim("b").unwrap().unwrap();
    let record = slot.record().unwrap();
    assert_eq!(record.environment, "a");
    assert!(record.dirty);
    assert!(record.provider_identity.is_none());
}

#[cfg(unix)]
#[test]
fn private_permissions_and_symlink_rejection() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let root = tempfile::tempdir().unwrap();
    let pool = Pool::new(root.path(), 1).unwrap();
    drop(pool.try_claim("a").unwrap().unwrap());
    assert_eq!(
        fs::metadata(root.path()).unwrap().permissions().mode() & 0o777,
        0o700
    );
    for name in ["capacity.lock", "0.lock", "0.json"] {
        assert_eq!(
            fs::metadata(root.path().join(name))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    let other = tempfile::NamedTempFile::new().unwrap();
    fs::remove_file(root.path().join("0.json")).unwrap();
    symlink(other.path(), root.path().join("0.json")).unwrap();
    assert!(pool.try_claim("a").is_err());
}
