use super::*;

fn results() -> Value {
    json!({
        "schema": struo_core::TEST_RESULTS_SCHEMA, "target": "tests",
        "runner": {"type": "demo", "metadata": {}}, "status": "passed",
        "summary": {"total": 1, "passed": 1, "failed": 0, "skipped": 0, "flaky": 0},
        "cases": [{"id": "tests::requested", "name": "requested", "suite": "tests", "status": "passed",
            "attempts": [{"status": "passed"}], "runner_metadata": {}}],
        "artifacts": {"logs": [], "native_results": []},
    })
}

#[test]
fn exact_batch_requires_valid_passing_evidence() {
    let exact = TestBatch::new("tests", vec!["tests::requested".to_string()]).unwrap();
    let whole = TestBatch::new("tests", Vec::new()).unwrap();
    assert!(!batch_succeeded(
        true,
        &exact,
        None,
        Some("missing requested unit")
    ));
    assert!(!batch_succeeded(true, &exact, None, None));
    assert!(batch_succeeded(true, &exact, Some(&results()), None));
    assert!(!batch_succeeded(false, &exact, Some(&results()), None));
    assert!(batch_succeeded(
        true,
        &whole,
        None,
        Some("optional results unavailable")
    ));

    let mut failed = results();
    failed["status"] = json!("failed");
    assert!(!batch_succeeded(true, &exact, Some(&failed), None));
    assert!(!batch_succeeded(true, &whole, Some(&failed), None));
    failed["status"] = json!("passed");
    failed["cases"][0]["status"] = json!("failed");
    assert!(!batch_succeeded(true, &exact, Some(&failed), None));
}

#[test]
fn exact_results_cannot_reuse_a_canonical_or_another_batch_path() {
    let workspace = tempfile::TempDir::new().unwrap();
    let batch = TestBatch::new("tests", vec!["tests::requested".to_string()]).unwrap();
    for path in [
        ".struo/out/tests/test/test_results.json",
        ".struo/out/tests/test/batches/other/test_results.json",
    ] {
        let error = load_batch_results(workspace.path(), &batch, Some(path), "").unwrap_err();
        assert!(error.to_string().contains("must isolate batch outputs"));
    }
}

#[cfg(unix)]
#[test]
fn startup_failure_preserves_its_exit_status_and_stderr_despite_old_evidence() {
    use std::os::unix::fs::PermissionsExt;
    let _guard = super::super::cancellation::TEST_LOCK.lock().unwrap();
    let workspace = tempfile::TempDir::new().unwrap();
    let batch = TestBatch::new("tests", vec!["tests::requested".to_string()]).unwrap();
    let directory = workspace
        .path()
        .join(".struo/out/tests/test/batches")
        .join(&batch.id);
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("test_results.json"),
        serde_json::to_vec(&results()).unwrap(),
    )
    .unwrap();
    let executable = workspace.path().join("failing-runner");
    std::fs::write(
        &executable,
        "#!/bin/sh\necho 'runner startup failed' >&2\nexit 17\n",
    )
    .unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
    let limits = super::super::executor::ChildLimits {
        memory_bytes: 0,
        cpu_slots: 1,
    };
    let portable = super::super::portable::Invocation {
        path: workspace.path().join("plan.json"),
        external_group: None,
        partition_index: None,
    };
    for invocation in [None, Some(&portable)] {
        let run = run_test_target(
            &executable,
            workspace.path(),
            &batch,
            SandboxMode::Off,
            limits,
            invocation,
            None,
        )
        .unwrap();
        assert_eq!(run["success"], false);
        assert_eq!(run["exit_code"], 17);
        assert!(run["stderr"]
            .as_str()
            .unwrap()
            .contains("runner startup failed"));
        if invocation.is_some() {
            assert!(run["results"].is_null());
            assert!(run["results_error"].is_string());
        }
    }
}

#[cfg(unix)]
#[test]
fn whole_target_startup_failure_cannot_reuse_previous_passing_results() {
    use std::os::unix::fs::PermissionsExt;
    let _guard = super::super::cancellation::TEST_LOCK.lock().unwrap();
    let workspace = tempfile::TempDir::new().unwrap();
    let batch = TestBatch::new("tests", Vec::new()).unwrap();
    let directory = workspace
        .path()
        .join(results_directory(&batch, "").unwrap());
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("test_results.json"),
        serde_json::to_vec(&results()).unwrap(),
    )
    .unwrap();
    let executable = workspace.path().join("failing-runner");
    std::fs::write(
        &executable,
        "#!/bin/sh\necho 'dependency sources missing' >&2\nexit 17\n",
    )
    .unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
    let run = run_test_target(
        &executable,
        workspace.path(),
        &batch,
        SandboxMode::Off,
        super::super::executor::ChildLimits {
            memory_bytes: 0,
            cpu_slots: 1,
        },
        None,
        None,
    )
    .unwrap();
    assert_eq!(run["success"], false);
    assert_eq!(run["exit_code"], 17);
    assert!(run["results"].is_null(), "{run}");
    assert!(run["stderr"]
        .as_str()
        .unwrap()
        .contains("dependency sources missing"));
}

#[cfg(unix)]
#[test]
fn each_batch_is_given_its_share_of_the_cpu_budget() {
    use std::os::unix::fs::PermissionsExt;
    let _guard = super::super::cancellation::TEST_LOCK.lock().unwrap();
    let workspace = tempfile::TempDir::new().unwrap();
    let batch = TestBatch::new("tests", Vec::new()).unwrap();
    let arguments = workspace.path().join("arguments");
    let executable = workspace.path().join("recording-runner");
    std::fs::write(
        &executable,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > {}\nexit 0\n",
            arguments.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
    let limits = super::super::executor::ChildLimits {
        memory_bytes: 0,
        cpu_slots: 3,
    };

    run_test_target(
        &executable,
        workspace.path(),
        &batch,
        SandboxMode::Off,
        limits,
        None,
        None,
    )
    .unwrap();

    let arguments = std::fs::read_to_string(arguments).unwrap();
    let arguments = arguments.lines().collect::<Vec<_>>();
    let flag = arguments
        .iter()
        .position(|argument| *argument == "--cpu-limit");
    assert_eq!(flag.map(|index| arguments[index + 1]), Some("3"));
    assert!(!arguments.contains(&"--memory-limit"));
    let subcommand = arguments.iter().position(|argument| *argument == "test");
    assert!(flag < subcommand, "global flags precede the subcommand");
}

#[cfg(unix)]
#[test]
fn scheduled_and_portable_children_receive_the_same_configuration_snapshot() {
    use std::ffi::OsStr;
    use std::os::unix::fs::PermissionsExt;

    let _guard = super::super::cancellation::TEST_LOCK.lock().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let arguments = workspace.path().join("arguments");
    let executable = workspace.path().join("recording-runner");
    std::fs::write(
        &executable,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > {}\nexit 0\n",
            arguments.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
    let configuration = super::super::Configuration {
        overrides: vec!["os=linux".into(), "token=release".into()],
        execution: Some(struo_core::RemoteExecution::provider("sprites")),
        path_suffix: "/cfg-test".into(),
    };
    let snapshot = configuration.snapshot().unwrap();
    let portable = super::super::portable::Invocation {
        path: workspace.path().join("plan.json"),
        external_group: None,
        partition_index: None,
    };
    let batch = TestBatch::new("tests", vec!["tests::requested".into()]).unwrap();
    for invocation in [None, Some(&portable)] {
        run_test_target(
            &executable,
            workspace.path(),
            &batch,
            SandboxMode::Off,
            super::super::executor::ChildLimits {
                memory_bytes: 1024 * 1024 * 1024,
                cpu_slots: 2,
            },
            invocation,
            Some(snapshot.path()),
        )
        .unwrap();
        let arguments = std::fs::read_to_string(&arguments).unwrap();
        let mut argv = vec![OsStr::new("struo")];
        argv.extend(arguments.lines().map(OsStr::new));
        let cli = crate::cli::Cli::try_parse_from(&argv).unwrap();
        let Some(crate::cli::Cmd::Test {
            remote,
            compute,
            config,
            internal_test_configuration,
            internal_test_batch,
            batch_test_units,
            plan,
            ..
        }) = cli.command
        else {
            panic!("expected test command")
        };
        assert!(!remote, "children must not resolve the provider again");
        assert!(compute.is_none());
        assert!(config.is_empty());
        let path = internal_test_configuration.unwrap();
        assert_eq!(path, snapshot.path());
        let child = super::super::Configuration::read(&path).unwrap();
        assert_eq!(child.overrides, configuration.overrides);
        assert_eq!(child.execution, configuration.execution);
        assert_eq!(child.path_suffix, configuration.path_suffix);
        assert_eq!(internal_test_batch, invocation.is_some());
        assert_eq!(plan.is_some(), invocation.is_some());
        assert_eq!(batch_test_units.is_empty(), invocation.is_some());
    }
}

#[test]
fn exact_batch_results_are_scoped_to_the_execution_configuration() {
    let workspace = tempfile::tempdir().unwrap();
    let batch = TestBatch::new("tests", vec!["tests::requested".into()]).unwrap();
    let directory = results_directory(&batch, "/cfg-remote").unwrap();
    std::fs::create_dir_all(workspace.path().join(&directory)).unwrap();
    let path = directory.join("test_results.json");
    std::fs::write(
        workspace.path().join(&path),
        serde_json::to_vec(&results()).unwrap(),
    )
    .unwrap();
    let path = path
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/");
    assert!(load_batch_results(workspace.path(), &batch, Some(&path), "/cfg-remote").is_ok());
    let error =
        load_batch_results(workspace.path(), &batch, Some(&path), "/cfg-other").unwrap_err();
    assert!(error.to_string().contains("must isolate batch outputs"));
}
