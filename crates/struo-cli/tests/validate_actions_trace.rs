#![cfg(any(
    all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ),
    target_os = "macos"
))]

use std::process::Command;

const MODULE: &str = r#"
def _impl(ctx):
    run_action(
        argv = ["/bin/sh", "-c", "cat declared.txt a.txt > out.txt; cat b.txt >> out.txt; cat ../../../../walked.txt >> out.txt; cat optional.txt > /dev/null 2>&1; true"],
        inputs = ["declared.txt"],
        outputs = ["out.txt"],
        identifier = "probe",
    )
    return {}

probe = target_kind(capabilities = [capability("build", ["default"])], impl = _impl)
"#;

/// The trace reports every undeclared read in one validation: reads the
/// sandbox made fail, a read whose failure the action ignored, and a read
/// that walked out of the execroot into the real workspace. A file the action
/// never touched is not reported.
#[test]
fn validate_actions_reports_traced_undeclared_reads() {
    let root = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    for file in [
        "declared.txt",
        "a.txt",
        "b.txt",
        "walked.txt",
        "optional.txt",
        "untouched.txt",
    ] {
        std::fs::write(root.path().join(file), file).unwrap();
    }
    std::fs::write(root.path().join("kinds.star"), MODULE).unwrap();
    std::fs::write(
        root.path().join("struo.toml"),
        "[modules]\npaths = [\"kinds.star\"]\n\n[[target]]\nname = \"app\"\nkind = \"probe\"\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_struo"))
        .args(["query", "validate-actions", "app", "--format", "json"])
        .current_dir(root.path())
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", home.path())
        .env("XDG_CACHE_HOME", home.path().join("cache"))
        .env("XDG_STATE_HOME", home.path().join("state"))
        .env("STRUO_CHANGE_TRACKER", "0")
        .output()
        .unwrap();
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
        panic!("{}", String::from_utf8_lossy(&output.stderr));
    });

    assert_eq!(report["valid"], false);
    let mut repairs = report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|diagnostic| diagnostic["code"] == "undeclared_read")
        .map(|diagnostic| diagnostic["repairs"][0].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    repairs.sort();
    assert_eq!(
        repairs,
        [
            "Add `a.txt` to the action inputs",
            "Add `b.txt` to the action inputs",
            "Add `optional.txt` to the action inputs",
            "Add `walked.txt` to the action inputs",
        ]
    );
    // macOS observes only denied reads, so it reports no host reads.
    #[cfg(target_os = "linux")]
    assert!(report["actions"][0]["host_reads"]
        .as_array()
        .unwrap()
        .iter()
        .any(|path| path.as_str().unwrap().ends_with("/cat")));
}
