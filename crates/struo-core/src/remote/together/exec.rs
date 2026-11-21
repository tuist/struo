use std::collections::BTreeMap;
use std::time::Duration;

use reqwest::Method;
use serde::{Deserialize, Serialize};

use super::client::{api_error, component, Client, Sandbox};
use crate::Result;

#[derive(Deserialize)]
struct Exec {
    id: String,
    status: String,
    #[serde(rename = "exitCode")]
    exit_code: Option<i32>,
}

#[derive(Serialize)]
struct ExecRequest<'a> {
    command: &'a str,
    args: &'a [String],
    cwd: &'a str,
    env: &'a BTreeMap<String, String>,
    autostart: bool,
    pty: bool,
}

impl Client {
    pub(super) async fn exec(
        &self,
        sandbox: &Sandbox,
        program: &str,
        args: &[String],
        cwd: &str,
        env: &BTreeMap<String, String>,
    ) -> Result<i32> {
        let request = self
            .guest(Method::POST, sandbox, "/api/v1/execs")?
            .timeout(Duration::from_secs(30));
        let mut exec: Exec = self
            .json(self.body(
                request,
                &ExecRequest {
                    command: program,
                    args,
                    cwd,
                    env,
                    autostart: true,
                    pty: false,
                },
            )?)
            .await?;
        loop {
            let status = exec.status.to_ascii_lowercase();
            if matches!(status.as_str(), "exited" | "finished" | "stopped") {
                return exec
                    .exit_code
                    .ok_or_else(|| api_error("exec ended without an exit code".into()));
            }
            if !matches!(status.as_str(), "running" | "created") {
                return Err(api_error(format!("unexpected exec status {}", exec.status)));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
            exec = self
                .json(
                    self.guest(
                        Method::GET,
                        sandbox,
                        &format!("/api/v1/execs/{}", component(&exec.id)),
                    )?
                    .timeout(Duration::from_secs(30)),
                )
                .await?;
        }
    }
}
