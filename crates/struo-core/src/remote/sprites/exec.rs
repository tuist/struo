use std::collections::BTreeMap;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use struo_cas::{ActionResult, CacheProvider};
use tokio_tungstenite::tungstenite::{
    client::IntoClientRequest, protocol::WebSocketConfig, Message,
};

use super::client::{api_error, Client, Sprite};
use crate::executor::staging::CapturePermit;
use crate::remote::http::check_size;
use crate::stream::{self, Destination, PIPE_CAPACITY};
use crate::{Error, Result};

impl Client {
    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    pub async fn execute(
        &self,
        sprite: &Sprite,
        argv: &[String],
        cwd: Option<&str>,
        env: &BTreeMap<String, String>,
        cache: &CacheProvider,
        mirror: bool,
        budget: Option<&CapturePermit>,
    ) -> Result<ActionResult> {
        let program = argv.first().ok_or(Error::EmptyArgv)?;
        let metadata_bytes = argv
            .iter()
            .map(|arg| arg.len() as u64)
            .chain(
                env.iter()
                    .map(|(key, value)| (key.len() as u64).saturating_add(value.len() as u64)),
            )
            .fold(0_u64, u64::saturating_add);
        check_size(
            metadata_bytes,
            self.limits.control_bytes(),
            "sprites",
            "request metadata bytes",
        )?;
        let mut url = self.url(&sprite.name, "exec")?;
        let scheme = if url.scheme() == "https" { "wss" } else { "ws" };
        url.set_scheme(scheme)
            .map_err(|()| api_error("invalid WebSocket URL"))?;
        {
            let mut query = url.query_pairs_mut();
            query
                .append_pair("path", program)
                .append_pair("tty", "false")
                .append_pair("stdin", "false")
                .append_pair("max_run_after_disconnect", "1s");
            for arg in argv {
                query.append_pair("cmd", arg);
            }
            if let Some(cwd) = cwd {
                query.append_pair("dir", cwd);
            }
            for (key, value) in env {
                query.append_pair("env", &format!("{key}={value}"));
            }
        }
        check_size(
            url.as_str().len() as u64,
            self.limits.control_bytes(),
            "sprites",
            "request metadata bytes",
        )?;
        let mut request = url
            .as_str()
            .into_client_request()
            .map_err(|_| api_error("invalid WebSocket request"))?;
        request.headers_mut().insert(
            "Authorization",
            format!("Bearer {}", self.token)
                .parse()
                .map_err(|_| api_error("invalid bearer token"))?,
        );
        let max = usize::try_from(self.limits.max_response_bytes).unwrap_or(usize::MAX);
        let config = WebSocketConfig::default()
            .max_message_size(Some(max))
            .max_frame_size(Some(max));
        let mut socket = tokio::time::timeout(Duration::from_secs(30), async {
            let key = request.headers().get("Sec-WebSocket-Key").unwrap();
            let expected =
                tokio_tungstenite::tungstenite::handshake::derive_accept_key(key.as_bytes());
            url.set_scheme(if scheme == "wss" { "https" } else { "http" })
                .unwrap();
            let response = self
                .http
                .get(url)
                .headers(request.headers().clone())
                .send()
                .await
                .map_err(|_| api_error("WebSocket connection failed"))?;
            if response.status() != reqwest::StatusCode::SWITCHING_PROTOCOLS
                || response
                    .headers()
                    .get("Sec-WebSocket-Accept")
                    .and_then(|h| h.to_str().ok())
                    != Some(expected.as_str())
                || !response
                    .headers()
                    .get("Upgrade")
                    .is_some_and(|h| h.as_bytes().eq_ignore_ascii_case(b"websocket"))
                || !response
                    .headers()
                    .get("Connection")
                    .and_then(|h| h.to_str().ok())
                    .is_some_and(|h| {
                        h.split(',')
                            .any(|v| v.trim().eq_ignore_ascii_case("upgrade"))
                    })
            {
                return Err(api_error("invalid WebSocket upgrade response"));
            }
            let stream = response
                .upgrade()
                .await
                .map_err(|_| api_error("WebSocket upgrade failed"))?;
            Ok(tokio_tungstenite::WebSocketStream::from_raw_socket(
                super::transport::Transport::new(stream, self.limits.stall_timeout),
                tokio_tungstenite::tungstenite::protocol::Role::Client,
                Some(config),
            )
            .await)
        })
        .await
        .map_err(|_| api_error("WebSocket connection timed out"))??;
        let (stdout_reader, mut stdout) = tokio::io::duplex(PIPE_CAPACITY);
        let (stderr_reader, mut stderr) = tokio::io::duplex(PIPE_CAPACITY);
        let decode = async {
            let mut captured = 0_u64;
            let exit_code = loop {
                let next = async { socket.next().await };
                let message = if let Some(idle) = self.limits.command_idle_timeout {
                    tokio::time::timeout(idle, next)
                        .await
                        .map_err(|_| Error::ExecutorStalled {
                            executor: "sprites".into(),
                            phase: "command output",
                            timeout: idle,
                        })?
                } else {
                    next.await
                };
                let message = message
                    .ok_or_else(|| api_error("command stream ended without an exit code"))?
                    .map_err(|error| websocket_error(&error, self.limits.stall_timeout))?;
                match message {
                    Message::Binary(bytes) => match binary_event(&bytes)? {
                        Event::Output(destination, payload) => {
                            captured = captured.saturating_add(payload.len() as u64);
                            check_size(
                                captured,
                                self.limits.max_capture_bytes,
                                "sprites",
                                "captured output bytes",
                            )?;
                            if let Some(budget) = budget {
                                budget.reserve(payload.len() as u64)?;
                            }
                            stream::write_parent(payload, destination, mirror).await?;
                            let writer = match destination {
                                Destination::Stdout => &mut stdout,
                                Destination::Stderr => &mut stderr,
                            };
                            stream::write_pipe(writer, payload).await?;
                        }
                        Event::Exit(code) => break code,
                    },
                    Message::Text(text) => {
                        let event: serde_json::Value = serde_json::from_str(&text)
                            .map_err(|_| api_error("invalid command event"))?;
                        match event.get("type").and_then(serde_json::Value::as_str) {
                            Some("exit") => {
                                break event
                                    .get("exit_code")
                                    .and_then(serde_json::Value::as_i64)
                                    .and_then(|code| i32::try_from(code).ok())
                                    .ok_or_else(|| api_error("invalid exit code"))?
                            }
                            Some("debug" | "session_info" | "port_opened" | "port_closed") => {}
                            _ => return Err(api_error("unexpected command event")),
                        }
                    }
                    Message::Ping(bytes) => socket
                        .send(Message::Pong(bytes))
                        .await
                        .map_err(|error| websocket_error(&error, self.limits.stall_timeout))?,
                    Message::Pong(_) => {}
                    Message::Close(_) => {
                        return Err(api_error("command stream closed without an exit code"))
                    }
                    Message::Frame(_) => return Err(api_error("unexpected WebSocket frame")),
                }
            };
            stream::shutdown_pipe(&mut stdout).await?;
            stream::shutdown_pipe(&mut stderr).await?;
            Ok::<_, Error>(exit_code)
        };
        let (exit_code, stdout, stderr) = tokio::try_join!(
            decode,
            async { cache.put_stream(stdout_reader).await.map_err(Error::from) },
            async { cache.put_stream(stderr_reader).await.map_err(Error::from) }
        )?;
        Ok(ActionResult {
            exit_code,
            stdout: Some(stdout),
            stderr: Some(stderr),
            outputs: BTreeMap::new(),
        })
    }
}

enum Event<'a> {
    Output(Destination, &'a [u8]),
    Exit(i32),
}

fn websocket_error(error: &tokio_tungstenite::tungstenite::Error, timeout: Duration) -> Error {
    if matches!(error, tokio_tungstenite::tungstenite::Error::Io(error) if error.kind() == std::io::ErrorKind::TimedOut)
    {
        Error::ExecutorStalled {
            executor: "sprites".into(),
            phase: "WebSocket transfer",
            timeout,
        }
    } else {
        api_error("WebSocket command stream failed")
    }
}

fn binary_event(bytes: &[u8]) -> Result<Event<'_>> {
    match bytes.split_first() {
        Some((1, payload)) => Ok(Event::Output(Destination::Stdout, payload)),
        Some((2, payload)) => Ok(Event::Output(Destination::Stderr, payload)),
        Some((3, [code])) => Ok(Event::Exit(i32::from(*code))),
        _ => Err(api_error("invalid binary command frame")),
    }
}
