use std::process::Command;

use serde_json::Value;
use tempfile::TempDir;

use super::{all_prelude_source, eval_prelude_string_function_in};

#[test]
#[allow(clippy::too_many_lines)]
fn isolated_go_coverage_attributes_cases_and_rejects_unresolved_profiles() {
    if !go_process_tracing_available() {
        eprintln!("complete Go coverage requires usable Linux process tracing");
        return;
    }
    let directory = TempDir::new().unwrap();
    let root = directory.path();
    for (name, contents) in [
        ("go.mod", "module example.com/coverage\n\ngo 1.23\n"),
        ("alpha.go", "package coverage\nfunc Alpha() int { return 3 }\n"),
        ("beta.go", "package coverage\nfunc Beta() int { return 7 }\n"),
        ("coverage_test.go", "package coverage\nimport \"testing\"\nfunc TestAlpha(t *testing.T) { if Alpha() != 3 { t.Fatal(\"alpha\") } }\nfunc TestBeta(t *testing.T) { if Beta() != 7 { t.Fatal(\"beta\") } }\n"),
    ] {
        std::fs::write(root.join(name), contents).unwrap();
    }
    let runner =
        eval_prelude_string_function_in(all_prelude_source(), "_go_test_runner_source", "()")
            .unwrap();
    std::fs::write(root.join("runner.go"), runner).unwrap();
    let build = Command::new("go")
        .args(["build", "-o", "runner", "runner.go"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(build.status.success(), "{build:?}");
    std::fs::remove_file(root.join("runner.go")).unwrap();
    let build = Command::new("go")
        .args(["test", "-c", "-cover", "-coverpkg=.", "-o", "tests"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(build.status.success(), "{build:?}");
    let run = |module_root: &str, extra: &[&str]| {
        let output = Command::new(root.join("runner"))
            .args([
                "go",
                "example.com/coverage",
                "tests",
                "out/results.json",
                "out/log",
                "out/native",
                "out/coverage.out",
                "opaque-target",
                "--",
            ])
            .args(extra)
            .env("STRUO_GO_COVERAGE_PER_TEST", "1")
            .env("STRUO_GO_MODULE_ROOT", module_root)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        serde_json::from_slice::<Value>(&std::fs::read(root.join("out/results.json")).unwrap())
            .unwrap()
    };
    let report = run(".", &[]);
    assert_eq!(report["coverage"]["partial"], false);
    assert_eq!(report["coverage_evidence"]["status"], "collected");
    let paths = report["coverage_evidence"]["paths"].as_array().unwrap();
    let scopes = report["coverage_evidence"]["scopes"].as_array().unwrap();
    assert_eq!(scopes.len(), 2);
    for (name, path) in [("TestAlpha", "alpha.go"), ("TestBeta", "beta.go")] {
        let scope = scopes
            .iter()
            .find(|scope| scope["name"] == format!("opaque-target::{name}"))
            .unwrap();
        let covered = scope["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|index| {
                paths[usize::try_from(index.as_u64().unwrap()).unwrap()]
                    .as_str()
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert_eq!(covered, [path]);
    }
    assert_eq!(report["coverage"]["files"].as_array().unwrap().len(), 2);
    std::fs::write(root.join("benchmark_test.go"), "package coverage\nimport (\"testing\"; \"os\")\nfunc BenchmarkAlpha(b *testing.B) { if Alpha() != 3 { b.Fatal(\"alpha\") }; if err := os.WriteFile(\"benchmark-ran\", []byte(\"executed\"), 0600); err != nil { b.Fatal(err) } }\n").unwrap();
    let build = Command::new("go")
        .args(["test", "-c", "-cover", "-coverpkg=.", "-o", "tests"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(build.status.success(), "{build:?}");
    let benchmark = run(
        ".",
        &[
            "-test.run=^$",
            "-test.bench=BenchmarkAlpha$",
            "-test.benchtime=1x",
        ],
    );
    assert_eq!(benchmark["status"], "passed");
    assert_eq!(
        std::fs::read(root.join("benchmark-ran")).unwrap(),
        b"executed"
    );
    assert_ne!(benchmark["coverage"]["partial"], false);
    assert_ne!(benchmark["coverage_evidence"]["status"], "collected");
    std::fs::remove_file(root.join("benchmark_test.go")).unwrap();
    let build = Command::new("go")
        .args(["test", "-c", "-cover", "-coverpkg=.", "-o", "tests"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(build.status.success(), "{build:?}");
    let incomplete = run("missing-module", &[]);
    assert_eq!(incomplete["coverage"]["partial"], true);
    assert_eq!(incomplete["coverage_evidence"]["status"], "failed");
    assert!(incomplete["coverage_evidence"]["scopes"]
        .as_array()
        .unwrap()
        .is_empty());
    let skipped = run(".", &["-test.skip=TestBeta"]);
    assert_eq!(skipped["coverage"]["partial"], true);
    assert_eq!(skipped["coverage_evidence"]["status"], "failed");
    std::fs::write(root.join("dynamic_test.go"), "package coverage\nimport \"testing\"\nfunc TestDynamic(t *testing.T) { t.Run(\"child\", func(t *testing.T) { Alpha() }) }\n").unwrap();
    let build = Command::new("go")
        .args(["test", "-c", "-cover", "-coverpkg=.", "-o", "tests"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(build.status.success(), "{build:?}");
    let dynamic = run(".", &[]);
    assert_eq!(dynamic["coverage"]["partial"], true);
    assert_eq!(dynamic["coverage_evidence"]["status"], "failed");
    std::fs::write(root.join("main_test.go"), "package coverage\nimport (\"testing\"; \"os\"; \"fmt\"; \"strings\")\nfunc TestMain(m *testing.M) { for _, arg := range os.Args { if strings.HasPrefix(arg, \"-test.list=\") { fmt.Println(\"TestAlpha\\nTestNotReal\"); os.Exit(9) } }; os.Exit(m.Run()) }\n").unwrap();
    let build = Command::new("go")
        .args(["test", "-c", "-cover", "-coverpkg=.", "-o", "tests"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(build.status.success(), "{build:?}");
    for arguments in [vec![], vec!["-test.bench=."]] {
        let discovery = Command::new(root.join("runner"))
            .args([
                "go",
                "example.com/coverage",
                "tests",
                "out/inventory.json",
                "out/log",
                "out/native",
                "out/coverage.out",
                "opaque-target",
                "--",
            ])
            .args(&arguments)
            .env("STRUO_GO_DISCOVER", "1")
            .env("STRUO_GO_COVERAGE_PER_TEST", "1")
            .current_dir(root)
            .output()
            .unwrap();
        assert_eq!(
            discovery.status.code(),
            Some(2),
            "partial listing was accepted with {arguments:?}: {discovery:?}"
        );
        assert!(!root.join("out/inventory.json").exists());
    }
}

#[test]
fn isolated_go_coverage_preserves_requested_benchmark_failures() {
    let directory = TempDir::new().unwrap();
    let root = directory.path();
    std::fs::write(
        root.join("go.mod"),
        "module example.com/benchmarks\n\ngo 1.23\n",
    )
    .unwrap();
    std::fs::write(
        root.join("benchmark_test.go"),
        "package benchmarks\nimport \"testing\"\nfunc BenchmarkMustFail(b *testing.B) { b.Fatal(\"benchmark was executed\") }\n",
    )
    .unwrap();
    let runner =
        eval_prelude_string_function_in(all_prelude_source(), "_go_test_runner_source", "()")
            .unwrap();
    std::fs::write(root.join("runner.go"), runner).unwrap();
    let build = Command::new("go")
        .args(["build", "-o", "runner", "runner.go"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(build.status.success(), "{build:?}");
    std::fs::remove_file(root.join("runner.go")).unwrap();
    let build = Command::new("go")
        .args(["test", "-c", "-cover", "-o", "tests"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(build.status.success(), "{build:?}");
    for arguments in [
        vec!["--", "-test.bench=.", "-test.benchtime=1x"],
        vec!["--", "-test.bench", ".", "-test.benchtime=1x"],
        vec![
            "--struo-filter",
            "BenchmarkMustFail",
            "--",
            "-test.benchtime=1x",
        ],
    ] {
        for isolated in ["0", "1"] {
            let output = Command::new(root.join("runner"))
                .args([
                    "go",
                    "example.com/benchmarks",
                    "tests",
                    "out/results.json",
                    "out/log",
                    "out/native",
                    "out/coverage.out",
                    "opaque-target",
                ])
                .args(&arguments)
                .env("STRUO_GO_COVERAGE_PER_TEST", isolated)
                .env("STRUO_GO_MODULE_ROOT", ".")
                .current_dir(root)
                .output()
                .unwrap();
            assert_eq!(
                output.status.code(),
                Some(1),
                "{arguments:?}, isolated={isolated}: {output:?}"
            );
            let report: Value =
                serde_json::from_slice(&std::fs::read(root.join("out/results.json")).unwrap())
                    .unwrap();
            assert_eq!(report["status"], "failed");
            assert!(report["cases"].as_array().unwrap().iter().any(|case| {
                case["id"] == "opaque-target::BenchmarkMustFail" && case["status"] == "failed"
            }));
            assert_ne!(report["coverage_evidence"]["status"], "collected");
        }
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn isolated_go_coverage_rejects_child_only_product_execution() {
    let directory = child_coverage_fixture();
    let root = directory.path();
    let compile = || {
        let build = Command::new("go")
            .args(["test", "-c", "-cover", "-coverpkg=.", "-o", "tests"])
            .current_dir(root)
            .output()
            .unwrap();
        assert!(build.status.success(), "{build:?}");
    };
    let run = |spawn: bool, trace_available: bool| {
        let mut command = child_coverage_command(root);
        command
            .arg("--")
            .env("SPAWN_CHILD", if spawn { "1" } else { "0" });
        if !trace_available {
            command.env("PATH", root.join("missing-tools"));
        }
        let output = command.output().unwrap();
        let report: Value =
            serde_json::from_slice(&std::fs::read(root.join("out/results.json")).unwrap()).unwrap();
        (output, report)
    };
    let (output, ordinary) = run(false, true);
    assert!(output.status.success(), "{output:?}");
    let can_trace = go_process_tracing_available();
    assert_eq!(ordinary["coverage"]["partial"], !can_trace, "{ordinary}");
    if can_trace {
        assert_eq!(
            ordinary["coverage_evidence"]["paths"],
            serde_json::json!(["parent.go"])
        );
        assert_eq!(
            ordinary["coverage_evidence"]["scopes"][0]["files"],
            serde_json::json!([0])
        );
    }
    let (untraced_output, untraced) = run(false, false);
    assert!(untraced_output.status.success(), "{untraced_output:?}");
    let (output, child) = run(true, true);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(child["status"], "passed");
    let files = child["coverage"]["files"].as_array().unwrap();
    for (path, count) in [("parent.go", 1), ("product.go", 0)] {
        let file = files.iter().find(|file| file["path"] == path).unwrap();
        assert_eq!(file["execution_counts"], serde_json::json!([count]));
    }
    for report in [&child, &untraced] {
        assert_eq!(report["coverage"]["partial"], true, "{report}");
        assert_eq!(report["coverage_evidence"]["status"], "failed");
        assert_eq!(report["coverage_evidence"]["scopes"], serde_json::json!([]));
    }
    std::fs::write(
        root.join("product.go"),
        "package child\nfunc Product() int { return 4 }\n",
    )
    .unwrap();
    compile();
    let (output, broken) = run(true, true);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(broken["status"], "failed");
    assert_eq!(broken["coverage"]["partial"], true);
    assert_eq!(broken["coverage_evidence"]["status"], "failed");
}

fn child_coverage_fixture() -> TempDir {
    let directory = TempDir::new().unwrap();
    let root = directory.path();
    for (name, contents) in [
        ("go.mod", "module example.com/child\n\ngo 1.23\n"),
        (
            "parent.go",
            "package child\nfunc Parent() int { return 7 }\n",
        ),
        (
            "product.go",
            "package child\nfunc Product() int { return 3 }\n",
        ),
        (
            "child_test.go",
            r#"package child
import ("fmt"; "os"; "os/exec"; "testing"; "time")
func TestMain(m *testing.M) {
    if os.Getenv("BACKGROUND_HELPER") == "1" {
        os.WriteFile("helper-started", []byte("started"), 0600)
        for { if _, err := os.Stat("release-helper"); err == nil { break }; time.Sleep(10*time.Millisecond) }
        os.WriteFile("helper-finished", []byte("finished"), 0600)
        os.Exit(0)
    }
    if os.Getenv("PRODUCT_CHILD") == "1" { fmt.Println(Product()); os.Exit(0) }
    os.Exit(m.Run())
}
func TestChild(t *testing.T) {
    executions, err := os.OpenFile("executions", os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0600)
    if err != nil { t.Fatal(err) }; executions.WriteString("run\n"); executions.Close()
    if Parent() != 7 { t.Fatal("parent") }
    if os.Getenv("BACKGROUND_CHILD") == "1" {
        executable, err := os.Executable(); if err != nil { t.Fatal(err) }
        command := exec.Command(executable)
        command.Env = append(os.Environ(), "BACKGROUND_HELPER=1")
        if err := command.Start(); err != nil { t.Fatal(err) }
        for { if _, err := os.Stat("helper-started"); err == nil { break }; time.Sleep(10*time.Millisecond) }
        if os.Getenv("FAIL_AFTER_BACKGROUND") == "1" { t.Fatal("failure after spawning helper") }
        return
    }
    if os.Getenv("SPAWN_CHILD") != "1" { return }
    executable, err := os.Executable()
    if err != nil { t.Fatal(err) }
    command := exec.Command(executable)
    command.Env = append(os.Environ(), "PRODUCT_CHILD=1")
    output, err := command.Output()
    if err != nil || string(output) != "3\n" { t.Fatalf("child output %q: %v", output, err) }
}
"#,
        ),
    ] {
        std::fs::write(root.join(name), contents).unwrap();
    }
    let runner =
        eval_prelude_string_function_in(all_prelude_source(), "_go_test_runner_source", "()")
            .unwrap();
    std::fs::write(root.join("runner.go"), runner).unwrap();
    let build = Command::new("go")
        .args(["build", "-o", "runner", "runner.go"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(build.status.success(), "{build:?}");
    std::fs::remove_file(root.join("runner.go")).unwrap();
    let build = Command::new("go")
        .args(["test", "-c", "-cover", "-coverpkg=.", "-o", "tests"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(build.status.success(), "{build:?}");
    directory
}

fn child_coverage_command(root: &std::path::Path) -> Command {
    let goroot = Command::new("go").args(["env", "GOROOT"]).output().unwrap();
    assert!(goroot.status.success(), "{goroot:?}");
    let go =
        std::path::PathBuf::from(String::from_utf8(goroot.stdout).unwrap().trim()).join("bin/go");
    let mut command = Command::new(root.join("runner"));
    command
        .arg(go)
        .args([
            "example.com/child",
            "tests",
            "out/results.json",
            "out/log",
            "out/native",
            "out/coverage.out",
            "opaque-target",
        ])
        .env("STRUO_GO_COVERAGE_PER_TEST", "1")
        .env("STRUO_GO_MODULE_ROOT", ".")
        .current_dir(root);
    command
}

fn go_process_tracing_available() -> bool {
    cfg!(target_os = "linux")
        && Command::new("strace")
            .args([
                "-I",
                "1",
                "-f",
                "-e",
                "trace=process",
                "-e",
                "raw=execve,execveat",
                "-o",
                "/dev/null",
                "go",
                "version",
            ])
            .output()
            .is_ok_and(|output| output.status.success())
}

#[test]
fn isolated_go_coverage_preserves_background_child_lifetime() {
    for failure in [false, true] {
        let directory = child_coverage_fixture();
        let root = directory.path();
        let mut child = child_coverage_command(root)
            .env("BACKGROUND_CHILD", "1")
            .env("FAIL_AFTER_BACKGROUND", if failure { "1" } else { "0" })
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let completed = loop {
            if child.try_wait().unwrap().is_some() {
                break true;
            }
            if std::time::Instant::now() >= deadline {
                break false;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        std::fs::write(root.join("release-helper"), "release").unwrap();
        let output = child.wait_with_output().unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !root.join("helper-finished").exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            completed,
            "runner waited for the background helper: {output:?}"
        );
        assert_eq!(output.status.code(), Some(i32::from(failure)), "{output:?}");
        assert!(root.join("helper-started").exists());
        assert!(
            root.join("helper-finished").exists(),
            "background helper was killed instead of detached"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("executions")).unwrap(),
            "run\n"
        );
        let report: Value =
            serde_json::from_slice(&std::fs::read(root.join("out/results.json")).unwrap()).unwrap();
        assert_eq!(report["status"], if failure { "failed" } else { "passed" });
        assert_eq!(report["coverage"]["partial"], true);
        assert_eq!(report["coverage_evidence"]["scopes"], serde_json::json!([]));
    }
}

#[test]
#[cfg(target_os = "linux")]
fn isolated_go_coverage_runs_without_ptrace_permission() {
    let directory = child_coverage_fixture();
    let root = directory.path();
    std::fs::write(root.join("deny-ptrace.c"), r"
#include <errno.h>
#include <linux/filter.h>
#include <linux/seccomp.h>
#include <stddef.h>
#include <sys/prctl.h>
#include <sys/syscall.h>
#include <unistd.h>
int main(int argc, char **argv) {
    if (argc < 2) return 99;
    struct sock_filter filter[] = {
        BPF_STMT(BPF_LD | BPF_W | BPF_ABS, offsetof(struct seccomp_data, nr)),
        BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, __NR_ptrace, 0, 1),
        BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ERRNO | EPERM),
        BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ALLOW),
    };
    struct sock_fprog program = { .len = sizeof(filter) / sizeof(filter[0]), .filter = filter };
    if (prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) || prctl(PR_SET_SECCOMP, SECCOMP_MODE_FILTER, &program)) return 98;
    execv(argv[1], argv + 1);
    return 97;
}
").unwrap();
    let build = Command::new("cc")
        .args(["deny-ptrace.c", "-o", "deny-ptrace"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(build.status.success(), "{build:?}");
    let normal = child_coverage_command(root);
    let output = Command::new(root.join("deny-ptrace"))
        .arg(normal.get_program())
        .args(normal.get_args())
        .envs(
            normal
                .get_envs()
                .filter_map(|(key, value)| value.map(|value| (key, value))),
        )
        .current_dir(root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        std::fs::read_to_string(root.join("executions")).unwrap(),
        "run\n"
    );
    let report: Value =
        serde_json::from_slice(&std::fs::read(root.join("out/results.json")).unwrap()).unwrap();
    assert_eq!(report["status"], "passed");
    assert_eq!(report["coverage"]["partial"], true);
    assert_eq!(report["coverage_evidence"]["scopes"], serde_json::json!([]));
}
