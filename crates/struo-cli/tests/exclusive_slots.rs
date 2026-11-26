//! Actions that claim the same named slot never overlap, even when
//! `struo test` runs their batches in separate worker processes, the way two
//! test bundles hosted by one app must not share a simulator at once.
#![cfg(unix)]

use std::process::Command;

use serde_json::Value;

const RUNNER: &str = r#"mkdir -p running
touch "running/$1"
echo "$1" >> ran
i=0
while [ "$i" -lt 8 ]; do
  if [ "$(ls running | wc -l)" -gt 1 ]; then echo "$1" >> overlapped; fi
  sleep 0.05
  i=$((i + 1))
done
rm "running/$1"
cp "$2" "$3"
"#;

#[test]
fn batches_claiming_one_slot_run_one_at_a_time_across_workers() {
    let workspace = tempfile::tempdir().unwrap();
    let root = workspace.path();
    std::fs::create_dir(root.join("modules")).unwrap();
    std::fs::write(
        root.join("modules/device.star"),
        include_str!("support/device.star"),
    )
    .unwrap();
    std::fs::write(root.join("runner.sh"), RUNNER).unwrap();
    std::fs::write(
        root.join("struo.toml"),
        r#"[modules]
paths = ["modules/*.star"]
[[target]]
name = "first"
kind = "device_test"
[[target]]
name = "second"
kind = "device_test"
"#,
    )
    .unwrap();
    let home = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_struo"))
        .current_dir(root)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join("config"))
        .env("XDG_CACHE_HOME", home.path().join("cache"))
        .env("XDG_STATE_HOME", home.path().join("state"))
        .env("XDG_DATA_HOME", home.path().join("data"))
        .env("XDG_RUNTIME_DIR", home.path().join("runtime"))
        .args(["--format", "json", "test", "--all", "--jobs", "2"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let overlapped = std::fs::read_to_string(root.join("overlapped")).unwrap_or_default();
    assert!(
        overlapped.is_empty(),
        "batches holding the `device` slot overlapped: {overlapped}\n{stdout}\n{stderr}"
    );
    assert!(output.status.success(), "{stdout}\n{stderr}");
    let report: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(report["schedule"]["workers"], 2, "{stdout}");
    let mut ran = std::fs::read_to_string(root.join("ran"))
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect::<Vec<_>>();
    ran.sort();
    assert_eq!(ran, ["first", "second"]);
}
