//! The traced contract probe: runs the action under the access trace,
//! stages each undeclared workspace file it tried to read, and reruns.

use std::collections::BTreeSet;
use std::path::Path;

use struo_cas::CacheProvider;

use super::{
    path_within_declared_input, report, ActionContractDiagnostic, ActionContractReport,
    FilesystemOperation, CONTRACT_STDERR_LIMIT, UNTRACED_READS,
};
use crate::{Action, WorkspacePath};

/// Most reruns a traced probe makes while staging undeclared reads.
const TRACE_ROUNDS: usize = 8;

/// Run the probe under the access trace, staging each undeclared workspace
/// file it tried to read and rerunning, so one validation reports every
/// undeclared read an action makes before it succeeds, without letting the
/// action see the real workspace.
pub(super) async fn probe(
    validated: Action,
    workspace: &Path,
    cache: &CacheProvider,
    helper: std::path::PathBuf,
) -> std::io::Result<ActionContractReport> {
    let Action::RunCommand {
        inputs: declared,
        outputs,
        ..
    } = &validated
    else {
        unreachable!("only command actions are traced");
    };
    let dir = tempfile::Builder::new().prefix("struo-trace-").tempdir()?;
    let request = crate::access_trace::Request {
        helper,
        dir: dir.path().to_path_buf(),
    };
    let mut staged = BTreeSet::<String>::new();
    let mut outside = BTreeSet::new();
    let mut limitations = BTreeSet::new();
    let mut unavailable = BTreeSet::new();
    let mut host_reads = BTreeSet::new();
    let mut round = 0;
    let outcome = loop {
        round += 1;
        let mut probe = validated.clone();
        if let Action::RunCommand { inputs, .. } = &mut probe {
            for path in &staged {
                inputs.push(WorkspacePath::try_from(path.as_str()).map_err(std::io::Error::other)?);
            }
        }
        let Action::RunCommand { inputs, .. } = &probe else {
            unreachable!("the probe is a command action");
        };
        let inputs = inputs.clone();
        // A helper that never starts must not leave the previous round's log.
        let _ = std::fs::remove_file(request.log_path());
        let outcome = crate::access_trace::scope(
            request.clone(),
            crate::run_uncached_contract(&probe, workspace, cache, false),
        )
        .await;
        let findings =
            crate::access_trace::classify(&request.log_path(), workspace, &inputs, outputs)?;
        tracing::debug!(
            round,
            missing = findings.missing.len(),
            outside = findings.outside.len(),
            traced = findings.traced,
            "traced contract probe"
        );
        outside.extend(findings.outside);
        host_reads.extend(findings.host_reads);
        if let Some(reason) = findings.unavailable {
            unavailable.insert(reason);
        }
        if !findings.sockets.is_empty() {
            limitations.insert(format!(
                "The action connected to {}; reads a process outside the traced tree makes on its behalf are not observed",
                findings.sockets.into_iter().collect::<Vec<_>>().join(", ")
            ));
        }
        let new = findings
            .missing
            .into_iter()
            .filter(|path| !staged.contains(path))
            .collect::<Vec<_>>();
        if new.is_empty() {
            break outcome;
        }
        staged.extend(new);
        if round >= TRACE_ROUNDS {
            limitations.insert(format!(
                "Stopped staging undeclared reads after {TRACE_ROUNDS} probe runs; validate again after declaring them"
            ));
            break outcome;
        }
    };
    limitations.insert(
        "Declared inputs the action never read are not reported; a run that does not read an input does not prove it is unused".to_string(),
    );
    if cfg!(target_os = "macos") {
        limitations.insert(
            "macOS observes only reads of undeclared workspace files, so no host reads are reported".to_string(),
        );
    }
    if let Ok(result) = &outcome {
        limitations.extend(nested_sandbox_limitation(result, cache).await?);
    }
    let mut reads = undeclared_reads(declared, &staged, &outside);
    reads.extend(unavailable.iter().map(trace_unavailable));
    report(
        outcome,
        reads,
        limitations.into_iter().collect(),
        host_reads.into_iter().collect(),
    )
}

/// One `undeclared_read` per staged or escaping read that no declared input covers.
fn undeclared_reads(
    declared: &[WorkspacePath],
    staged: &BTreeSet<String>,
    outside: &BTreeSet<String>,
) -> Vec<ActionContractDiagnostic> {
    staged
        .iter()
        .map(|path| (path, "action read an undeclared workspace file inside its sandbox"))
        .chain(outside.iter().map(|path| {
            (
                path,
                "action read an undeclared workspace file through the real workspace, outside its sandbox",
            )
        }))
        .filter(|(path, _)| {
            !declared
                .iter()
                .any(|input| path_within_declared_input(path, [input.as_str()]))
        })
        .map(|(path, message)| ActionContractDiagnostic {
            code: "undeclared_read".to_string(),
            operation: FilesystemOperation::Read,
            path: path.clone(),
            message: format!("{message}: `{path}`"),
            repairs: vec![format!("Add `{path}` to the action inputs")],
        })
        .collect()
}

/// A trace that was asked for but did not run must not pass validation:
/// without it, undeclared reads that succeed go unseen.
fn trace_unavailable(reason: &String) -> ActionContractDiagnostic {
    ActionContractDiagnostic {
        code: "trace_unavailable".to_string(),
        operation: FilesystemOperation::Read,
        path: String::new(),
        message: format!("{UNTRACED_READS}, so undeclared reads may be missing: {reason}"),
        repairs: vec!["Fix what stopped the access trace and validate again".to_string()],
    }
}

/// The limitation for a run that failed because the action tried to apply
/// its own sandbox inside the trace's, read from its captured stderr.
async fn nested_sandbox_limitation(
    result: &struo_cas::ActionResult,
    cache: &CacheProvider,
) -> std::io::Result<Option<String>> {
    let Some(stderr) = result.stderr.filter(|_| result.exit_code != 0) else {
        return Ok(None);
    };
    let (bytes, _) = cache
        .read_blob_limited(&stderr, CONTRACT_STDERR_LIMIT, false)
        .await
        .map_err(std::io::Error::other)?;
    Ok(crate::access_trace::nested_sandbox_failure(
        &String::from_utf8_lossy(&bytes),
    ))
}
