mod http;
mod tools;

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use super::cas::Cas;
use super::execution::Executor;
use super::*;
use crate::{ResourceLimits, ResourcePool};

fn executor(root: &Path) -> Executor {
    let preparation_root = root.join("prepared");
    std::fs::create_dir_all(preparation_root.join("data")).unwrap();
    std::fs::write(preparation_root.join("data/tool"), "immutable").unwrap();
    std::fs::create_dir(preparation_root.join("home")).unwrap();
    std::fs::write(preparation_root.join("home/credential"), "secret").unwrap();
    let tool_environment = BTreeMap::from([
        ("PATH".into(), "/usr/bin:/bin".into()),
        (
            "HOME".into(),
            preparation_root.join("home").to_string_lossy().into_owned(),
        ),
        (
            "TMPDIR".into(),
            preparation_root.join("tmp").to_string_lossy().into_owned(),
        ),
    ]);
    let options = ServeOptions {
        environment: "test".into(),
        preparation_root,
        tool_environment,
        tool_launcher: Vec::new(),
        memory_bytes: 32 * 1024 * 1024,
        capacity_bytes: 1024 * 1024,
    };
    Executor::new(
        options,
        Arc::new(ResourcePool::new(ResourceLimits::new(1, 32 * 1024 * 1024))),
    )
}

fn action(script: &str) -> Request {
    Request {
        version: 1,
        environment: "test".into(),
        argv: vec!["/bin/sh".into(), "-c".into(), script.into()],
        env: BTreeMap::new(),
        cwd: None,
        inputs: Vec::new(),
        post_setup_inputs: Vec::new(),
        outputs: Vec::new(),
        clean_paths: Vec::new(),
        create_dirs: Vec::new(),
        stdout: None,
        stderr: None,
        timeout_ms: Some(5000),
        success_exit_codes: vec![0],
        memory_bytes: 1024 * 1024,
        shared_cache: None,
    }
}

#[tokio::test]
async fn incompatible_or_oversized_requests_never_start() {
    let root = tempfile::tempdir().unwrap();
    let mut executor = executor(root.path());
    let mut cas = Cas::open(root.path().join("cas"), 1024).unwrap();
    let mut request = action("touch should-not-exist");
    request.memory_bytes = 33 * 1024 * 1024;
    assert!(executor
        .execute(root.path(), &mut cas, request, CancellationToken::new())
        .await
        .unwrap_err()
        .to_string()
        .contains("memory budget"));
    let mut request = action("touch should-not-exist");
    request.environment = "stale".into();
    assert!(executor
        .execute(root.path(), &mut cas, request, CancellationToken::new())
        .await
        .is_err());
    assert!(!root.path().join("should-not-exist").exists());
}

#[tokio::test]
async fn native_isolation_probe_then_status_outputs_and_fresh_workspaces() {
    let root = tempfile::tempdir().unwrap();
    let mut executor = executor(root.path());
    if let Err(error) = executor.probe(root.path()).await {
        eprintln!("ISOLATION NOT EXERCISED: {error}");
        assert!(error.to_string().contains("bubblewrap"));
        return;
    }
    let mut cas = Cas::open(root.path().join("cas"), 1024 * 1024).unwrap();
    let secret = root.path().join("credential");
    std::fs::write(&secret, "must remain hidden").unwrap();
    let tool = executor.options.preparation_root.join("data/tool");
    let script = format!(
        "test ! -e '{}' && test ! -e /home/sprite/.ssh && test ! -e /run/secrets && test ! -e /tmp/host-secret && test \"$PWD\" = '{}' && test \"$HOME\" = '{}/test-home' && mkdir -p \"$HOME\" && echo home > \"$HOME/file\" && ! echo changed > '{}' && echo captured && echo result > nested/output && mkdir -p folder && echo nested > folder/result",
        secret.display(),
        EXECUTION_ROOT,
        EXECUTION_ROOT,
        tool.display()
    );
    let mut request = action(&script);
    request
        .env
        .insert("HOME".into(), format!("{EXECUTION_ROOT}/test-home"));
    request.outputs.push("nested/output".parse().unwrap());
    request.outputs.push("folder".parse().unwrap());
    request.outputs.push("folder/result".parse().unwrap());
    let response = executor
        .execute(root.path(), &mut cas, request, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(response.result.exit_code, 0);
    assert!(response.result.stdout.is_some());
    let archive = response.output_archive.unwrap();
    assert!(response.output_archive_bytes > 0);
    let mut archive = tar::Archive::new(std::fs::File::open(cas.path(&archive)).unwrap());
    assert_eq!(archive.entries().unwrap().count(), 3);
    assert_eq!(std::fs::read_to_string(&tool).unwrap(), "immutable");
    if Path::new("/usr/bin/cc").is_file() {
        let response = executor.execute(
            root.path(), &mut cas,
            action("printf 'int main(void) { return 0; }' > main.c && /usr/bin/cc main.c -o executable && ./executable"),
            CancellationToken::new(),
        ).await.unwrap();
        assert_eq!(response.result.exit_code, 0);
    }
    let response = executor
        .execute(
            root.path(),
            &mut cas,
            action("test ! -e nested && test ! -e test-home && test ! -e \"$HOME/credential\" && echo private > \"$HOME/file\" && echo temporary > \"$TMPDIR/file\""),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(response.result.exit_code, 0);
    let mut request = action("echo failure >&2; exit 7");
    request.outputs.push("missing".parse().unwrap());
    let response = executor
        .execute(root.path(), &mut cas, request, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(response.result.exit_code, 7);
    assert!(response.result.stderr.is_some());
    assert!(response.output_archive.is_none());
    assert!(!std::fs::read_dir(root.path()).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("action-")
    }));
}

#[tokio::test]
async fn native_timeout_and_cancellation_remove_workspaces() {
    let root = tempfile::tempdir().unwrap();
    let mut executor = executor(root.path());
    if let Err(error) = executor.probe(root.path()).await {
        eprintln!("ISOLATION NOT EXERCISED: {error}");
        return;
    }
    let mut cas = Cas::open(root.path().join("cas"), 1024 * 1024).unwrap();
    let mut request = action("sleep 60 & wait");
    request.timeout_ms = Some(100);
    let error = executor
        .execute(root.path(), &mut cas, request, CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    let cancel = CancellationToken::new();
    let work = executor.execute(
        root.path(),
        &mut cas,
        action("sleep 60 & wait"),
        cancel.clone(),
    );
    let signal = async {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        cancel.cancel();
    };
    let (result, ()) = tokio::join!(work, signal);
    assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::Interrupted);
    assert!(!std::fs::read_dir(root.path()).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("action-")
    }));
}
