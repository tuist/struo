//! File access tracing for contract validation probes.
//!
//! A traced probe runs its command under a helper process, `struo __trace`,
//! which records the file accesses of the whole process tree as JSON lines.
//! The helper is a separate process so the tracing mechanism never shares
//! the caller's runtime: ptrace has to reap every descendant with
//! `waitpid(-1)`, which would steal the exit statuses of other actions.
//!
//! - Linux traces file syscalls with ptrace and reports every path the tree
//!   opened, probed, listed, or executed, with whether the call succeeded.
//! - macOS cannot observe a read that succeeds without privileges, so the
//!   probe stages the workspace's undeclared files into the execroot as well,
//!   a Seatbelt profile denies content reads of them, and the helper collects
//!   the denials Seatbelt writes to the unified log.
//!
//! [`classify`] turns either trace into workspace-relative findings.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::future::Future;
use std::io::{BufRead as _, Write as _};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::WorkspacePath;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod linux;
#[cfg(target_os = "macos")]
mod macos;

#[cfg(test)]
mod tests;

/// The hidden CLI verb that runs the helper.
pub const HELPER_VERB: &str = "__trace";

/// Most host paths a report keeps.
const HOST_READ_LIMIT: usize = 500;

tokio::task_local! {
    static REQUEST: Arc<Request>;
}

/// What a traced probe needs: the helper executable and a private directory
/// for the spec, the event log, and any probe files.
#[derive(Debug, Clone)]
pub(crate) struct Request {
    pub helper: PathBuf,
    pub dir: PathBuf,
}

impl Request {
    pub(crate) fn spec_path(&self) -> PathBuf {
        self.dir.join("spec.json")
    }

    pub(crate) fn log_path(&self) -> PathBuf {
        self.dir.join("events.jsonl")
    }
}

pub(crate) async fn scope<F: Future>(request: Request, future: F) -> F::Output {
    REQUEST.scope(Arc::new(request), future).await
}

pub(crate) fn current() -> Option<Arc<Request>> {
    REQUEST.try_with(Arc::clone).ok()
}

/// Whether this platform has a tracing backend.
pub(crate) const fn supported() -> bool {
    cfg!(any(
        all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64")
        ),
        target_os = "macos"
    ))
}

/// Whether the probe must stage undeclared workspace files into the execroot
/// so the backend can observe reads of them.
pub(crate) const fn stages_undeclared_files() -> bool {
    cfg!(target_os = "macos")
}

/// The limitation for an action that failed applying its own Seatbelt
/// sandbox: macOS refuses a nested profile with this message, and the
/// trace's backend runs every action under one.
pub(crate) fn nested_sandbox_failure(stderr: &str) -> Option<String> {
    (cfg!(target_os = "macos") && stderr.contains("sandbox_apply: Operation not permitted")).then(|| {
        "The action failed applying its own Seatbelt sandbox, which cannot nest inside the access trace's sandbox; disable the tool's own sandbox for this action, if it has an option for that, and validate again".to_string()
    })
}

/// How a sandbox wraps a traced command: the helper executable and the spec
/// it reads.
#[derive(Debug, Clone)]
pub(crate) struct Wrapper {
    pub program: PathBuf,
    pub spec: PathBuf,
}

/// What the helper needs beyond the command: written by the sandbox, which
/// knows the execroot, and read by the helper.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Spec {
    pub log: PathBuf,
    pub execroot: PathBuf,
    pub workspace: PathBuf,
    /// Execroot-relative paths the action may read: declared inputs and outputs.
    pub readable: Vec<String>,
    /// Whether the backend must also deny the network, because it replaces
    /// the platform's network isolation instead of nesting inside it.
    pub deny_network: bool,
}

/// One line of the event log.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Event {
    /// The trace started; paths below are relative to this execroot.
    Started { backend: String, execroot: PathBuf },
    /// The backend could not trace; the command ran untraced.
    Unavailable { reason: String },
    /// A file access. `ok` is whether the call succeeded.
    Access { op: Op, path: PathBuf, ok: bool },
    /// The tree connected to a socket, so another process may read on its behalf.
    Socket { address: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Op {
    Read,
    Probe,
    List,
    Exec,
}

/// Run the helper: `struo __trace <spec> -- <program> [args...]`. Returns the
/// traced command's exit code.
pub fn run_helper(args: &[OsString]) -> u8 {
    match helper(args) {
        Ok(code) => u8::try_from(code.clamp(0, 255)).unwrap_or(255),
        Err(error) => {
            eprintln!("struo {HELPER_VERB}: {error}");
            127
        }
    }
}

fn helper(args: &[OsString]) -> std::io::Result<i32> {
    let (spec, command) = match args {
        [spec, separator, command @ ..] if separator == "--" && !command.is_empty() => {
            (spec, command)
        }
        _ => {
            return Err(std::io::Error::other(format!(
                "usage: struo {HELPER_VERB} <spec> -- <program> [args...]"
            )))
        }
    };
    let spec: Spec = serde_json::from_slice(&std::fs::read(spec)?)?;
    let mut log = Log::create(&spec.log)?;
    let code = backend(&spec, command, &mut log)?;
    log.flush()?;
    Ok(code)
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
fn backend(spec: &Spec, command: &[OsString], log: &mut Log) -> std::io::Result<i32> {
    linux::trace(spec, command, log)
}

#[cfg(target_os = "macos")]
fn backend(spec: &Spec, command: &[OsString], log: &mut Log) -> std::io::Result<i32> {
    macos::trace(spec, command, log)
}

#[cfg(not(any(
    all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ),
    target_os = "macos"
)))]
fn backend(_spec: &Spec, command: &[OsString], log: &mut Log) -> std::io::Result<i32> {
    log.write(&Event::Unavailable {
        reason: "no file access tracer for this platform".to_string(),
    })?;
    run_untraced(command)
}

#[allow(dead_code)]
fn run_untraced(command: &[OsString]) -> std::io::Result<i32> {
    let status = std::process::Command::new(&command[0])
        .args(&command[1..])
        .status()?;
    Ok(exit_code(status))
}

#[allow(dead_code)]
fn exit_code(status: std::process::ExitStatus) -> i32 {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt as _;
        if let Some(signal) = status.signal() {
            return 128 + signal;
        }
    }
    status.code().unwrap_or(1)
}

/// The event log. Events repeat heavily (every compiler probes the same
/// headers), so each distinct event is written once.
pub(crate) struct Log {
    writer: std::io::BufWriter<std::fs::File>,
    seen: std::collections::HashSet<Event>,
}

impl Log {
    fn create(path: &Path) -> std::io::Result<Self> {
        Ok(Self {
            writer: std::io::BufWriter::new(std::fs::File::create(path)?),
            seen: std::collections::HashSet::new(),
        })
    }

    pub(crate) fn write(&mut self, event: &Event) -> std::io::Result<()> {
        if self.seen.insert(event.clone()) {
            serde_json::to_writer(&mut self.writer, event)?;
            self.writer.write_all(b"\n")?;
        }
        Ok(())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.writer.flush()
    }
}

/// What a trace says about an action's declared contract.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Findings {
    pub traced: bool,
    pub unavailable: Option<String>,
    /// Undeclared workspace files the action tried to read inside its
    /// sandbox. Staging them lets a rerun get further.
    pub missing: BTreeSet<String>,
    /// Undeclared workspace files the action read through the real workspace,
    /// outside its sandbox, such as by walking up from the execroot.
    pub outside: BTreeSet<String>,
    /// Files read outside the workspace, such as toolchains and libraries.
    pub host_reads: BTreeSet<String>,
    pub sockets: BTreeSet<String>,
}

/// Classify a helper's event log against the action's contract. `workspace`
/// is the real workspace, whose files decide what counts as a workspace read.
pub(crate) fn classify(
    log: &Path,
    workspace: &Path,
    inputs: &[WorkspacePath],
    outputs: &[WorkspacePath],
) -> std::io::Result<Findings> {
    let mut findings = Findings::default();
    let file = match std::fs::File::open(log) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            findings.unavailable = Some("the trace helper wrote no event log".to_string());
            return Ok(findings);
        }
        Err(error) => return Err(error),
    };
    let workspace_roots = roots(workspace);
    let mut execroots = Vec::new();
    for line in std::io::BufReader::new(file).lines() {
        let line = line?;
        let Ok(event) = serde_json::from_str::<Event>(&line) else {
            continue;
        };
        match event {
            Event::Started { execroot, .. } => {
                findings.traced = true;
                execroots = roots(&execroot);
            }
            Event::Unavailable { reason } => findings.unavailable = Some(reason),
            Event::Socket { address } => {
                findings.sockets.insert(address);
            }
            Event::Access { op, path, ok } => {
                if let Some(relative) = relative_to(&path, &execroots) {
                    if !ok && undeclared_file(&relative, workspace, inputs, outputs) {
                        findings.missing.insert(relative);
                    }
                } else if let Some(relative) = relative_to(&path, &workspace_roots) {
                    if relative != ".struo"
                        && !relative.starts_with(".struo/")
                        && undeclared_file(&relative, workspace, inputs, outputs)
                    {
                        findings.outside.insert(relative);
                    }
                } else if ok
                    && matches!(op, Op::Read | Op::Exec)
                    && findings.host_reads.len() < HOST_READ_LIMIT
                    && !["/proc/", "/dev/", "/sys/"]
                        .iter()
                        .any(|prefix| path.starts_with(prefix))
                {
                    findings.host_reads.insert(path.display().to_string());
                }
            }
        }
    }
    if !findings.traced && findings.unavailable.is_none() {
        findings.unavailable = Some("the trace helper did not start".to_string());
    }
    Ok(findings)
}

/// A path and its canonical form, so traces that report resolved paths
/// (`/private/var` on macOS) still match.
fn roots(path: &Path) -> Vec<PathBuf> {
    let mut roots = vec![path.to_path_buf()];
    if let Ok(canonical) = std::fs::canonicalize(path) {
        if canonical != path {
            roots.push(canonical);
        }
    }
    roots
}

fn relative_to(path: &Path, roots: &[PathBuf]) -> Option<String> {
    let path = normalize(path);
    roots.iter().find_map(|root| {
        let relative = path.strip_prefix(root).ok()?;
        let text = relative.to_string_lossy().replace('\\', "/");
        (!text.is_empty()).then_some(text)
    })
}

/// Collapse `.` and `..` lexically. Tracers report paths as the action
/// spelled them, joined onto the directory they were relative to.
pub(crate) fn normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other),
        }
    }
    normalized
}

/// Whether `relative` is a regular file in the workspace that no declared
/// input or output covers. Paths that do not exist in the workspace are not
/// undeclared reads: there is nothing to declare.
fn undeclared_file(
    relative: &str,
    workspace: &Path,
    inputs: &[WorkspacePath],
    outputs: &[WorkspacePath],
) -> bool {
    let covered = |paths: &[WorkspacePath]| {
        paths.iter().any(|declared| {
            let declared = declared.as_str().trim_end_matches('/');
            relative == declared || relative.starts_with(&format!("{declared}/"))
        })
    };
    !covered(inputs)
        && !covered(outputs)
        && std::fs::metadata(workspace.join(relative)).is_ok_and(|metadata| metadata.is_file())
}
