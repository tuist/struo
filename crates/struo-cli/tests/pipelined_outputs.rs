#![cfg(unix)]

use std::process::{Command, Output};

const MODULE: &str = r#"
def _ancestor(ctx):
    run_action(argv = ["/bin/sh", "-c", "printf inherited > inherited"], outputs = ["inherited"])
    return {"inherited": "inherited"}

def _producer(ctx):
    mode = ctx["attr"]["mode"]
    script = """set -eu
printf '%s\n' '{"kind":"diagnostic","text":"sample diagnostic"}' >&2
printf 'plain diagnostic\n' >&2
printf metadata > artifacts/metadata
printf '{"kind":"ready"}\n' >&2
if [ "$1" = sandbox ]; then
    sleep 0.1
elif [ "$1" != fallback ]; then
    for i in $(seq 1 500); do
        [ -f consumer-mode ] && break
        sleep 0.01
    done
    test -f consumer-mode
fi
if [ "$1" = fail ]; then exit 17; fi
if [ "$1" = mutate ]; then printf changed > artifacts/metadata; fi
printf linked > artifacts/full
printf finished > producer-finished
"""
    run_action(
        argv = ["/bin/sh", "-c", script, "sh", mode],
        inputs = ["seed"],
        outputs = ["artifacts/metadata", "artifacts/full", "producer-finished"],
        create_dirs = ["artifacts"],
        ready_outputs = [{"path": "artifacts/metadata", "stream": "stderr", "event": {"kind": "absent" if mode == "fallback" else "ready"}, "rendered_field": "text"}],
    )
    if mode == "rewrite":
        run_action(argv = ["/bin/sh", "-c", "printf replaced > artifacts/metadata"], outputs = ["artifacts/metadata"])
    return {"metadata": "artifacts/metadata", "full": "artifacts/full", "inherited": ctx["deps"][0]["inherited"]}

def _consumer(ctx):
    dep = ctx["deps"][0]
    run_action(
        argv = ["/bin/sh", "-c", "set -eu; test \"$(cat artifacts/metadata)\" = metadata; if [ -f producer-finished ]; then printf complete; else printf overlap; fi > consumer-mode"],
        inputs = [dep["metadata"], dep["inherited"]],
        outputs = ["consumer-mode"],
    )
    return {}

def _link(ctx):
    dep = ctx["deps"][0]
    run_action(
        argv = ["/bin/sh", "-c", "set -eu; cat artifacts/full > linked"],
        inputs = [dep["full"]],
        outputs = ["linked"],
    )
    return {}

def _ordinary(ctx):
    if not host_file_exists("producer-finished"):
        fail("ordinary analysis ran before producer completion")
    return {}

ancestor = target_kind(capabilities = [capability("build", ["default"])], impl = _ancestor)
producer = target_kind(attrs = [attr("mode", "string", required = True)], providers = ["artifact"], capabilities = [capability("build", ["default"])], impl = _producer)
consumer = target_kind(accepts_ready_dependencies = True, capabilities = [capability("build", ["default"])], impl = _consumer)
link = target_kind(accepts_ready_dependencies = True, capabilities = [capability("build", ["default"])], impl = _link)
ordinary = target_kind(capabilities = [capability("build", ["default"])], impl = _ordinary)
root = target_kind(capabilities = [capability("build", ["default"])], impl = lambda ctx: {})
"#;

struct Workspace {
    root: tempfile::TempDir,
    home: tempfile::TempDir,
}

impl Workspace {
    fn new(mode: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("modules")).unwrap();
        std::fs::write(root.path().join("modules/kinds.star"), MODULE).unwrap();
        std::fs::write(root.path().join("seed"), "original").unwrap();
        std::fs::write(
            root.path().join("struo.toml"),
            format!(
                r#"[modules]
paths = ["modules/*.star"]
[[target]]
name = "ancestor"
kind = "ancestor"
[[target]]
name = "producer"
kind = "producer"
deps = ["ancestor"]
[target.attrs]
mode = "{mode}"
[[target]]
name = "consumer"
kind = "consumer"
deps = ["producer"]
[[target]]
name = "link"
kind = "link"
deps = ["producer"]
[[target]]
name = "ordinary"
kind = "ordinary"
deps = ["producer"]
[[target]]
name = "root"
kind = "root"
deps = ["consumer", "link", "ordinary"]
"#
            ),
        )
        .unwrap();
        Self {
            root,
            home: tempfile::tempdir().unwrap(),
        }
    }

    fn build(&self, extra: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_struo"))
            .current_dir(self.root.path())
            .args([
                "--format",
                "json",
                "--cpu-limit",
                "4",
                "--memory-limit",
                "3GiB",
            ])
            .args(["build", "root"])
            .args(extra)
            .env("HOME", self.home.path())
            .env("XDG_CACHE_HOME", self.home.path().join("cache"))
            .env("XDG_STATE_HOME", self.home.path().join("state"))
            .env("STRUO_CACHE_PROVIDER", "local")
            .env("STRUO_CHANGE_TRACKER", "0")
            .env_remove("TUIST_TOKEN")
            .output()
            .unwrap()
    }

    fn read(&self, path: &str) -> String {
        std::fs::read_to_string(self.root.path().join(path)).unwrap()
    }
}

fn success(output: &Output) {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn metadata_overlaps_but_link_and_ordinary_analysis_wait_and_cache_restores() {
    let workspace = Workspace::new("overlap");
    success(&workspace.build(&[]));
    assert_eq!(workspace.read("consumer-mode"), "overlap");
    assert_eq!(workspace.read("linked"), "linked");
    for path in ["artifacts", ".struo/out"] {
        let path = workspace.root.path().join(path);
        if path.exists() {
            std::fs::remove_dir_all(path).unwrap();
        }
    }
    for path in ["consumer-mode", "producer-finished", "linked"] {
        std::fs::remove_file(workspace.root.path().join(path)).unwrap();
    }
    success(&workspace.build(&[]));
    assert_eq!(workspace.read("consumer-mode"), "overlap");
    assert_eq!(workspace.read("artifacts/metadata"), "metadata");
    assert_eq!(workspace.read("linked"), "linked");
}

#[test]
fn missing_readiness_event_falls_back_to_completion() {
    let workspace = Workspace::new("fallback");
    success(&workspace.build(&[]));
    assert_eq!(workspace.read("consumer-mode"), "complete");
}

#[test]
fn sandbox_override_waits_for_private_outputs_to_be_copied_back() {
    let workspace = Workspace::new("sandbox");
    success(&workspace.build(&["--sandbox", "inputs"]));
    assert_eq!(workspace.read("linked"), "linked");
}

#[test]
fn later_actions_cannot_rewrite_announced_outputs() {
    let workspace = Workspace::new("rewrite");
    let output = workspace.build(&[]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("overlaps another action write"));
    assert!(!workspace.root.path().join("artifacts/metadata").exists());
}

#[test]
fn producer_failure_or_changed_announced_bytes_cannot_succeed() {
    for mode in ["fail", "mutate"] {
        let workspace = Workspace::new(mode);
        let output = workspace.build(&[]);
        assert!(!output.status.success(), "{mode}");
        assert_eq!(workspace.read("consumer-mode"), "overlap");
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(
            error.contains(if mode == "fail" {
                "17"
            } else {
                "changed after readiness"
            }),
            "{error}"
        );
        if mode == "fail" {
            let record: serde_json::Value =
                serde_json::from_str(error.lines().last().unwrap()).unwrap();
            let message = record["error"]["message"].as_str().unwrap();
            let stderr = message.rsplit("\n\nstderr:\n").next().unwrap();
            assert_eq!(stderr, "sample diagnosticplain diagnostic\n");
        }
    }
}

#[test]
fn rust_libraries_preserve_explicit_color_flags() {
    for flags in [vec!["--color=always"], vec!["--color", "never"]] {
        let workspace = Workspace {
            root: tempfile::tempdir().unwrap(),
            home: tempfile::tempdir().unwrap(),
        };
        std::fs::create_dir(workspace.root.path().join("src")).unwrap();
        std::fs::write(
            workspace.root.path().join("src/lib.rs"),
            "pub fn value() -> u32 { 42 }\n",
        )
        .unwrap();
        std::fs::write(
            workspace.root.path().join("struo.toml"),
            format!(
                "[[target]]\nname = \"root\"\nkind = \"rust_library\"\n[target.attrs]\nrustc_flags = {}\n",
                serde_json::to_string(&flags).unwrap()
            ),
        )
        .unwrap();
        success(&workspace.build(&[]));
    }
}
