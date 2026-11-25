use std::collections::BTreeSet;
use std::path::Path;

use super::{classify, Event, Op};
use crate::WorkspacePath;

fn access(op: Op, path: &Path, ok: bool) -> Event {
    Event::Access {
        op,
        path: path.to_path_buf(),
        ok,
    }
}

fn set(paths: &[&str]) -> BTreeSet<String> {
    paths.iter().map(ToString::to_string).collect()
}

#[test]
fn classify_reports_only_undeclared_workspace_files() {
    let workspace = tempfile::tempdir().unwrap();
    let root = workspace.path();
    for file in [
        "main.c",
        "src/lib.h",
        "include/extra.h",
        "notes.txt",
        ".struo/state",
    ] {
        let path = root.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"x").unwrap();
    }
    let execroot = root.join(".struo/sandboxes/digest-1/execroot");
    std::fs::create_dir_all(&execroot).unwrap();
    let events = [
        Event::Started {
            backend: "test".to_string(),
            execroot: execroot.clone(),
        },
        // A failed read of an existing undeclared file inside the sandbox.
        access(Op::Read, &execroot.join("include/extra.h"), false),
        // A child of a declared directory input is covered.
        access(Op::Read, &execroot.join("src/lib.h"), false),
        // A path that does not exist in the workspace cannot be declared.
        access(Op::Probe, &execroot.join("config.h"), false),
        // A failed probe of an existing directory is not a file read.
        access(Op::Probe, &execroot.join("include"), false),
        // A successful read inside the sandbox read something staged there.
        access(Op::Read, &execroot.join("main.c"), true),
        // Walking up from the execroot reaches the real workspace.
        access(Op::Read, &execroot.join("../../../../notes.txt"), true),
        // Struo's own state is never an action input.
        access(Op::Read, &root.join(".struo/state"), true),
        access(Op::Exec, Path::new("/usr/bin/cc"), true),
        access(Op::Read, Path::new("/usr/include/missing.h"), false),
        access(Op::Read, Path::new("/proc/self/status"), true),
        Event::Socket {
            address: "unix:/tmp/daemon.sock".to_string(),
        },
    ];
    let log = root.join("events.jsonl");
    let lines = events
        .iter()
        .map(|event| serde_json::to_string(event).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&log, lines + "\nnot json\n").unwrap();
    let inputs = [
        WorkspacePath::try_from("main.c").unwrap(),
        WorkspacePath::try_from("src").unwrap(),
    ];

    let findings = classify(&log, root, &inputs, &[]).unwrap();

    assert!(findings.traced);
    assert_eq!(findings.unavailable, None);
    assert_eq!(findings.missing, set(&["include/extra.h"]));
    assert_eq!(findings.outside, set(&["notes.txt"]));
    assert_eq!(findings.host_reads, set(&["/usr/bin/cc"]));
    assert_eq!(findings.sockets, set(&["unix:/tmp/daemon.sock"]));
}

#[test]
fn classify_reports_a_helper_that_never_started() {
    let workspace = tempfile::tempdir().unwrap();
    let findings = classify(
        &workspace.path().join("absent.jsonl"),
        workspace.path(),
        &[],
        &[],
    )
    .unwrap();
    assert!(!findings.traced);
    assert!(findings.unavailable.is_some());
}

/// Only Seatbelt refusing a nested profile is blamed on the trace; another
/// failure, even one that mentions a sandbox, is the action's own.
#[cfg(target_os = "macos")]
#[test]
fn nested_sandbox_failure_matches_only_a_refused_nested_profile() {
    let refused = "error: 'tiny': Invalid manifest (compiled with: [\"swiftc\"])\nsandbox-exec: sandbox_apply: Operation not permitted\n";
    assert!(super::nested_sandbox_failure(refused).is_some());
    let other = "sandbox-exec: sandbox_apply: Invalid argument\nerror: no such module 'Foo'\n";
    assert!(super::nested_sandbox_failure(other).is_none());
}
