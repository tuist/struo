use super::*;

#[tokio::test]
async fn final_executable_receives_literal_arguments_and_selected_ifs_and_pwd() {
    let directory = tempfile::tempdir().unwrap();
    let stdout = directory.path().join("stdout");
    let stderr = directory.path().join("stderr");
    let (url, server) = test_server::spawn_with_handler(3, move |request| {
        if request.method == "POST" {
            let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
            let script = body["args"][1]
                .as_str()
                .unwrap()
                .replace("/tmp/struo-stdout", stdout.to_str().unwrap())
                .replace("/tmp/struo-stderr", stderr.to_str().unwrap());
            let status = std::process::Command::new("sh")
                .args(["-c", &script])
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
    ]);
    let resources = ResourceRequest::default();
    let argv = vec!["env".into(), "literal=value ' ; $HOME".into()];
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
    assert_eq!(result.exit_code, 0);
    let stdout = String::from_utf8(cache.get_blob(&result.stdout.unwrap()).await.unwrap()).unwrap();
    for expected in [
        "IFS=selected ' ; $IFS",
        "PWD=selected ' ; $PWD",
        "literal=value ' ; $HOME",
    ] {
        assert!(
            stdout.lines().any(|line| line == expected),
            "missing {expected}"
        );
    }
    assert!(cache
        .get_blob(&result.stderr.unwrap())
        .await
        .unwrap()
        .is_empty());
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
