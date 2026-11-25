//! Seatbelt backend for macOS.
//!
//! macOS has no unprivileged way to observe a read that succeeds. Seatbelt
//! does log every denial to the unified log, and `log stream` reads it
//! without privileges. So the probe stages the workspace's undeclared files
//! into the execroot, this backend runs the command under a profile that
//! denies content reads of regular workspace files other than the declared
//! inputs and outputs, and the denials name the undeclared reads.
//!
//! The log delivers late and a fresh stream can stay silent for a while, so
//! a probe read that the helper's own profile denies proves the stream live
//! before the command runs, and a second probe after it is the barrier for
//! the command's denials. Seatbelt coalesces a denial repeated within about
//! a second, which only drops duplicates of a path already reported.
//! Seatbelt profiles do not nest, so a command that applies its own sandbox
//! fails under this backend.

use std::ffi::OsString;
use std::io::BufRead as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::{exit_code, Event, Log, Op, Spec};
use crate::local::isolation::DENY_NETWORK_RULES;

const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";
const LIVENESS_ATTEMPTS: u32 = 3;
const LIVENESS_TIMEOUT: Duration = Duration::from_secs(3);
const BARRIER_TIMEOUT: Duration = Duration::from_secs(10);

pub(super) fn trace(spec: &Spec, command: &[OsString], log: &mut Log) -> std::io::Result<i32> {
    let scratch = spec
        .log
        .parent()
        .map_or_else(std::env::temp_dir, Path::to_path_buf);
    let mut stream = match Stream::start() {
        Ok(stream) => stream,
        Err(error) => {
            log.write(&Event::Unavailable {
                reason: format!("could not read the unified log: {error}"),
            })?;
            return run_unobserved(spec, command);
        }
    };
    let live = (0..LIVENESS_ATTEMPTS)
        .any(|attempt| stream.probe(&scratch, &format!("live-{attempt}"), LIVENESS_TIMEOUT));
    if !live {
        stream.stop();
        log.write(&Event::Unavailable {
            reason: "the unified log did not report Seatbelt denials".to_string(),
        })?;
        return run_unobserved(spec, command);
    }
    log.write(&Event::Started {
        backend: "seatbelt".to_string(),
        execroot: spec.execroot.clone(),
    })?;
    let status = Command::new(SANDBOX_EXEC)
        .arg("-p")
        .arg(profile(spec))
        .arg("--")
        .args(command)
        .status()?;
    if !stream.probe(&scratch, "barrier", BARRIER_TIMEOUT) {
        log.write(&Event::Unavailable {
            reason: "the unified log fell behind; some denials may be missing".to_string(),
        })?;
    }
    let lines = stream.stop();
    let workspace = super::roots(&spec.workspace);
    for line in lines {
        if let Some(path) = denied_read(&line) {
            if workspace.iter().any(|root| path.starts_with(root)) {
                log.write(&Event::Access {
                    op: Op::Read,
                    path,
                    ok: false,
                })?;
            }
        }
    }
    Ok(exit_code(status))
}

/// Run without observing reads, keeping the network denial the action declared.
fn run_unobserved(spec: &Spec, command: &[OsString]) -> std::io::Result<i32> {
    if !spec.deny_network {
        return super::run_untraced(command);
    }
    let status = Command::new(SANDBOX_EXEC)
        .arg("-p")
        .arg(format!("(version 1) (allow default){DENY_NETWORK_RULES}"))
        .arg("--")
        .args(command)
        .status()?;
    Ok(exit_code(status))
}

/// The command's profile. Later rules win, so the workspace-wide denial comes
/// first and each exception after it.
pub(super) fn profile(spec: &Spec) -> String {
    let workspace = canonical(&spec.workspace);
    let execroot = canonical(&spec.execroot);
    let mut profile = String::from("(version 1)\n(allow default)\n");
    let deny = |root: &Path| {
        format!(
            "(deny file-read-data (require-all (subpath {}) (vnode-type REGULAR-FILE)))\n",
            quote(root)
        )
    };
    let allow = |path: &Path| format!("(allow file-read-data (subpath {}))\n", quote(path));
    profile.push_str(&deny(&workspace));
    profile.push_str(&allow(&workspace.join(".struo")));
    profile.push_str(&deny(&execroot));
    for readable in &spec.readable {
        profile.push_str(&allow(&execroot.join(readable.trim_end_matches('/'))));
    }
    if spec.deny_network {
        profile.push_str(DENY_NETWORK_RULES);
    }
    profile
}

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn quote(path: &Path) -> String {
    let text = path.to_string_lossy();
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The path of a `file-read*` denial in a unified log line, such as
/// `Sandbox: cat(4321) deny(1) file-read-data /private/tmp/secret`.
pub(super) fn denied_read(line: &str) -> Option<PathBuf> {
    let message = serde_json::from_str::<serde_json::Value>(line)
        .ok()
        .and_then(|value| value.get("eventMessage")?.as_str().map(str::to_string))
        .unwrap_or_else(|| line.to_string());
    let (_, rest) = message.split_once(") deny(")?;
    let (_, rest) = rest.split_once(") ")?;
    let (operation, path) = rest.split_once(' ')?;
    (operation.starts_with("file-read") && path.starts_with('/')).then(|| PathBuf::from(path))
}

struct Stream {
    child: std::process::Child,
    lines: mpsc::Receiver<String>,
    collected: Vec<String>,
}

impl Stream {
    fn start() -> std::io::Result<Self> {
        let mut child = Command::new("/usr/bin/log")
            .args([
                "stream",
                "--style",
                "ndjson",
                "--predicate",
                "sender == \"Sandbox\"",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let stdout = child.stdout.take().expect("piped stdout");
        let (sender, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            lines,
            collected: Vec::new(),
        })
    }

    /// Read a probe file under a profile that denies it, and wait for the
    /// denial. Every line received meanwhile is kept for the caller.
    fn probe(&mut self, scratch: &Path, name: &str, timeout: Duration) -> bool {
        let file = scratch.join(format!("probe-{name}"));
        if std::fs::write(&file, b"probe").is_err() {
            return false;
        }
        let file = canonical(&file);
        let denied = Command::new(SANDBOX_EXEC)
            .arg("-p")
            .arg(format!(
                "(version 1) (allow default) (deny file-read-data (literal {}))",
                quote(&file)
            ))
            .arg("/bin/cat")
            .arg(&file)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if denied.is_err() {
            return false;
        }
        let deadline = Instant::now() + timeout;
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match self.lines.recv_timeout(left) {
                // ndjson escapes `/` as `\/`, so match the decoded path.
                Ok(line) if denied_read(&line).is_some_and(|path| path == file) => return true,
                Ok(line) => self.collected.push(line),
                Err(_) => return false,
            }
        }
        false
    }

    fn stop(mut self) -> Vec<String> {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.collected.extend(self.lines.try_iter());
        self.collected
    }
}
