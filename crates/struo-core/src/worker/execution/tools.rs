use std::io::{self, Read};
use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};
use tokio_util::sync::CancellationToken;

use super::super::cas::invalid;
use super::super::EXECUTION_ROOT;
use super::{process, Executor};

impl Executor {
    pub(super) async fn resolve_value(
        &mut self,
        value: &str,
        root: &Path,
        workspace: &Path,
        cancel: &CancellationToken,
    ) -> io::Result<String> {
        let mut value = value.to_owned();
        for (marker, tool) in crate::execution_tool::ExecutionTool::find(&value)
            .map_err(|_| invalid("invalid execution tool marker"))?
        {
            let binding = if let Some(binding) = self.verified.get(&marker) {
                binding.clone()
            } else {
                if self.verified.len() >= 1024 {
                    return Err(invalid("worker tool binding capacity exceeded"));
                }
                let discovery = tempfile::tempdir_in(root)?;
                let mut argv = self.options.tool_launcher.clone();
                argv.extend(tool.resolve);
                let output = process::run(
                    &self.options,
                    workspace,
                    &argv,
                    &self.options.tool_environment,
                    EXECUTION_ROOT,
                    discovery.path(),
                    Some(30_000),
                    32 * 1024,
                    cancel,
                    crate::local::Redirect::default(),
                )
                .await?;
                if output.exit_code != 0 {
                    return Err(invalid("execution tool discovery failed"));
                }
                let path = std::fs::read_to_string(output.stdout)?;
                if !Path::new(path.trim()).is_absolute() || path.trim().contains(['\n', '\r', '\0'])
                {
                    return Err(invalid(
                        "execution tool resolver did not return one absolute path",
                    ));
                }
                let mut resolved = PathBuf::from(path.trim());
                if !tool.suffix.is_empty() {
                    resolved.push(tool.suffix);
                }
                let resolved = std::fs::canonicalize(resolved)?;
                let preparation = std::fs::canonicalize(&self.options.preparation_root)?;
                // Only explicitly exposed immutable files may become cached bindings.
                let allowed = ["mise", "data", "env", "system.toml"]
                    .into_iter()
                    .any(|part| resolved.starts_with(preparation.join(part)));
                if !allowed || !resolved.is_file() {
                    return Err(invalid("tool is outside immutable prepared files"));
                }
                let mut file = std::fs::File::open(&resolved)?;
                let mut hasher = Sha256::new();
                let mut buffer = [0_u8; 16 * 1024];
                loop {
                    let read = file.read(&mut buffer)?;
                    if read == 0 {
                        break;
                    }
                    hasher.update(&buffer[..read]);
                }
                if hex::encode(hasher.finalize()) != tool.sha256.to_ascii_lowercase() {
                    return Err(invalid("prepared execution tool SHA256 mismatch"));
                }
                let binding = resolved
                    .to_str()
                    .ok_or_else(|| invalid("non-UTF8 tool path"))?
                    .to_owned();
                let retained = self
                    .verified
                    .iter()
                    .map(|(key, value)| key.len() + value.len())
                    .sum::<usize>();
                if retained as u64 + marker.len() as u64 + binding.len() as u64
                    > self.options.memory_bytes / 64
                {
                    return Err(invalid("worker tool binding memory capacity exceeded"));
                }
                self.verified.insert(marker.clone(), binding.clone());
                discovery
                    .close()
                    .map_err(|_| io::Error::other("worker discovery cleanup failed"))?;
                binding
            };
            value = value.replace(&marker, &binding);
        }
        // Substitute private homes before the common helper can resolve them to
        // service-owned paths from the worker process environment.
        value = value
            .replace("{{struo.home}}", "/private-home")
            .replace("{{struo.data_dir}}", "/private-home/.local/share/struo")
            .replace("{{struo.cache_dir}}", "/private-home/.cache/struo");
        Ok(crate::resolve_execution_value(
            &value,
            Path::new(EXECUTION_ROOT),
        ))
    }
}
