use super::*;
use serde_json::json;
use tempfile::TempDir;

const CONFIG: &str = "[tools]\nnode = '22.1.0'\n";
const LOCK: &str = "[[tools.node]]\nversion = '22.1.0'\nbackend = 'core:node'\n";

fn workspace() -> TempDir {
    let root = TempDir::new().unwrap();
    write(root.path(), "mise.toml", CONFIG);
    write(root.path(), "mise.lock", LOCK);
    root
}

fn write(root: &Path, path: &str, content: &str) {
    std::fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
    std::fs::write(root.join(path), content).unwrap();
}

fn binding(properties: serde_json::Value) -> RemoteExecution {
    let mut binding = RemoteExecution::provider("fleet");
    binding.executor = Some("sprites".into());
    binding.properties.insert("worker_pool".into(), properties);
    binding
}

fn spec(root: &Path, properties: serde_json::Value) -> WorkerPoolSpec {
    serde_json::from_value(
        bind(root, binding(properties)).unwrap().properties["worker_pool"].clone(),
    )
    .unwrap()
}

fn error(root: &Path) -> String {
    format!("{:#}", bind(root, binding(json!({}))).unwrap_err())
}

#[test]
fn opt_in_and_adapter_selection_preserve_existing_behavior() {
    let root = TempDir::new().unwrap();
    let ephemeral = RemoteExecution::provider("sprites");
    assert_eq!(bind(root.path(), ephemeral.clone()).unwrap(), ephemeral);
    let mut other = binding(json!({"files": {}}));
    other.executor = Some("docker".into());
    assert_eq!(bind(root.path(), other.clone()).unwrap(), other);
}

#[test]
fn snapshots_before_action_identity_and_ignores_sources() {
    let root = workspace();
    let first = bind(root.path(), binding(json!({}))).unwrap();
    write(root.path(), "main.rs", "source edit");
    assert_eq!(first, bind(root.path(), binding(json!({}))).unwrap());
    let action = |remote| -> struo_core::Action {
        serde_json::from_value(json!({"kind":"run_command", "argv":["tool"], "remote":remote}))
            .unwrap()
    };
    write(
        root.path(),
        "mise.toml",
        &format!("{CONFIG}# environment config change\n"),
    );
    let second = bind(root.path(), binding(json!({}))).unwrap();
    assert_ne!(action(first.clone()).digest(), action(second).digest());
    write(root.path(), "mise.toml", CONFIG);
    write(root.path(), "mise.lock", &LOCK.replace("22.1.0", "22.2.0"));
    write(
        root.path(),
        "mise.toml",
        &CONFIG.replace("22.1.0", "22.2.0"),
    );
    let third = bind(root.path(), binding(json!({}))).unwrap();
    assert_ne!(action(first).digest(), action(third).digest());
}

#[test]
fn environment_fingerprint_includes_files_setup_but_not_capacity_policy() {
    let root = workspace();
    let initial = spec(root.path(), json!({}));
    let capacity = spec(
        root.path(),
        json!({"max_workers": 10, "max_age_seconds": 42}),
    );
    assert_eq!(initial.fingerprint(), capacity.fingerprint());
    assert_ne!(
        initial.fingerprint(),
        spec(root.path(), json!({"setup":[["tool","prepare"]]})).fingerprint()
    );
    write(
        root.path(),
        "mise.lock",
        &format!("{LOCK}# new artifact metadata\n"),
    );
    assert_ne!(
        initial.fingerprint(),
        spec(root.path(), json!({})).fingerprint()
    );
    write(root.path(), "mise.toml", &format!("{CONFIG}# new config\n"));
    assert_ne!(
        initial.fingerprint(),
        spec(root.path(), json!({})).fingerprint()
    );
}

#[test]
fn rejects_caller_files_even_empty_and_invalid_policy() {
    let root = workspace();
    for value in [
        json!({"files": {}}),
        json!({"files": null}),
        json!({"max_workers":0}),
        json!({"setup":[[]]}),
    ] {
        assert!(bind(root.path(), binding(value)).is_err());
    }
}

#[test]
fn requires_both_files_and_valid_toml() {
    let root = TempDir::new().unwrap();
    assert!(error(root.path()).contains("mise.toml"));
    write(root.path(), "mise.toml", CONFIG);
    assert!(error(root.path()).contains("mise.lock"));
    write(root.path(), "mise.lock", "not valid toml");
    assert!(error(root.path()).contains("parsing"));
}

#[test]
fn rejects_unlocked_mismatched_and_multiple_lock_records() {
    let root = workspace();
    for lock in [
        "[tools]", "[[tools.node]]\nversion = '22.2.0'\nbackend = 'core:node'",
        &format!("{LOCK}{LOCK}"),
        "lockfile_version=99\n[tools]",
        "lockfile_version=1\n[[tools.node]]\nversion='22.1.0'\nbackend='core:node'\nspecifiers=['20']",
    ] {
        write(root.path(), "mise.lock", lock);
        assert!(bind(root.path(), binding(json!({}))).is_err(), "{lock}");
    }
}

#[test]
fn modern_specifiers_match_ranges_without_guessing_first_record() {
    let root = workspace();
    write(root.path(), "mise.toml", "[tools]\nnode = '22'\n");
    for format in 1..=4 {
        write(
            root.path(),
            "mise.lock",
            &format!("lockfile_version={format}\n{LOCK}specifiers=['22']\n"),
        );
        assert_eq!(spec(root.path(), json!({})).files.len(), 2);
    }
    write(root.path(), "mise.lock", LOCK);
    assert!(error(root.path()).contains("exact pin"));
}

#[test]
fn rejects_includes_hooks_tasks_env_and_settings() {
    let root = workspace();
    for suffix in [
        "includes=['other.toml']",
        "[hooks]\nenter='echo x'",
        "[tasks.test]\nrun='test'",
        "[env]\nPATH='other'",
        "[settings]\nidiomatic_version_file_enable_tools=['node']",
    ] {
        write(root.path(), "mise.toml", &format!("{suffix}\n{CONFIG}"));
        assert!(
            error(root.path()).contains("does not support")
                || error(root.path()).contains("supports only")
        );
    }
}

#[test]
fn rejects_local_and_alternate_configs_and_unsupported_native_files() {
    for file in [
        "mise.local.toml",
        ".mise.toml",
        ".mise/config.toml",
        ".config/mise/config.toml",
        ".tool-versions",
        "rust-toolchain.toml",
    ] {
        let root = workspace();
        write(root.path(), file, "");
        assert!(error(root.path()).contains("does not support"), "{file}");
    }
}

#[test]
fn native_version_file_is_snapshotted_and_must_match() {
    let root = workspace();
    write(root.path(), ".nvmrc", "22.1.0\n");
    let native = spec(root.path(), json!({}));
    assert_eq!(native.files[".nvmrc"], "22.1.0\n");
    write(root.path(), ".nvmrc", "22.2.0\n");
    assert!(error(root.path()).contains("must match"));
}

#[test]
fn uv_sidecars_are_required_bounded_parsed_and_snapshotted() {
    let root = workspace();
    let locked = format!("lockfile_version=2\n{LOCK}specifiers=['22.1.0']\nuv={{path='.mise/locks/example',digest='sha256:{}'}}\n", "0".repeat(64));
    write(root.path(), "mise.lock", &locked);
    assert!(error(root.path()).contains("sidecars"));
    write(
        root.path(),
        ".mise/locks/example/pyproject.toml",
        "[project]\nname='example'\nversion='1'\n",
    );
    write(root.path(), ".mise/locks/example/uv.lock", "version=1\n");
    assert_eq!(spec(root.path(), json!({})).files.len(), 4);
    write(
        root.path(),
        ".mise/locks/example/uv.lock",
        "[[package]]\nsource={directory='../outside'}\n",
    );
    assert!(error(root.path()).contains("local dependency"));
    write(
        root.path(),
        "mise.lock",
        &locked.replace(".mise/locks/example", "../outside"),
    );
    assert!(error(root.path()).contains("parent traversal"));
}

#[test]
fn rejects_unsupported_aube_sidecars_without_omitting_them() {
    let root = workspace();
    write(
        root.path(),
        "mise.lock",
        &format!("{LOCK}aube={{path='anything',digest='anything'}}\n"),
    );
    assert!(error(root.path()).contains("aube"));
}

#[test]
fn aggregate_snapshot_is_capped() {
    let root = workspace();
    write(
        root.path(),
        "mise.toml",
        &format!("{CONFIG}#{}", "x".repeat(MAX_BYTES)),
    );
    assert!(error(root.path()).contains("1 MiB"));
    write(
        root.path(),
        "mise.toml",
        &format!("{CONFIG}#{}", "x".repeat(MAX_BYTES / 2)),
    );
    write(
        root.path(),
        "mise.lock",
        &format!("{LOCK}#{}", "x".repeat(MAX_BYTES / 2)),
    );
    assert!(error(root.path()).contains("1 MiB"));
}

#[cfg(unix)]
#[test]
fn rejects_file_and_parent_directory_symlinks() {
    let root = workspace();
    std::fs::remove_file(root.path().join("mise.lock")).unwrap();
    std::os::unix::fs::symlink("mise.toml", root.path().join("mise.lock")).unwrap();
    assert!(error(root.path()).contains("symlink"));
    write(root.path(), "mise.lock.real", LOCK);
    std::fs::remove_file(root.path().join("mise.lock")).unwrap();
    std::fs::rename(
        root.path().join("mise.lock.real"),
        root.path().join("mise.lock"),
    )
    .unwrap();
    std::os::unix::fs::symlink("missing", root.path().join(".nvmrc")).unwrap();
    assert!(error(root.path()).contains("symlink"));
    std::fs::remove_file(root.path().join(".nvmrc")).unwrap();
    std::os::unix::fs::symlink(root.path(), root.path().join(".mise")).unwrap();
    assert!(error(root.path()).contains("symlink"));
}

#[test]
fn resolve_execution_snapshots_named_sprites_binding() {
    let root = workspace();
    write(root.path(), "struo.toml", "[infrastructures.fleet]\nkind='sprites'\n[infrastructure.execution]\nprovider='fleet'\n[infrastructure.execution.properties.worker_pool]\nmax_workers=10\nsetup=[['tool','prepare']]\n");
    let xdg = struo_core::Xdg {
        cache_home: root.path().join("cache"),
        state_home: root.path().join("state"),
        data_home: root.path().join("data"),
        config_home: root.path().join("config"),
        runtime_dir: root.path().join("runtime"),
    };
    let binding = super::super::resolve_execution(root.path(), &xdg, None).unwrap();
    assert_eq!(
        binding.properties["worker_pool"]["files"]["mise.lock"],
        LOCK
    );
    assert_eq!(binding.properties["worker_pool"]["max_workers"], 10);
}
