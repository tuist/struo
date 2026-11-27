use std::path::Path;
use std::process::Command;

const MODULE: &str = r#"
def _impl(ctx):
    tool_home = host_env("XDG_DATA_HOME") + "/struo/tools/mise-data/installs/tool/1.0"
    out = declare_output("out.txt")
    run_action(
        argv = [tool_home + "/bin/tool", out],
        outputs = [out],
        env = {"TOOL_HOME": tool_home},
        toolchain_identity = "tool\x00" + tool_home + "/bin/tool",
        identifier = "touch",
    )
    return {}

managed_tool = target_kind(capabilities = [capability("build", ["default"])], impl = _impl)
"#;

fn build_digest(home: &Path) -> String {
    let tool = home.join("data/struo/tools/mise-data/installs/tool/1.0/bin/tool");
    std::fs::create_dir_all(tool.parent().unwrap()).unwrap();
    std::fs::write(&tool, "#!/bin/sh\nprintf tool > \"$1\"\n").unwrap();
    std::fs::set_permissions(&tool, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let workspace = home.join("checkout");
    std::fs::create_dir_all(workspace.join("modules")).unwrap();
    std::fs::write(workspace.join("modules/tool.star"), MODULE).unwrap();
    std::fs::write(
        workspace.join("struo.toml"),
        "[modules]\npaths = [\"modules/*.star\"]\n\n[[target]]\nname = \"Tool\"\nkind = \"managed_tool\"\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_struo"))
        .arg("-C")
        .arg(&workspace)
        .args(["--format", "json", "build", "Tool"])
        .env("HOME", home)
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_STATE_HOME", home.join("state"))
        .env("STRUO_CACHE_PROVIDER", "local")
        .env("STRUO_CHANGE_TRACKER", "0")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        std::fs::read_to_string(workspace.join(".struo/out/Tool/out.txt")).unwrap(),
        "tool"
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    report["action_digest"].as_str().unwrap().to_string()
}

#[test]
fn paths_inside_struo_data_dir_do_not_change_action_keys_across_users() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    assert_eq!(build_digest(first.path()), build_digest(second.path()));
}
