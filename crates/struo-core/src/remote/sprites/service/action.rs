use std::path::Path;

use struo_cas::{CacheProvider, Digest};
use tokio_util::io::ReaderStream;

use super::super::client::api_error;
use super::{inputs, Service};
use crate::remote::{http, phase_permit, PreparedCommand};
use crate::Result;

impl Service {
    #[expect(clippy::too_many_lines)]
    pub(in crate::remote::sprites) async fn execute(
        &self,
        environment: &str,
        command: &PreparedCommand<'_>,
        workspace: &Path,
        cache: &CacheProvider,
        mirror: bool,
        setup: crate::remote::CommandFilesystemSetup<'_>,
    ) -> Result<struo_cas::ActionResult> {
        let started = std::time::Instant::now();
        let local = struo_cas::Cas::open(cache.root());
        let (inputs, mut blobs) = inputs::collect(
            workspace,
            command.inputs,
            &local,
            command.lease.as_ref(),
            self.limits,
        )
        .await?;
        let (post_setup_inputs, post_blobs) = inputs::collect(
            workspace,
            setup.post_setup_inputs,
            &local,
            command.lease.as_ref(),
            self.limits,
        )
        .await?;
        blobs.extend(post_blobs);
        tracing::debug!(
            provider = "sprites",
            phase = "worker_input_snapshot",
            elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            blobs = blobs.len(),
            "worker transfer phase completed"
        );
        let started = std::time::Instant::now();
        let missing_request = http::encode(
            &blobs.keys().collect::<Vec<_>>(),
            "sprites",
            self.limits.max_metadata_bytes,
        )?;
        let response = self
            .send(
                self.request(reqwest::Method::POST, "/v1/cas/missing")?
                    .header("content-type", "application/json")
                    .body(missing_request)
                    .timeout(self.limits.stall_timeout),
            )
            .await?;
        let mut missing: Vec<Digest> = http::decode(
            response,
            "sprites",
            self.limits.max_metadata_bytes,
            self.limits,
        )
        .await?;
        let absent = missing.len();
        let shared_cache = super::shared::connection(cache).await;
        if let Some(shared) = &shared_cache {
            if !missing.is_empty() {
                let requested = inputs
                    .iter()
                    .chain(&post_setup_inputs)
                    .filter_map(|input| match input.kind {
                        crate::worker::InputKind::File { digest, .. } => Some(digest),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                match self.pull(shared, &requested).await {
                    Ok(remaining) if remaining.iter().all(|digest| missing.contains(digest)) => {
                        missing = remaining;
                    }
                    _ => {
                        tracing::debug!(
                            "shared worker fetch unavailable; using coordinator inputs"
                        );
                    }
                }
            }
        }
        let missing_count = missing.len();
        let mut uploaded_bytes = 0_u64;
        for digest in missing {
            let size = *blobs
                .get(&digest.to_hex())
                .ok_or_else(|| api_error("worker requested an undeclared blob"))?;
            let _staging = phase_permit(command.lease.as_ref(), size).await?;
            let file = crate::remote::archive::output_archive_file(workspace)?;
            local.copy_blob_to_file(&digest, file.path()).await?;
            let reader = tokio::fs::File::open(file.path()).await.map_err(|source| {
                crate::Error::FileAction {
                    action: "open_worker_blob",
                    path: "worker input".into(),
                    source,
                }
            })?;
            let progress = http::Progress::new();
            let response = progress
                .watch_upload(
                    "sprites",
                    self.limits,
                    self.send(
                        self.request(
                            reqwest::Method::PUT,
                            &format!("/v1/cas/{}", digest.to_hex()),
                        )?
                        .header("content-length", size)
                        .body(reqwest::Body::wrap_stream(
                            progress
                                .track_bytes(ReaderStream::with_capacity(reader, 64 * 1024), size),
                        )),
                    ),
                )
                .await?;
            http::checked(response, "sprites")?;
            uploaded_bytes = uploaded_bytes.saturating_add(size);
        }
        tracing::debug!(
            provider = "sprites",
            phase = "worker_input_transfer",
            elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            missing_blobs = missing_count,
            shared_fetched_blobs = absent.saturating_sub(missing_count),
            uploaded_bytes,
            "worker transfer phase completed"
        );
        let started = std::time::Instant::now();
        let request = crate::worker::Request {
            version: crate::worker::PROTOCOL_VERSION,
            environment: environment.into(),
            argv: command.argv.to_vec(),
            env: command.env.clone(),
            cwd: command.cwd.cloned(),
            inputs,
            post_setup_inputs,
            outputs: command.outputs.to_vec(),
            clean_paths: setup.clean_paths.to_vec(),
            create_dirs: setup.create_dirs.to_vec(),
            stdout: command.redirect.stdout.cloned(),
            stderr: command.redirect.stderr.cloned(),
            timeout_ms: command.timeout_ms,
            success_exit_codes: command.success_exit_codes.to_vec(),
            memory_bytes: command.resources.memory_bytes,
            shared_cache,
        };
        let body = http::encode(&request, "sprites", self.limits.max_metadata_bytes)?;
        // A lost response is an uncertain execution, never an invitation to replay it.
        let response = self
            .send(
                self.request(reqwest::Method::POST, "/v1/actions")?
                    .header("content-type", "application/json")
                    .body(body),
            )
            .await?;
        let response: crate::worker::Response = http::decode(
            response,
            "sprites",
            self.limits.max_metadata_bytes,
            self.limits,
        )
        .await?;
        tracing::debug!(
            provider = "sprites",
            phase = "worker_remote_action",
            elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            shared_published_blobs = response.shared_blobs.len(),
            "worker transfer phase completed"
        );
        let started = std::time::Instant::now();
        if !response.result.outputs.is_empty() {
            return Err(api_error("worker returned unsupported output references"));
        }
        if let Some(digest) = response.output_archive {
            if !command.accepts_exit_code(response.result.exit_code) || command.outputs.is_empty() {
                return Err(api_error("worker returned an unexpected output archive"));
            }
            let archive = crate::remote::archive::output_archive_file(workspace)?;
            let phase = self
                .download(
                    digest,
                    archive.path(),
                    command,
                    cache,
                    &response.shared_blobs,
                )
                .await?;
            if std::fs::metadata(archive.path())
                .map_err(|source| crate::Error::FileAction {
                    action: "read_worker_output",
                    path: "worker output".into(),
                    source,
                })?
                .len()
                != response.output_archive_bytes
            {
                return Err(api_error("worker output archive length mismatch"));
            }
            crate::remote::archive::install_output_archive_limited(
                archive.path(),
                workspace,
                command.outputs,
                "sprites",
                self.limits,
                phase,
            )
            .await?;
        } else if command.accepts_exit_code(response.result.exit_code)
            && !command.outputs.is_empty()
        {
            return Err(api_error("worker omitted declared outputs"));
        }
        for (digest, stderr) in [
            (response.result.stdout, false),
            (response.result.stderr, true),
        ] {
            if let Some(digest) = digest {
                let file = crate::remote::archive::output_archive_file(workspace)?;
                let _phase = self
                    .download(digest, file.path(), command, cache, &response.shared_blobs)
                    .await?;
                let reader = tokio::fs::File::open(file.path()).await.map_err(|source| {
                    crate::Error::FileAction {
                        action: "read_worker_capture",
                        path: "worker capture".into(),
                        source,
                    }
                })?;
                if cache.put_stream(reader).await? != digest {
                    return Err(api_error("worker capture digest mismatch"));
                }
                if mirror {
                    let mut reader =
                        tokio::fs::File::open(file.path()).await.map_err(|source| {
                            crate::Error::FileAction {
                                action: "read_worker_capture",
                                path: "worker capture".into(),
                                source,
                            }
                        })?;
                    if stderr {
                        tokio::io::copy(&mut reader, &mut tokio::io::stderr()).await
                    } else {
                        tokio::io::copy(&mut reader, &mut tokio::io::stdout()).await
                    }
                    .map_err(|source| crate::Error::FileAction {
                        action: "mirror_worker_capture",
                        path: "worker capture".into(),
                        source,
                    })?;
                }
            }
        }
        tracing::debug!(
            provider = "sprites",
            phase = "worker_output_transfer",
            elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            archive_bytes = response.output_archive_bytes,
            "worker transfer phase completed"
        );
        Ok(response.result)
    }

    async fn download(
        &self,
        digest: Digest,
        destination: &Path,
        command: &PreparedCommand<'_>,
        cache: &CacheProvider,
        shared: &[Digest],
    ) -> Result<crate::remote::PhasePermit> {
        if shared.contains(&digest) {
            if let Ok(Some(phase)) = self
                .download_shared(cache, digest, destination, command)
                .await
            {
                return Ok(phase);
            }
            tracing::debug!("shared output unavailable; retrieving retained worker blob");
        }
        let response = self
            .send(
                self.request(
                    reqwest::Method::GET,
                    &format!("/v1/cas/{}", digest.to_hex()),
                )?
                .timeout(self.limits.artifact_download_timeout),
            )
            .await?;
        let phase = http::download(
            response,
            destination,
            "sprites",
            self.limits,
            command.lease.as_ref(),
            2,
        )
        .await?;
        let path = destination.to_path_buf();
        let valid = tokio::task::spawn_blocking(move || {
            Digest::of_reader(std::io::BufReader::new(std::fs::File::open(path)?))
        })
        .await
        .map_err(|_| api_error("worker blob verification task failed"))?
        .map_err(|source| crate::Error::FileAction {
            action: "verify_worker_blob",
            path: "worker blob".into(),
            source,
        })?;
        if valid != digest {
            return Err(api_error("worker blob digest mismatch"));
        }
        Ok(phase)
    }
}
