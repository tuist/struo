use std::collections::BTreeMap;

use super::*;
use crate::remote::{CommandFilesystemSetup, PreparedCommand};
use crate::{ResourceRequest, WorkspacePath};

mod shared;

#[tokio::test]
async fn adapter_transfers_real_blobs_and_reuses_an_isolated_native_worker() {
    let root = tempfile::tempdir().unwrap();
    let prepared = root.path().join("prepared");
    std::fs::create_dir_all(prepared.join("data")).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let binding = Binding {
        url: format!("http://{}", listener.local_addr().unwrap()),
        token: "0123456789abcdef0123456789abcdef".into(),
        binary: String::new(),
    };
    let task = tokio::spawn(crate::worker::serve(
        listener,
        root.path().join("service"),
        binding.token.clone(),
        crate::worker::ServeOptions {
            environment: "test".into(),
            preparation_root: prepared,
            tool_environment: BTreeMap::from([("PATH".into(), "/usr/bin:/bin".into())]),
            tool_launcher: Vec::new(),
            memory_bytes: 64 * 1024 * 1024,
            capacity_bytes: 1024 * 1024,
        },
    ));
    let service = Service::new(&binding, RemoteIoLimits::default()).unwrap();
    if let Err(error) = service.ready("test").await {
        if task.is_finished() {
            let failure = task.await.unwrap().unwrap_err();
            assert!(failure.to_string().contains("bubblewrap"), "{failure}");
            eprintln!("ISOLATION NOT EXERCISED: {failure}");
            return;
        }
        panic!("worker readiness failed: {error}");
    }
    let workspace = root.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::write(workspace.join("input"), "original").unwrap();
    let cache = struo_cas::CacheProvider::open_local(root.path().join("cache"));
    let argv = vec!["/bin/sh".into(), "-c".into(),
        "test ! -e leaked && cp input output && echo mutated > input && touch leaked && printf 'worker capture'".into()];
    let inputs: Vec<WorkspacePath> = vec!["input".parse().unwrap()];
    let outputs: Vec<WorkspacePath> = vec!["output".parse().unwrap()];
    let env = BTreeMap::new();
    let resources = ResourceRequest::new(1, 1024 * 1024);
    for _ in 0..2 {
        let command = PreparedCommand {
            argv: &argv,
            env: &env,
            inputs: &inputs,
            outputs: &outputs,
            cwd: None,
            resources: &resources,
            redirect: crate::local::Redirect::default(),
            timeout_ms: Some(5000),
            success_exit_codes: &[0],
            lease: None,
        };
        let result = service
            .execute(
                "test",
                &command,
                &workspace,
                &cache,
                false,
                CommandFilesystemSetup::default(),
            )
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(
            std::fs::read_to_string(workspace.join("output")).unwrap(),
            "original"
        );
        assert_eq!(
            std::fs::read_to_string(workspace.join("input")).unwrap(),
            "original"
        );
        let (capture, truncated) = cache
            .read_blob_limited(&result.stdout.unwrap(), 128, false)
            .await
            .unwrap();
        assert!(!truncated);
        assert_eq!(capture, b"worker capture");
    }
    task.abort();
}
