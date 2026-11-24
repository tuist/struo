use super::*;

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn final_executable_receives_literal_argv_environment_and_binary_capture() {
    let directory = tempfile::tempdir().unwrap();
    let python = std::process::Command::new("python3")
        .args(["-I", "-c", "import sys; print(sys.executable)"])
        .output()
        .unwrap();
    assert!(python.status.success());
    let executable = directory.path().join("tool=probe ' ; $");
    std::os::unix::fs::symlink(
        String::from_utf8(python.stdout).unwrap().trim(),
        &executable,
    )
    .unwrap();
    let guest = directory.path().join("guest");
    std::fs::create_dir_all(guest.join("struo-workspace/nested")).unwrap();
    let (url, server) = test_server::spawn_with_handler(3, move |request| {
        let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
        if request.path.ends_with("execute_async") {
            let command = body["command"]
                .as_str()
                .unwrap()
                .replace("/home/user", guest.to_str().unwrap());
            let output = std::process::Command::new("sh")
                .args(["-c", &command])
                .output()
                .unwrap();
            assert!(output.stdout.is_empty());
            assert!(output.stderr.is_empty(), "{:?}", output.stderr);
            execution("completed", output.status.code())
        } else {
            assert!(request.path.ends_with("download_file"));
            binary(&std::fs::read(guest.join(body["path"].as_str().unwrap())).unwrap())
        }
    })
    .await;
    let env = BTreeMap::from([
        ("IFS".into(), "selected ' ; $IFS".into()),
        ("PWD".into(), "selected ' ; $PWD".into()),
        ("not-a.shell identifier".into(), "value\n' ; $HOME".into()),
        ("PYTHONPATH".into(), "/nonexistent/selected".into()),
    ]);
    let literal_args = ["literal=value ' ; $HOME", "", "--", "line\nnext"];
    let mut argv = vec![
        executable.to_str().unwrap().into(),
        "-I".into(),
        "-c".into(),
        "import json, os, sys; os.write(1, json.dumps([sys.argv[1:], dict(os.environ), os.path.basename(os.getcwd())]).encode() + b'\\x00\\xff'); os.write(2, b'\\xfe\\x00'); sys.exit(7)".into(),
    ];
    argv.extend(literal_args.iter().map(|arg| (*arg).to_owned()));
    let workspace = tempfile::tempdir().unwrap();
    let cache = CacheProvider::open_local(workspace.path().join("cache"));
    let resources = ResourceRequest::default();
    let cwd = WorkspacePath::try_from("nested").unwrap();
    let result = capture::execute(
        &client(&url),
        &super::super::client::Sandbox {
            id: "dbx_test".into(),
            status: "running".into(),
            metadata: BTreeMap::new(),
        },
        &PreparedCommand {
            lease: None,
            argv: &argv,
            env: &env,
            cwd: Some(&cwd),
            inputs: &[],
            outputs: &[],
            resources: &resources,
            redirect: crate::local::Redirect::default(),
            timeout_ms: None,
            success_exit_codes: &[0],
        },
        workspace.path(),
        &cache,
        false,
    )
    .await
    .unwrap();
    assert_eq!(result.exit_code, 7);
    let stdout = cache.get_blob(&result.stdout.unwrap()).await.unwrap();
    assert!(stdout.ends_with(b"\0\xff"));
    let observed: serde_json::Value = serde_json::from_slice(&stdout[..stdout.len() - 2]).unwrap();
    assert_eq!(observed[0], serde_json::json!(literal_args));
    for (key, value) in env {
        assert_eq!(observed[1][&key], value);
    }
    assert_eq!(observed[2], "nested");
    assert_eq!(
        cache.get_blob(&result.stderr.unwrap()).await.unwrap(),
        b"\xfe\0"
    );
    test_server::assert_ready(server).await.unwrap();
}
