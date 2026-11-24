use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use serde::Deserialize;

use super::client::{api_error, Client, Sandbox};
use super::{GUEST_ROOT, OUTPUT_ARCHIVE};
use crate::{Error, Result, WorkspacePath};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ExecuteResponse {
    pub status: String,
    pub pid: Option<String>,
    pub exit_code: Option<i32>,
}

impl ExecuteResponse {
    pub(super) fn exit_code(&self) -> Result<i32> {
        if !matches!(self.status.as_str(), "completed" | "failed") {
            return Err(api_error("process did not complete normally"));
        }
        self.exit_code
            .ok_or_else(|| api_error("process response omitted exit code"))
    }
}

impl Client {
    pub(super) async fn execute(
        &self,
        sandbox: &Sandbox,
        command: &str,
        cwd: Option<&str>,
        env: &BTreeMap<String, String>,
        timeout_ms: Option<u64>,
    ) -> Result<ExecuteResponse> {
        let timeout = Duration::from_millis(timeout_ms.unwrap_or(600_000));
        let deadline = Instant::now() + timeout;
        let body = serde_json::json!({
            "command": command, "workingDir": cwd.unwrap_or("/tmp"), "env": env,
            "waitForCompletion": false, "keepAlive": true,
            "timeout": timeout.as_secs().saturating_add(u64::from(timeout.subsec_nanos() > 0)).max(1),
            "restartOnFailure": false
        });
        let response = self
            .guest(sandbox, reqwest::Method::POST, "process")?
            .header("Content-Type", "application/json")
            .body(super::super::http::encode(
                &body,
                "blaxel",
                self.limits.control_bytes(),
            )?)
            .send()
            .await
            .map_err(super::client::http_error)?;
        let mut process: ExecuteResponse = self.decode(response).await?;
        loop {
            match process.status.as_str() {
                "completed" | "failed" => {
                    process.exit_code()?;
                    return Ok(process);
                }
                "running" => {}
                _ => return Err(api_error("remote process failed, stopped, or was killed")),
            }
            if Instant::now() >= deadline {
                return Err(Error::Timeout(timeout));
            }
            let pid = process
                .pid
                .as_deref()
                .ok_or_else(|| api_error("running process omitted pid"))?;
            if pid.is_empty() || !pid.bytes().all(|b| b.is_ascii_digit()) {
                return Err(api_error("invalid process pid"));
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
            process = self
                .decode(
                    self.guest(sandbox, reqwest::Method::GET, &format!("process/{pid}"))?
                        .send()
                        .await
                        .map_err(super::client::http_error)?,
                )
                .await?;
        }
    }
}

pub(super) fn ensure_success(response: &ExecuteResponse, operation: &str) -> Result<()> {
    let code = response.exit_code()?;
    if code == 0 {
        Ok(())
    } else {
        Err(api_error(format!(
            "{operation} failed with exit code {code}"
        )))
    }
}

pub(super) fn command_line(argv: &[String]) -> Result<String> {
    if argv.is_empty() {
        return Err(Error::EmptyArgv);
    }
    Ok(argv
        .iter()
        .map(|arg| shell_word(arg))
        .collect::<Vec<_>>()
        .join(" "))
}

pub(super) fn shell_word(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub(super) fn workdir(cwd: Option<&WorkspacePath>) -> String {
    cwd.map_or_else(
        || GUEST_ROOT.into(),
        |cwd| super::super::join_path(GUEST_ROOT, cwd.as_str()),
    )
}

pub(super) fn pack_outputs_command(outputs: &[WorkspacePath]) -> String {
    let paths = outputs
        .iter()
        .map(|path| shell_word(&format!("./{}", path.as_str())))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "tar --hard-dereference -cf {} -C {} {paths}",
        shell_word(OUTPUT_ARCHIVE),
        shell_word(GUEST_ROOT)
    )
}
