use std::fs;

use tempfile::TempDir;

use super::*;

fn xdg_under(root: &Path) -> Xdg {
    Xdg {
        cache_home: root.join("cache"),
        state_home: root.join("state"),
        data_home: root.join("data"),
        config_home: root.join("config"),
        runtime_dir: root.join("runtime"),
    }
}

#[test]
fn finds_script_after_runtime_args() {
    let tmp = TempDir::new().unwrap();
    let script = tmp.path().join("scripts").join("build.py");
    fs::create_dir_all(script.parent().unwrap()).unwrap();
    fs::write(&script, "#!/usr/bin/env python3\nprint('hi')\n").unwrap();

    let argv = vec![
        "python3".to_string(),
        "-O".to_string(),
        "scripts/build.py".to_string(),
        "--flag".to_string(),
    ];

    let (runtime, runtime_args, script_arg, script_args) =
        parse_script_exec_argv(tmp.path(), &argv).unwrap();

    assert_eq!(runtime, "python3");
    assert_eq!(runtime_args, &["-O".to_string()]);
    assert_eq!(script_arg, "scripts/build.py");
    assert_eq!(script_args, vec!["--flag".to_string()]);
}

#[test]
fn local_aliases_use_native_runtime_and_environment_resolution() {
    assert!(!uses_provider_environment(None).unwrap());
    let mut binding = RemoteExecution::provider("native-alias");
    binding.executor = Some("local".to_string());
    assert!(!uses_provider_environment(Some(&binding)).unwrap());
    for name in [
        "e2b",
        "daytona",
        "together",
        "sprites",
        "microsandbox",
        "cloudflare",
        "opensandbox",
    ] {
        assert!(uses_provider_environment(Some(&RemoteExecution::provider(name))).unwrap());
    }
}

#[test]
fn computes_relative_paths_from_cwd_to_script() {
    assert_eq!(relative_path("scripts", "scripts/build.sh"), "build.sh");
    assert_eq!(
        relative_path("tools/gen", "scripts/build.sh"),
        "../../scripts/build.sh"
    );
    assert_eq!(relative_path("", "scripts/build.sh"), "scripts/build.sh");
}

#[test]
fn normalizes_script_relative_paths_before_workspace_validation() {
    let script_dir = WorkspacePath::try_from("scripts").unwrap();

    let input = normalize_from_script_dir(&script_dir, "../input.txt").unwrap();
    assert_eq!(input.as_str(), "input.txt");

    let err = normalize_from_script_dir(&script_dir, "../../escape.txt").unwrap_err();
    assert!(
        err.to_string().contains("must not escape the workspace"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn autodetects_annotated_scripts_without_script_flag() {
    let tmp = TempDir::new().unwrap();
    let cache = CacheProvider::Local(struo_cas::Cas::open(tmp.path().join(".cache")));
    let script = tmp.path().join("scripts").join("build.sh");
    fs::create_dir_all(script.parent().unwrap()).unwrap();
    fs::write(
        &script,
        "#!/bin/bash\n# struo input \"../input.txt\"\ncat ../input.txt\n",
    )
    .unwrap();
    fs::write(tmp.path().join("input.txt"), "hello\n").unwrap();

    let plan = autodetected_script_action(
        &cache,
        RunOpts::default(),
        tmp.path(),
        &ScriptExecutionOptions {
            explicit_env: Vec::new(),
            cwd_override: None,
            timeout_ms_override: None,
            remote_override: None,
            sandbox: SandboxMode::default(),
            network: NetworkPolicy::default(),
            resource_limits: ResourceLimits::default(),
            xdg: None,
        },
        &["/bin/bash".to_string(), "scripts/build.sh".to_string()],
    )
    .await
    .unwrap()
    .expect("annotated script should be autodetected");

    let Action::RunCommand {
        argv,
        cwd,
        input_digest,
        ..
    } = plan.1
    else {
        panic!("script autodetection should produce a command action");
    };
    assert_eq!(argv, vec!["/bin/bash".to_string(), "build.sh".to_string()]);
    assert_eq!(cwd.unwrap().as_str(), "scripts");
    assert!(input_digest.is_some());
}

#[tokio::test]
async fn remote_script_resolves_named_executor_and_dependency_outputs() {
    for (config, executor, environment) in [
        (
            "kind = \"microsandbox\"\nimage = \"node:22.18.0-alpine\"",
            "microsandbox",
            Some("node:22.18.0-alpine"),
        ),
        ("kind = \"sprites\"", "sprites", None),
    ] {
        let tmp = TempDir::new().unwrap();
        let xdg = xdg_under(tmp.path());
        let script = tmp.path().join("scripts").join("test.sh");
        fs::create_dir_all(script.parent().unwrap()).unwrap();
        fs::write(
            tmp.path().join("struo.toml"),
            format!("[infrastructures.remote_tests]\n{config}\n"),
        )
        .unwrap();
        fs::write(
            &script,
            "#!/bin/sh\n# struo remote \"remote_tests\"\n# struo input \"../tests\"\ntrue\n",
        )
        .unwrap();
        fs::create_dir(tmp.path().join("tests")).unwrap();

        let invocation = script_invocation(
            tmp.path(),
            BTreeMap::new(),
            None,
            None,
            None,
            SandboxMode::Off,
            Some(&xdg),
            &["/bin/sh".to_string(), "scripts/test.sh".to_string()],
        )
        .await
        .unwrap();
        let action = action_from_script_invocation(
            &invocation,
            None,
            &[DependencyFingerprint {
                script_path: "scripts/install.sh".to_string(),
                digest: Digest::of_bytes(b"dependencies"),
                outputs: vec![WorkspacePath::try_from("node_modules").unwrap()],
            }],
        )
        .await
        .unwrap();

        let Action::RunCommand {
            argv,
            env,
            inputs,
            remote,
            ..
        } = action
        else {
            panic!("script action should produce a command action");
        };
        assert_eq!(argv, vec!["/bin/sh".to_string(), "test.sh".to_string()]);
        assert!(env.is_empty());
        assert!(inputs.iter().any(|input| input.as_str() == "node_modules"));
        assert_eq!(
            remote,
            Some(Box::new(RemoteExecution {
                provider: "remote_tests".to_string(),
                executor: Some(executor.to_string()),
                environment: environment.map(str::to_string),
                account: None,
                project: None,
                properties: BTreeMap::new(),
            }))
        );
    }
}

#[tokio::test]
async fn northflank_annotation_preserves_configured_project_and_team() {
    let tmp = TempDir::new().unwrap();
    let xdg = xdg_under(tmp.path());
    fs::write(
        tmp.path().join("struo.toml"),
        r#"
[infrastructures.remote_tests]
kind = "northflank"
image = "node:22"
deployment_plan = "nf-compute-200"

[infrastructure.execution]
provider = "remote_tests"
account = "team"
project = "tests"
"#,
    )
    .unwrap();
    fs::write(
        tmp.path().join("test.sh"),
        "#!/bin/sh\n# struo remote \"remote_tests\"\ntrue\n",
    )
    .unwrap();
    let invocation = script_invocation(
        tmp.path(),
        BTreeMap::new(),
        None,
        None,
        None,
        SandboxMode::Off,
        Some(&xdg),
        &["/bin/sh".to_string(), "test.sh".to_string()],
    )
    .await
    .unwrap();
    let action = action_from_script_invocation(&invocation, None, &[])
        .await
        .unwrap();
    let remote = action_remote(&action).unwrap();
    assert_eq!(remote.provider, "remote_tests");
    assert_eq!(remote.account.as_deref(), Some("team"));
    assert_eq!(remote.project.as_deref(), Some("tests"));
    assert_eq!(remote.environment.as_deref(), Some("node:22"));
    assert_eq!(remote.properties["deployment_plan"], "nf-compute-200");
    assert_eq!(
        Executor::from_binding(remote).unwrap(),
        Executor::Northflank
    );
}

#[tokio::test]
async fn script_annotation_sets_output_symlink_mode() {
    let tmp = TempDir::new().unwrap();
    let script = tmp.path().join("scripts").join("build.sh");
    fs::create_dir_all(script.parent().unwrap()).unwrap();
    fs::write(
        &script,
        "#!/bin/bash\n# struo output-symlinks \"preserve\"\ntrue\n",
    )
    .unwrap();

    let (_, action) = script_action(
        tmp.path(),
        Vec::new(),
        None,
        None,
        None,
        &["/bin/bash".to_string(), "scripts/build.sh".to_string()],
    )
    .await
    .unwrap();

    let Action::RunCommand {
        output_symlink_mode,
        ..
    } = action
    else {
        panic!("script action should produce a command action");
    };
    assert_eq!(output_symlink_mode, OutputSymlinkMode::Preserve);
}

#[tokio::test]
async fn rejects_invalid_output_symlink_mode() {
    let tmp = TempDir::new().unwrap();
    let script = tmp.path().join("scripts").join("build.sh");
    fs::create_dir_all(script.parent().unwrap()).unwrap();
    fs::write(
        &script,
        "#!/bin/bash\n# struo output-symlinks \"copy-everything\"\ntrue\n",
    )
    .unwrap();

    let err = script_action(
        tmp.path(),
        Vec::new(),
        None,
        None,
        None,
        &["/bin/bash".to_string(), "scripts/build.sh".to_string()],
    )
    .await
    .unwrap_err();

    assert!(err.to_string().contains("parsing output-symlinks"));
}

#[test]
fn rejects_relative_script_paths_that_escape_the_workspace() {
    let tmp = TempDir::new().unwrap();
    let outside = tmp.path().parent().unwrap().join("outside.sh");
    fs::write(&outside, "#!/bin/bash\n").unwrap();

    let err = parse_script_exec_argv(
        tmp.path(),
        &["bash".to_string(), "../outside.sh".to_string()],
    )
    .unwrap_err();

    assert!(err
        .to_string()
        .contains("must stay within the selected workspace"));
}

#[test]
fn rejects_globs_that_match_too_many_files() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join("scripts")).unwrap();
    fs::create_dir_all(tmp.path().join("scripts/src")).unwrap();
    fs::write(tmp.path().join("scripts/src/one.txt"), "1\n").unwrap();
    fs::write(tmp.path().join("scripts/src/two.txt"), "2\n").unwrap();

    let script_dir = WorkspacePath::try_from("scripts").unwrap();
    let err = expand_script_globs_with_limit(tmp.path(), &script_dir, "src/*.txt", 1).unwrap_err();

    assert!(err.to_string().contains("matched more than 1 files"));
}

#[test]
fn fingerprint_shell_uses_stable_system_shell() {
    let shell = fingerprint_shell();

    #[cfg(windows)]
    {
        assert_eq!(shell.command_arg, "/C");
        assert!(!shell.program.is_empty());
    }

    #[cfg(not(windows))]
    {
        assert_eq!(shell.program, "/bin/sh");
        assert_eq!(shell.command_arg, "-c");
    }
}

#[cfg(not(windows))]
#[tokio::test]
async fn fingerprint_commands_respect_script_timeout() {
    let tmp = TempDir::new().unwrap();
    let invocation = ScriptInvocation {
        workspace: tmp.path().to_path_buf(),
        runtime: "/bin/sh".to_string(),
        runtime_args: Vec::new(),
        script_path: WorkspacePath::try_from("build.sh").unwrap(),
        script_args: Vec::new(),
        needs: Vec::new(),
        fingerprints: vec!["sleep 5".to_string()],
        inputs: Vec::new(),
        cwd: WorkspacePath::root(),
        env: BTreeMap::from([("PATH".to_string(), "/usr/bin:/bin".to_string())]),
        outputs: Vec::new(),
        output_symlink_mode: OutputSymlinkMode::default(),
        timeout_ms: Some(10),
        remote: None,
        sandbox: SandboxMode::default(),
        network: NetworkPolicy::default(),
    };

    let err = run_fingerprint_commands(&invocation).await.unwrap_err();

    assert!(err.to_string().contains("timed out after 10ms"));
}

#[tokio::test]
async fn does_not_autodetect_unannotated_scripts() {
    let tmp = TempDir::new().unwrap();
    let cache = CacheProvider::Local(struo_cas::Cas::open(tmp.path().join(".cache")));
    let script = tmp.path().join("scripts").join("build.sh");
    fs::create_dir_all(script.parent().unwrap()).unwrap();
    fs::write(&script, "#!/bin/bash\ncat ../input.txt\n").unwrap();

    let detected = autodetected_script_action(
        &cache,
        RunOpts::default(),
        tmp.path(),
        &ScriptExecutionOptions {
            explicit_env: Vec::new(),
            cwd_override: None,
            timeout_ms_override: None,
            remote_override: None,
            sandbox: SandboxMode::default(),
            network: NetworkPolicy::default(),
            resource_limits: ResourceLimits::default(),
            xdg: None,
        },
        &["/bin/bash".to_string(), "scripts/build.sh".to_string()],
    )
    .await
    .unwrap();

    assert!(detected.is_none());
}

#[tokio::test]
async fn autodetection_rejects_malformed_struo_directives() {
    let tmp = TempDir::new().unwrap();
    let cache = CacheProvider::Local(struo_cas::Cas::open(tmp.path().join(".cache")));
    let script = tmp.path().join("build.sh");
    fs::write(&script, "#!/bin/sh\n# struo ouput \"result.txt\"\ntrue\n").unwrap();

    let error = autodetected_script_action(
        &cache,
        RunOpts::default(),
        tmp.path(),
        &ScriptExecutionOptions {
            explicit_env: Vec::new(),
            cwd_override: None,
            timeout_ms_override: None,
            remote_override: None,
            sandbox: SandboxMode::default(),
            network: NetworkPolicy::default(),
            resource_limits: ResourceLimits::default(),
            xdg: None,
        },
        &["/bin/sh".to_string(), "build.sh".to_string()],
    )
    .await
    .unwrap_err();

    assert!(error.to_string().contains("unknown struo directive"));
}
