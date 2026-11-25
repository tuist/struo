use std::collections::BTreeMap;

use struo_cas::CacheProvider;

use super::client::{api_error, Client, Sprite};
use crate::executor::staging::CapturePermit;
use crate::remote::http;
use crate::{Result, WorkerPoolSpec};

pub(super) const ROOT: &str = "/home/sprite/.struo-worker";
pub(super) const MISE: &str = "/home/sprite/.struo-worker/mise";

pub(super) fn environment() -> BTreeMap<String, String> {
    [
        ("HOME", format!("{ROOT}/home")),
        (
            "PATH",
            "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".into(),
        ),
        ("MISE_DATA_DIR", format!("{ROOT}/data")),
        ("MISE_CACHE_DIR", format!("{ROOT}/cache")),
        ("MISE_CONFIG_DIR", format!("{ROOT}/config")),
        ("MISE_GLOBAL_CONFIG_FILE", format!("{ROOT}/env/mise.toml")),
        ("MISE_GLOBAL_CONFIG_ROOT", format!("{ROOT}/env")),
        ("MISE_SYSTEM_CONFIG_FILE", format!("{ROOT}/system.toml")),
        ("MISE_IGNORED_CONFIG_PATHS", super::GUEST_ROOT.into()),
        ("MISE_CEILING_PATHS", "/home/sprite".into()),
        (
            "MISE_OVERRIDE_CONFIG_FILENAMES",
            "struo-no-project-config.toml".into(),
        ),
        (
            "MISE_OVERRIDE_TOOL_VERSIONS_FILENAMES",
            "struo-no-tool-versions".into(),
        ),
        ("MISE_IDIOMATIC_VERSION_FILE_ENABLE_TOOLS", String::new()),
        ("MISE_YES", "1".into()),
        ("MISE_AUTO_INSTALL", "0".into()),
        ("MISE_EXEC_AUTO_INSTALL", "0".into()),
        ("TMPDIR", format!("{ROOT}/tmp")),
    ]
    .into_iter()
    .map(|(key, value)| (key.into(), value))
    .collect()
}

impl Client {
    pub(super) async fn write_file(
        &self,
        sprite: &Sprite,
        path: &str,
        contents: String,
    ) -> Result<()> {
        let response = self
            .request(reqwest::Method::PUT, self.url(&sprite.name, "fs/write")?)
            .query(&[("path", path), ("workingDir", "/"), ("mode", "0600")])
            .timeout(std::time::Duration::from_secs(60))
            .header("Content-Type", "application/octet-stream")
            .body(contents)
            .send()
            .await
            .map_err(|error| http::http_error("sprites", error))?;
        http::checked(response, "sprites")?;
        Ok(())
    }
}

pub(super) async fn prepare(
    client: &Client,
    sprite: &Sprite,
    spec: &WorkerPoolSpec,
    cache: &CacheProvider,
    capture: Option<&CapturePermit>,
) -> Result<()> {
    let mut env = environment();
    env.insert("HOME".into(), format!("{ROOT}/data/home"));
    let run = async |argv: Vec<String>| {
        tracing::debug!(provider = "sprites", program = ?argv.first(), "preparing locked worker");
        let result =
            Box::pin(client.execute(sprite, &argv, Some(ROOT), &env, cache, false, capture))
                .await?;
        if result.exit_code != 0 {
            tracing::warn!(provider = "sprites", program = ?argv.first(), exit_code = result.exit_code,
                stdout_digest = ?result.stdout, stderr_digest = ?result.stderr,
                "locked worker preparation failed");
            return Err(api_error("locked worker preparation failed; check setup argv and mise.lock linux-x64 entries"));
        }
        Ok(result)
    };
    super::setup(
        client,
        sprite,
        &[
            "mkdir".into(),
            "-p".into(),
            ROOT.into(),
            format!("{ROOT}/home"),
            format!("{ROOT}/data/home"),
            format!("{ROOT}/tmp"),
            format!("{ROOT}/env"),
        ],
        cache,
        capture,
    )
    .await?;
    client
        .write_file(sprite, &format!("{ROOT}/system.toml"), String::new())
        .await?;
    let asset = crate::env::asset_for("linux", "x86_64")
        .ok_or_else(|| api_error("managed mise is unavailable for the worker platform"))?;
    run(vec![
        "curl".into(),
        "--fail".into(),
        "--silent".into(),
        "--show-error".into(),
        "--location".into(),
        "--max-time".into(),
        "300".into(),
        "--output".into(),
        MISE.into(),
        asset.url(),
    ])
    .await?;
    client
        .write_file(
            sprite,
            &format!("{ROOT}/mise.sha256"),
            format!("{}  {MISE}\n", asset.sha256),
        )
        .await?;
    run(vec![
        "sha256sum".into(),
        "--check".into(),
        format!("{ROOT}/mise.sha256"),
    ])
    .await?;
    run(vec!["chmod".into(), "0700".into(), MISE.into()]).await?;
    for (path, contents) in &spec.files {
        let path = format!("{ROOT}/env/{path}");
        let parent = std::path::Path::new(&path)
            .parent()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        run(vec!["mkdir".into(), "-p".into(), parent]).await?;
        client.write_file(sprite, &path, contents.clone()).await?;
    }
    for argv in &spec.setup {
        run(argv.clone()).await?;
    }
    run(vec![MISE.into(), "install".into(), "--locked".into()]).await?;
    let result = run(vec![MISE.into(), "env".into(), "--json".into()]).await?;
    let digest = result
        .stdout
        .ok_or_else(|| api_error("mise omitted its tool environment"))?;
    let (bytes, truncated) = cache
        .read_blob_limited(&digest, client.limits.control_bytes(), false)
        .await?;
    if truncated {
        return Err(api_error("tool environment exceeds metadata limit"));
    }
    let mut tools: BTreeMap<String, String> = serde_json::from_slice(&bytes)
        .map_err(|_| api_error("invalid prepared tool environment"))?;
    tools.remove("HOME");
    tools.remove("TMPDIR");
    client
        .write_file(
            sprite,
            &format!("{ROOT}/tool-environment.json"),
            serde_json::to_string(&tools).unwrap(),
        )
        .await
}

pub(super) async fn tool_environment(
    client: &Client,
    sprite: &Sprite,
) -> Result<BTreeMap<String, String>> {
    let response = client
        .request(reqwest::Method::GET, client.url(&sprite.name, "fs/read")?)
        .query(&[
            ("path", format!("{ROOT}/tool-environment.json")),
            ("workingDir", "/".into()),
        ])
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|error| http::http_error("sprites", error))?;
    let tools: BTreeMap<String, String> = http::decode(
        response,
        "sprites",
        client.limits.control_bytes(),
        client.limits,
    )
    .await?;
    let mut env = environment();
    env.extend(tools);
    Ok(env)
}
