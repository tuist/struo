mod files;
mod process;
mod publication;
mod tools;

use std::collections::BTreeMap;
use std::io;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use tokio_util::sync::CancellationToken;

use super::cas::{invalid, Cas};
use super::{Request, Response, ServeOptions, EXECUTION_ROOT, PROTOCOL_VERSION};
use crate::{ResourcePool, ResourceRequest};

pub(super) struct Executor {
    pub options: ServeOptions,
    pub resources: Arc<ResourcePool>,
    verified: BTreeMap<String, String>,
}

impl Executor {
    pub fn new(options: ServeOptions, resources: Arc<ResourcePool>) -> Self {
        Self {
            options,
            resources,
            verified: BTreeMap::new(),
        }
    }

    pub async fn probe(&self, root: &Path) -> io::Result<()> {
        let temporary = tempfile::Builder::new()
            .prefix("action-probe-")
            .tempdir_in(root)?;
        let workspace = temporary.path().join("workspace");
        std::fs::create_dir(&workspace)?;
        let result = process::run(
            &self.options,
            &workspace,
            &["/usr/bin/true".into()],
            &self.options.tool_environment,
            EXECUTION_ROOT,
            temporary.path(),
            Some(5000),
            32 * 1024,
            &CancellationToken::new(),
            crate::local::Redirect::default(),
        )
        .await;
        let result = result.and_then(|output| {
            if output.exit_code == 0 {
                Ok(())
            } else {
                let detail = std::fs::read_to_string(output.stderr)?;
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    format!("bubblewrap isolation probe failed: {}", detail.trim()),
                ))
            }
        });
        temporary
            .close()
            .map_err(|_| io::Error::other("worker probe cleanup failed"))?;
        result
    }

    pub async fn execute(
        &mut self,
        root: &Path,
        cas: &mut Cas,
        mut request: Request,
        cancel: CancellationToken,
    ) -> io::Result<Response> {
        if request.version != PROTOCOL_VERSION || request.environment != self.options.environment {
            return Err(invalid("worker protocol or prepared environment mismatch"));
        }
        if request.argv.is_empty() || request.argv[0].is_empty() {
            return Err(invalid("empty action argv"));
        }
        let memory = if request.memory_bytes == 0 {
            self.resources.limits().default_action_memory_bytes
        } else {
            request.memory_bytes
        };
        let transfer_memory = if request
            .shared_cache
            .as_ref()
            .is_some_and(|cache| !cache.read_only)
        {
            super::shared::TRANSFER_MEMORY
        } else {
            0
        };
        let admitted_memory =
            if memory.saturating_add(transfer_memory) <= self.resources.limits().memory_bytes {
                memory.saturating_add(transfer_memory)
            } else {
                request.shared_cache = None;
                memory
            };
        let _permit = self
            .resources
            .acquire_required(ResourceRequest::new(1, admitted_memory))
            .await
            .map_err(|_| invalid("action exceeds worker memory budget"))?;
        let pinned = request
            .inputs
            .iter()
            .chain(&request.post_setup_inputs)
            .filter_map(|input| {
                if let super::InputKind::File { digest, .. } = input.kind {
                    Some(digest)
                } else {
                    None
                }
            })
            .collect();
        cas.make_room_pinned(cas.capacity / 4, &pinned)?;
        let temporary = tempfile::Builder::new()
            .prefix("action-")
            .tempdir_in(root)?;
        let result = self
            .execute_inner(temporary.path(), cas, request, cancel)
            .await;
        // A failed close is fatal: no result is returned and no subsequent action
        // may assume that the disposable workspace was successfully removed.
        let started = Instant::now();
        temporary
            .close()
            .map_err(|_| io::Error::other("worker workspace cleanup failed"))?;
        timing("cleanup", started);
        result
    }

    async fn execute_inner(
        &mut self,
        root: &Path,
        cas: &mut Cas,
        request: Request,
        cancel: CancellationToken,
    ) -> io::Result<Response> {
        let started = Instant::now();
        let workspace = root.join("workspace");
        files::prepare(&workspace, cas, &request)?;
        timing("materialization", started);
        let started = Instant::now();
        let mut env = self.options.tool_environment.clone();
        env.extend(request.env);
        let mut argv = request.argv;
        for value in argv.iter_mut().chain(env.values_mut()) {
            *value = self.resolve_value(value, root, &workspace, &cancel).await?;
        }
        let mut launched = self.options.tool_launcher.clone();
        launched.extend(argv);
        timing("tool_resolution", started);
        let cwd = request.cwd.map_or_else(
            || EXECUTION_ROOT.into(),
            |path| {
                path.resolve(Path::new(EXECUTION_ROOT))
                    .to_string_lossy()
                    .into_owned()
            },
        );
        let started = Instant::now();
        let output = process::run(
            &self.options,
            &workspace,
            &launched,
            &env,
            &cwd,
            root,
            request.timeout_ms,
            cas.available(),
            &cancel,
            crate::local::Redirect {
                stdout: request.stdout.as_ref(),
                stderr: request.stderr.as_ref(),
            },
        )
        .await?;
        timing("sandbox_command", started);
        let started = Instant::now();
        let mut response = publication::capture(
            root,
            &workspace,
            cas,
            &output,
            &request.outputs,
            &request.success_exit_codes,
        )?;
        if let Some(shared) = request.shared_cache {
            let outputs = if request
                .success_exit_codes
                .contains(&response.result.exit_code)
            {
                request.outputs.as_slice()
            } else {
                &[]
            };
            if let Err(error) = super::shared::publish(
                &shared,
                &workspace,
                cas,
                &request.inputs,
                outputs,
                &mut response,
                &cancel,
            )
            .await
            {
                if cancel.is_cancelled() {
                    return Err(error);
                }
                tracing::debug!(
                    "shared output publication unavailable; retaining worker blobs for fallback"
                );
            }
        }
        timing("output_publication", started);
        Ok(response)
    }
}

fn timing(phase: &'static str, started: Instant) {
    tracing::debug!(
        phase,
        elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        "worker action phase completed"
    );
}
