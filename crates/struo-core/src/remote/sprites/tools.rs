use std::collections::BTreeMap;
use std::path::Path;

use struo_cas::CacheProvider;

use super::{
    client::{api_error, Client, Sprite},
    preparation,
};
use crate::execution_tool::ExecutionTool;
use crate::executor::staging::CapturePermit;
use crate::remote::PreparedCommand;
use crate::Result;

pub(super) async fn command(
    client: &Client,
    sprite: &Sprite,
    command: &PreparedCommand<'_>,
    cache: &CacheProvider,
    capture: Option<&CapturePermit>,
) -> Result<(Vec<String>, BTreeMap<String, String>)> {
    let tools = resolve(client, sprite, command, cache, capture).await?;
    let resolve = |value: &str| {
        tools
            .iter()
            .fold(value.to_string(), |value, (marker, path)| {
                value.replace(marker, path)
            })
    };
    let argv: Vec<_> = command.argv.iter().map(|value| resolve(value)).collect();
    let env = command
        .env
        .iter()
        .map(|(key, value)| (key.clone(), resolve(value)))
        .collect();
    let mut argv = crate::resolve_execution_argv(&argv, Path::new(super::GUEST_ROOT));
    let mut env = crate::resolve_execution_env(&env, Path::new(super::GUEST_ROOT));
    let mut wrapped = vec![
        preparation::MISE.into(),
        "exec".into(),
        "--locked".into(),
        "--".into(),
    ];
    wrapped.append(&mut argv);
    for (key, value) in preparation::environment() {
        if key == "HOME" || key == "TMPDIR" || key == "PATH" {
            env.entry(key).or_insert(value);
        } else {
            env.insert(key, value);
        }
    }
    Ok((wrapped, env))
}

async fn resolve(
    client: &Client,
    sprite: &Sprite,
    command: &PreparedCommand<'_>,
    cache: &CacheProvider,
    capture: Option<&CapturePermit>,
) -> Result<BTreeMap<String, String>> {
    let mut tools = BTreeMap::new();
    for value in command.argv.iter().chain(command.env.values()) {
        for (marker, tool) in ExecutionTool::find(value).map_err(|error| api_error(&error))? {
            tools.insert(marker, tool);
        }
    }
    if tools.len() > 64 {
        return Err(api_error("action exceeds 64 execution tool bindings"));
    }
    let env = preparation::environment();
    let mut resolved = BTreeMap::new();
    for (marker, tool) in tools {
        let mut argv = vec![
            preparation::MISE.into(),
            "exec".into(),
            "--locked".into(),
            "--".into(),
        ];
        argv.extend(tool.resolve);
        let path = output(client, sprite, &argv, &env, cache, capture).await?;
        let path = path.trim();
        if !Path::new(path).is_absolute() || path.chars().any(char::is_control) {
            return Err(api_error(
                "execution tool resolver must return one absolute path",
            ));
        }
        let path = if tool.suffix.is_empty() {
            path.to_string()
        } else {
            format!("{}/{}", path.trim_end_matches('/'), tool.suffix)
        };
        let digest = output(
            client,
            sprite,
            &["sha256sum".into(), "--".into(), path.clone()],
            &env,
            cache,
            capture,
        )
        .await?;
        if digest.split_whitespace().next() != Some(tool.sha256.as_str()) {
            return Err(api_error(&format!("prepared tool does not match analysis executable {}; align the coordinator toolchain with mise.lock", tool.path)));
        }
        tracing::debug!(provider = "sprites", analysis_path = %tool.path, execution_path = %path, "verified execution tool binding");
        resolved.insert(marker, path);
    }
    Ok(resolved)
}

async fn output(
    client: &Client,
    sprite: &Sprite,
    argv: &[String],
    env: &BTreeMap<String, String>,
    cache: &CacheProvider,
    capture: Option<&CapturePermit>,
) -> Result<String> {
    let result = client
        .execute(
            sprite,
            argv,
            Some(preparation::ROOT),
            env,
            cache,
            false,
            capture,
        )
        .await?;
    if result.exit_code != 0 {
        return Err(api_error(
            "execution tool discovery failed in locked environment",
        ));
    }
    let digest = result
        .stdout
        .ok_or_else(|| api_error("execution tool discovery returned no path"))?;
    let (bytes, truncated) = cache.read_blob_limited(&digest, 4096, false).await?;
    if truncated {
        return Err(api_error("execution tool discovery output exceeds 4 KiB"));
    }
    String::from_utf8(bytes).map_err(|_| api_error("execution tool discovery output must be UTF-8"))
}
