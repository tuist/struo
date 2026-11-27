use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::host_error;
use crate::remote::path::{Directory, Parent};
use crate::{RemoteIoLimits, Result, WorkspacePath};

const FILES: usize = 0;
const BACKUP: usize = 1;
const WORKSPACE: usize = 2;

/// Resolves output parents through the descriptor-relative tree walkers while keeping only the
/// most recent parent of each root open, so descriptor use stays constant however many
/// directories the outputs span.
struct Parents<'a> {
    roots: [&'a Directory; 3],
    open: [Option<(PathBuf, Directory)>; 3],
    seen: BTreeSet<(usize, PathBuf)>,
    metadata_bytes: u64,
    limits: RemoteIoLimits,
}

impl Parents<'_> {
    fn parent(&mut self, root: usize, path: &Path, create: bool) -> Result<Parent> {
        let directory = path.parent().unwrap_or(Path::new(""));
        if self.seen.insert((root, directory.to_path_buf())) {
            self.metadata_bytes = self
                .metadata_bytes
                .saturating_add(directory.as_os_str().len() as u64)
                .saturating_add(256);
            crate::remote::http::check_size(
                self.metadata_bytes,
                self.limits.max_metadata_bytes,
                "remote",
                "output parent metadata bytes",
            )?;
        }
        if !matches!(&self.open[root], Some((open, _)) if open == directory) {
            self.open[root] = None;
            let opened = self.roots[root]
                .descend(directory, create)
                .map_err(|source| host_error("open_remote_output_parent", path, source))?;
            self.open[root] = Some((directory.to_path_buf(), opened));
        }
        let (_, opened) = self.open[root].as_ref().unwrap();
        opened
            .parent(Path::new(path.file_name().unwrap()), false)
            .map_err(|source| host_error("open_remote_output_parent", path, source))
    }
}

pub(super) fn install_outputs(
    files: &Directory,
    backup: &Directory,
    workspace: &Directory,
    outputs: &[WorkspacePath],
    limits: RemoteIoLimits,
    directory_modes: &BTreeMap<PathBuf, u32>,
    captured: bool,
) -> Result<()> {
    let mode_bytes = directory_modes.keys().fold(0_u64, |bytes, path| {
        bytes
            .saturating_add(path.as_os_str().len() as u64)
            .saturating_add(64)
    });
    crate::remote::path::validate_output_tree(
        files,
        outputs,
        limits,
        mode_bytes,
        captured.then_some(workspace),
    )?;
    for (path, mode) in directory_modes.iter().rev() {
        files
            .set_directory_mode(path, *mode)
            .map_err(|source| host_error("set_remote_output_permissions", path, source))?;
    }
    let mut parents = Parents {
        roots: [files, backup, workspace],
        open: [None, None, None],
        seen: BTreeSet::new(),
        metadata_bytes: 0,
        limits,
    };
    let mut backed_up = Vec::new();
    let mut installed = Vec::new();
    for output in outputs {
        if let Err(error) = install_output(&mut parents, output, &mut backed_up, &mut installed) {
            rollback_outputs(&mut parents, &installed, &backed_up);
            return Err(error);
        }
    }
    Ok(())
}

fn install_output<'a>(
    parents: &mut Parents<'_>,
    output: &'a WorkspacePath,
    backed_up: &mut Vec<&'a Path>,
    installed: &mut Vec<&'a Path>,
) -> Result<()> {
    let path = Path::new(output.as_str());
    let destination = parents.parent(WORKSPACE, path, true)?;
    let saved = parents.parent(BACKUP, path, true)?;
    let source = parents.parent(FILES, path, false)?;
    match destination.metadata() {
        Ok(_) => {
            destination
                .rename_to(&saved)
                .map_err(|source| host_error("backup_remote_output", path, source))?;
            backed_up.push(path);
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => return Err(host_error("backup_remote_output", path, source)),
    }
    source
        .rename_to(&destination)
        .map_err(|source| host_error("install_remote_output", path, source))?;
    installed.push(path);
    Ok(())
}

/// Best effort: moves installed outputs back into staging and restores the saved originals,
/// resolving each parent again because only the most recent ones stay open.
fn rollback_outputs(parents: &mut Parents<'_>, installed: &[&Path], backed_up: &[&Path]) {
    let mut restore = |from: usize, to: usize, path: &Path| -> Result<()> {
        let source = parents.parent(from, path, false)?;
        let destination = parents.parent(to, path, false)?;
        let _ = source.rename_to(&destination);
        Ok(())
    };
    for path in installed.iter().rev() {
        let _ = restore(WORKSPACE, FILES, path);
    }
    for path in backed_up.iter().rev() {
        let _ = restore(BACKUP, WORKSPACE, path);
    }
}
