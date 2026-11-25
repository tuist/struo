use std::collections::BTreeMap;
use std::path::Path;

use struo_cas::Cas;
use tokio::io::AsyncReadExt;

use super::super::client::api_error;
use crate::remote::{archive::input::plan::InputPlan, phase_permit};
use crate::{ExecutionPermit, RemoteIoLimits, Result, WorkspacePath};

pub(super) async fn collect(
    workspace: &Path,
    inputs: &[WorkspacePath],
    cache: &Cas,
    lease: Option<&ExecutionPermit>,
    limits: RemoteIoLimits,
) -> Result<(Vec<crate::worker::Input>, BTreeMap<String, u64>)> {
    let workspace = workspace.to_path_buf();
    let inputs = inputs.to_vec();
    let guard = lease.cloned();
    let plan = tokio::task::spawn_blocking(move || {
        let _guard = guard;
        InputPlan::collect(&workspace, &inputs, "sprites", limits)
    })
    .await
    .map_err(|_| api_error("worker input planning failed"))??;
    let mut entries = Vec::new();
    let mut blobs = BTreeMap::new();
    for relative in plan.entries.keys() {
        if relative.as_os_str().is_empty() {
            continue;
        }
        let fail = |source| crate::Error::FileAction {
            action: "plan_worker_input",
            path: relative.display().to_string(),
            source,
        };
        let metadata = plan.workspace.metadata(relative).map_err(fail)?;
        let kind = if metadata.is_file() {
            let _phase = phase_permit(lease, metadata.len).await?;
            let file = plan.workspace.open_file(relative).map_err(fail)?;
            #[cfg(unix)]
            let executable = {
                use std::os::unix::fs::PermissionsExt;
                file.metadata().map_err(fail)?.permissions().mode() & 0o111 != 0
            };
            #[cfg(not(unix))]
            let executable = false;
            let mut reader = tokio::fs::File::from_std(file);
            let digest = cache.put_stream((&mut reader).take(metadata.len)).await?;
            if reader.metadata().await.map_err(fail)?.len() != metadata.len {
                return Err(api_error("worker input changed during snapshot"));
            }
            blobs.insert(digest.to_hex(), metadata.len);
            crate::worker::InputKind::File { digest, executable }
        } else if metadata.is_dir() {
            crate::worker::InputKind::Directory
        } else if metadata.is_symlink() {
            let target = plan.workspace.read_link(relative).map_err(fail)?;
            if !crate::remote::path::input_link_stays_within(
                relative,
                &target,
                &plan.declared_roots,
            ) {
                return Err(api_error("worker input link escapes declared inputs"));
            }
            crate::worker::InputKind::Symlink {
                target: target
                    .to_str()
                    .ok_or_else(|| api_error("worker input links must be UTF-8"))?
                    .into(),
            }
        } else {
            return Err(api_error(
                "worker inputs must be files, directories, or symlinks",
            ));
        };
        entries.push(crate::worker::Input {
            path: WorkspacePath::try_from(
                relative
                    .to_str()
                    .ok_or_else(|| api_error("worker input paths must be UTF-8"))?,
            )
            .map_err(|_| api_error("invalid worker input path"))?,
            kind,
        });
    }
    Ok((entries, blobs))
}
