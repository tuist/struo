use std::sync::Mutex;

use anyhow::Result;
use struo_cas::ActionResult;
use struo_core::{
    Action, ActionOutputObserver, ActionOutputStream, ExecutionPermit, OutputSymlinkMode,
};
use struo_frontend::analysis::{DeclaredAction, DeclaredReadyOutput};
use tokio::sync::mpsc;
use tracing::Instrument;

use super::{provider, DeclaredActionContext};

const LINE_LIMIT: usize = 64 * 1024;

struct Stream {
    line: Vec<u8>,
    oversized: bool,
}

pub(super) struct Observer<'a> {
    rules: &'a [DeclaredReadyOutput],
    streams: Mutex<[Stream; 2]>,
    sent: Mutex<Vec<bool>>,
    events: mpsc::Sender<usize>,
    forward: Option<&'a dyn ActionOutputObserver>,
}

pub(super) fn rendered_field<'a>(
    rules: &'a [DeclaredReadyOutput],
    stream: &str,
) -> Option<&'a str> {
    rules
        .iter()
        .filter(|rule| rule.stream == stream)
        .find_map(|rule| rule.rendered_field.as_deref())
}

pub(super) fn render(bytes: &[u8], field: &str) -> Vec<u8> {
    let mut rendered = Vec::new();
    for line in bytes.split_inclusive(|byte| *byte == b'\n') {
        match serde_json::from_slice::<serde_json::Value>(line) {
            Ok(event) if event.is_object() => {
                if let Some(text) = event.get(field).and_then(serde_json::Value::as_str) {
                    rendered.extend_from_slice(text.as_bytes());
                }
            }
            _ => rendered.extend_from_slice(line),
        }
    }
    rendered
}

impl<'a> Observer<'a> {
    pub(super) fn new(
        rules: &'a [DeclaredReadyOutput],
        forward: Option<&'a dyn ActionOutputObserver>,
    ) -> (Self, mpsc::Receiver<usize>) {
        let (events, receiver) = mpsc::channel(rules.len().max(1));
        (
            Self {
                rules,
                streams: Mutex::new(std::array::from_fn(|_| Stream {
                    line: Vec::new(),
                    oversized: false,
                })),
                sent: Mutex::new(vec![false; rules.len()]),
                events,
                forward,
            },
            receiver,
        )
    }

    fn forward_line(&self, stream: ActionOutputStream, bytes: &[u8], field: Option<&str>) {
        if let (Some(forward), Some(field)) = (self.forward, field) {
            forward.observe(stream, &render(bytes, field));
        }
    }

    pub(super) fn finish(&self) {
        let streams = self.streams.lock().unwrap();
        for (index, (stream, name)) in [
            (ActionOutputStream::Stdout, "stdout"),
            (ActionOutputStream::Stderr, "stderr"),
        ]
        .into_iter()
        .enumerate()
        {
            self.forward_line(
                stream,
                &streams[index].line,
                rendered_field(self.rules, name),
            );
        }
    }
}

impl ActionOutputObserver for Observer<'_> {
    fn observe(&self, stream: ActionOutputStream, bytes: &[u8]) {
        if self.rules.is_empty() {
            if let Some(forward) = self.forward {
                forward.observe(stream, bytes);
            }
            return;
        }
        let (index, name) = match stream {
            ActionOutputStream::Stdout => (0, "stdout"),
            ActionOutputStream::Stderr => (1, "stderr"),
        };
        let field = rendered_field(self.rules, name);
        if field.is_none() {
            if let Some(forward) = self.forward {
                forward.observe(stream, bytes);
            }
        }
        let mut streams = self.streams.lock().unwrap();
        let state = &mut streams[index];
        for byte in bytes {
            if *byte == b'\n' {
                if !state.oversized {
                    if let Ok(event) = serde_json::from_slice::<serde_json::Value>(&state.line) {
                        let mut sent = self.sent.lock().unwrap();
                        for (index, rule) in self.rules.iter().enumerate() {
                            if !sent[index]
                                && rule.stream == name
                                && rule.event.iter().all(|(key, value)| {
                                    event.get(key).and_then(serde_json::Value::as_str)
                                        == Some(value.as_str())
                                })
                            {
                                sent[index] = true;
                                let _ = self.events.try_send(index);
                            }
                        }
                    }
                }
                if !state.oversized {
                    state.line.push(b'\n');
                    self.forward_line(stream, &state.line, field);
                } else if field.is_some() {
                    if let Some(forward) = self.forward {
                        forward.observe(stream, b"\n");
                    }
                }
                state.line.clear();
                state.oversized = false;
            } else if state.line.len() < LINE_LIMIT && !state.oversized {
                state.line.push(*byte);
            } else {
                if field.is_some() {
                    if let Some(forward) = self.forward {
                        if !state.oversized {
                            forward.observe(stream, &state.line);
                        }
                        forward.observe(stream, std::slice::from_ref(byte));
                    }
                }
                state.line.clear();
                state.oversized = true;
            }
        }
    }
}

pub(super) async fn execute(
    context: &DeclaredActionContext<'_>,
    action: &Action,
    declared: &DeclaredAction,
    lease: ExecutionPermit,
) -> Result<ActionResult> {
    let local = struo_core::Executor::resolve(action)? == struo_core::Executor::Local;
    let enabled = context.pipeline.is_some()
        && !declared.ready_outputs.is_empty()
        && local
        && matches!(
            action,
            Action::RunCommand {
                sandbox: struo_core::SandboxMode::Off,
                ..
            }
        );
    let (observer, mut receiver) = Observer::new(&declared.ready_outputs, context.output_observer);
    let execution = provider::execute(
        action,
        context.workspace,
        context.cache,
        if local && !declared.ready_outputs.is_empty() {
            Some(&observer)
        } else if local {
            context.output_observer
        } else {
            None
        },
        lease.clone(),
        &declared.clean_paths,
        &declared.create_dirs,
        &declared.arg_files,
    )
    .instrument(tracing::debug_span!(
        "graph_action",
        target = context.target_id,
        action_index = context.index,
        identifier = context.identifier
    ));
    tokio::pin!(execution);
    let result = loop {
        tokio::select! {
            biased;
            Some(index) = receiver.recv(), if enabled => {
                let path = super::workspace_path(&declared.ready_outputs[index].path, "ready output")?;
                anyhow::ensure!(tokio::fs::symlink_metadata(path.resolve(context.workspace)).await?.is_file(), "ready output must be a regular file");
                let captured = struo_core::capture_outputs_admitted(
                    std::slice::from_ref(&path), context.workspace, &context.cache.local_tier(),
                    OutputSymlinkMode::default(), Some(&lease),
                ).await?;
                context.pipeline.unwrap().ready(context.target_id, path.as_str(), captured[path.as_str()]).await?;
                tracing::debug!(target = context.target_id, path = path.as_str(), "published early action output");
            }
            result = &mut execution => {
                observer.finish();
                break result?;
            }
        }
    };
    if let Some(pipeline) = context.pipeline {
        pipeline.validate(context.target_id, &result.outputs)?;
    }
    Ok(result)
}
