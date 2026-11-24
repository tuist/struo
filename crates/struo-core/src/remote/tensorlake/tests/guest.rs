use super::*;

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn final_executable_receives_literal_arguments_and_selected_ifs_and_pwd() {
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
    let stdout = directory.path().join("stdout");
    let stderr = directory.path().join("stderr");
    let (url, server) = test_server::spawn_with_handler(3, move |request| {
        if request.method == "POST" {
            let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
            assert_eq!(body["command"], "python3");
            let mut args = body["args"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_str().unwrap().to_owned())
                .collect::<Vec<_>>();
            assert_eq!(args[0], "-I");
            args[4] = stdout.to_str().unwrap().into();
            args[5] = stderr.to_str().unwrap().into();
            let status = std::process::Command::new("python3")
                .args(args)
                .status()
                .unwrap();
            exited(status.code().unwrap())
        } else {
            Response {
                body: std::fs::read(if request.path.contains("stdout") {
                    &stdout
                } else {
                    &stderr
                })
                .unwrap(),
                ..Response::empty(200)
            }
        }
    })
    .await;
    let client = client(&url);
    let workspace = tempfile::tempdir().unwrap();
    let env = BTreeMap::from([
        ("IFS".into(), "selected ' ; $IFS".into()),
        ("PWD".into(), "selected ' ; $PWD".into()),
        ("not-a.shell identifier".into(), "value\n' ; $HOME".into()),
        ("PYTHONPATH".into(), "/nonexistent/selected".into()),
    ]);
    let resources = ResourceRequest::default();
    let literal_args = vec!["literal=value ' ; $HOME", "", "--", "line\nnext"];
    let mut argv = vec![
        executable.to_str().unwrap().into(),
        "-I".into(),
        "-c".into(),
        "import json, os, sys; os.write(1, json.dumps([sys.argv[1:], dict(os.environ)]).encode() + b'\\x00\\xff'); os.write(2, b'\\xfe\\x00'); sys.exit(7)".into(),
    ];
    argv.extend(literal_args.iter().map(|arg| (*arg).to_owned()));
    let command = PreparedCommand {
        lease: None,
        argv: &argv,
        env: &env,
        cwd: None,
        inputs: &[],
        outputs: &[],
        resources: &resources,
        redirect: crate::local::Redirect::default(),
        timeout_ms: None,
        success_exit_codes: &[0],
    };
    let cache = CacheProvider::open_local(workspace.path().join("cache"));
    let result = super::super::capture::execute(
        &client,
        &sandbox(),
        &client.guest_url(&url).unwrap(),
        &command,
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
    assert_eq!(
        cache.get_blob(&result.stderr.unwrap()).await.unwrap(),
        b"\xfe\0"
    );
    test_server::assert_ready(server).await.unwrap();
}

#[tokio::test]
async fn raw_upload_is_binary_safe_across_transfer_chunks() {
    let data = (0..100_003_u32)
        .map(|index| index.to_le_bytes()[0])
        .collect::<Vec<_>>();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("input");
    std::fs::write(&path, &data).unwrap();
    let (url, server) = test_server::spawn(vec![Response::empty(204)]).await;
    let client = client(&url);
    client
        .upload(
            &sandbox(),
            &client.guest_url(&url).unwrap(),
            &path,
            data.len() as u64,
            "/tmp/binary",
        )
        .await
        .unwrap();
    let requests = test_server::assert_ready(server).await.unwrap();
    assert_eq!(requests[0].body, data);
    assert_eq!(requests[0].headers["content-length"], "100003");
    assert_eq!(requests[0].headers["x-tensorlake-sandbox-id"], "sb-one");
}
