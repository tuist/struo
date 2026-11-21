use std::collections::BTreeMap;
use std::path::Path;

use reqwest::Method;
use struo_cas::{ActionResult, CacheProvider};
use tokio_util::io::ReaderStream;

use super::client::{api_error, component, http_error, Client, Sandbox};
use super::{OUTPUT, ROOT};
use crate::executor::staging::CapturePermit;
use crate::remote::{archive::output_archive_file, http, PhasePermit, PreparedCommand};
use crate::stream::{self, Destination};
use crate::{Error, ExecutionPermit, Result};

impl Client {
    pub async fn restore_outputs(
        &self,
        sandbox: &Sandbox,
        command: &PreparedCommand<'_>,
        workspace: &Path,
    ) -> Result<()> {
        let mut args = vec![
            "--hard-dereference".into(),
            "-cf".into(),
            OUTPUT.into(),
            "-C".into(),
            ROOT.into(),
        ];
        args.extend(
            command
                .outputs
                .iter()
                .map(|path| format!("./{}", path.as_str())),
        );
        self.setup(sandbox, "tar", &args).await?;
        let archive = output_archive_file(workspace)?;
        let phase = self
            .download(sandbox, OUTPUT, archive.path(), command.lease.as_ref(), 2)
            .await?;
        crate::remote::archive::install_output_archive_limited(
            archive.path(),
            workspace,
            command.outputs,
            "together",
            self.limits,
            phase,
        )
        .await
    }

    pub async fn upload(
        &self,
        sandbox: &Sandbox,
        source: &Path,
        len: u64,
        destination: &str,
    ) -> Result<()> {
        let file = tokio::fs::File::open(source).await.map_err(file_error)?;
        let progress = http::Progress::new();
        let body = reqwest::Body::wrap_stream(progress.track_bytes(ReaderStream::new(file), len));
        let response = progress
            .watch_upload("together", self.limits, async {
                self.guest(
                    Method::POST,
                    sandbox,
                    &format!("/api/v1/files/{}", component(destination)),
                )?
                .header("Content-Type", "application/octet-stream")
                .header("Content-Length", len)
                .body(body)
                .send()
                .await
                .map_err(http_error)
            })
            .await?;
        http::checked(response, "together")?;
        Ok(())
    }

    pub async fn download(
        &self,
        sandbox: &Sandbox,
        source: &str,
        destination: &Path,
        lease: Option<&ExecutionPermit>,
        copies: u64,
    ) -> Result<PhasePermit> {
        let response = http::Progress::new()
            .watch(
                "together",
                "download headers",
                self.limits.stall_timeout,
                async {
                    self.guest(
                        Method::GET,
                        sandbox,
                        &format!("/api/v1/files/{}", component(source)),
                    )?
                    .header("Accept", "application/octet-stream")
                    .timeout(self.limits.artifact_download_timeout)
                    .send()
                    .await
                    .map_err(http_error)
                },
            )
            .await?;
        if !response
            .headers()
            .get("content-type")
            .is_some_and(|value| value.as_bytes().starts_with(b"application/octet-stream"))
        {
            http::checked(response, "together")?;
            return Err(api_error(
                "guest agent does not support raw binary file reads; update the snapshot agent"
                    .into(),
            ));
        }
        http::download(
            response,
            destination,
            "together",
            self.limits,
            lease,
            copies,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn capture(
        &self,
        sandbox: &Sandbox,
        command: &PreparedCommand<'_>,
        workspace: &Path,
        cache: &CacheProvider,
        mirror: bool,
        budget: Option<CapturePermit>,
    ) -> Result<ActionResult> {
        let stdout = output_archive_file(workspace)?;
        let stderr = output_archive_file(workspace)?;
        let argv = crate::resolve_execution_argv(command.argv, Path::new(ROOT));
        if argv.is_empty() {
            return Err(Error::EmptyArgv);
        }
        let mut wrapper = vec![
            "-c".into(),
            "exec \"$@\" > /tmp/struo-stdout 2> /tmp/struo-stderr".into(),
            "struo".into(),
        ];
        wrapper.extend(argv);
        let cwd = command.cwd.map_or_else(
            || ROOT.into(),
            |cwd| super::super::join_path(ROOT, cwd.as_str()),
        );
        let env = crate::resolve_execution_env(command.env, Path::new(ROOT));
        let exit_code = self.exec(sandbox, "sh", &wrapper, &cwd, &env).await?;
        let mut reader = self.clone();
        reader.limits.max_archive_bytes = self
            .limits
            .max_capture_bytes
            .min(self.limits.max_archive_bytes);
        let mut digests = Vec::new();
        for (guest, file, destination) in [
            ("/tmp/struo-stdout", &stdout, Destination::Stdout),
            ("/tmp/struo-stderr", &stderr, Destination::Stderr),
        ] {
            // Capture already owns its full staging allowance before create.
            let phase = reader
                .download(sandbox, guest, file.path(), None, 1)
                .await?;
            let len = tokio::fs::metadata(file.path())
                .await
                .map_err(file_error)?
                .len();
            if let Some(budget) = &budget {
                budget.reserve(len)?;
            }
            reader.limits.max_archive_bytes = reader.limits.max_archive_bytes.saturating_sub(len);
            let file = tokio::fs::File::open(file.path())
                .await
                .map_err(file_error)?;
            digests.push(stream::to_cache(file, destination, cache, mirror).await?);
            drop(phase);
        }
        Ok(ActionResult {
            exit_code,
            stdout: Some(digests[0]),
            stderr: Some(digests[1]),
            outputs: BTreeMap::new(),
        })
    }
}

fn file_error(source: std::io::Error) -> Error {
    Error::FileAction {
        action: "remote_capture",
        path: ".struo/tmp".into(),
        source,
    }
}
