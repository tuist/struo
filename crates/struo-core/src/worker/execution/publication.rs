use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use struo_cas::ActionResult;

use super::super::cas::Cas;
use super::super::Response;
use super::{files, process};
use crate::WorkspacePath;

pub(super) fn capture(
    root: &Path,
    workspace: &Path,
    cas: &mut Cas,
    output: &process::Output,
    outputs: &[WorkspacePath],
    success: &[i32],
) -> io::Result<Response> {
    let mut result = ActionResult {
        exit_code: output.exit_code,
        stdout: None,
        stderr: None,
        outputs: BTreeMap::new(),
    };
    for (source, captured) in [
        (&output.stdout, &mut result.stdout),
        (&output.stderr, &mut result.stderr),
    ] {
        if source.metadata()?.len() != 0 {
            *captured = Some(cas.publish(source, None)?);
        }
    }
    let mut response = Response {
        result,
        output_archive: None,
        output_archive_bytes: 0,
        shared_blobs: Vec::new(),
    };
    if success.contains(&output.exit_code) && !outputs.is_empty() {
        let archive = root.join("outputs.tar");
        files::archive(workspace, outputs, &archive, cas.available())?;
        response.output_archive_bytes = archive.metadata()?.len();
        response.output_archive = Some(cas.publish(&archive, None)?);
    }
    Ok(response)
}
