use std::path::{Path, PathBuf};

use serde_json::json;
use struo_cas::{CacheProvider, Digest};
use tokio::sync::OnceCell;

use super::super::{
    client::{api_error, Client, Sprite},
    preparation, setup, INPUT_ARCHIVE,
};
use super::{Binding, Service};
use crate::executor::staging::CapturePermit;
use crate::remote::{http, phase_permit};
use crate::{ExecutionPermit, Result};

pub(in crate::remote::sprites) struct Binary {
    pub path: PathBuf,
    pub digest: String,
}

pub(in crate::remote::sprites) async fn binary() -> Result<&'static Binary> {
    static BINARY: OnceCell<Binary> = OnceCell::const_new();
    BINARY
        .get_or_try_init(|| async {
            let path = std::env::var_os("STRUO_WORKER_BINARY")
                .map_or_else(std::env::current_exe, |path| Ok(PathBuf::from(path)))
                .map_err(|source| crate::Error::FileAction {
                    action: "locate_worker_binary",
                    path: "worker executable".into(),
                    source,
                })?;
            let source = path.clone();
            let digest = tokio::task::spawn_blocking(move || {
                use std::io::{Read, Seek};
                let mut file = std::fs::File::open(source)?;
                let mut header = [0; 20];
                file.read_exact(&mut header)?;
                if &header[..6] != b"\x7fELF\x02\x01" || header[18..20] != [62, 0] {
                    return Err(std::io::Error::other(
                        "set STRUO_WORKER_BINARY to a Linux x86_64 Struo executable",
                    ));
                }
                file.rewind()?;
                Digest::of_reader(std::io::BufReader::new(file))
            })
            .await
            .map_err(|_| api_error("worker executable hashing failed"))?
            .map_err(|source| crate::Error::FileAction {
                action: "verify_worker_binary",
                path: "worker executable".into(),
                source,
            })?;
            Ok(Binary {
                path,
                digest: digest.to_hex(),
            })
        })
        .await
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub(in crate::remote::sprites) async fn install(
    client: &Client,
    sprite: &Sprite,
    environment: &str,
    program: &Binary,
    cache: &CacheProvider,
    capture: Option<&CapturePermit>,
    lease: Option<&ExecutionPermit>,
    restart: bool,
) -> Result<Binding> {
    let response = client
        .request(reqwest::Method::GET, client.url(&sprite.name, "")?)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|error| http::http_error("sprites", error))?;
    let value: serde_json::Value = http::decode(
        response,
        "sprites",
        client.limits.control_bytes(),
        client.limits,
    )
    .await?;
    if value["id"] != sprite.provider_id || value["name"] != sprite.name {
        return Err(api_error(
            "worker identity changed before service installation",
        ));
    }
    let url = value["url"]
        .as_str()
        .filter(|url| !url.is_empty())
        .ok_or_else(|| api_error("Sprite metadata omitted its service URL"))?
        .to_string();
    let token =
        blake3::hash(format!("{}{}", uuid::Uuid::now_v7(), uuid::Uuid::now_v7()).as_bytes())
            .to_hex()
            .to_string();
    let binding = Binding {
        url,
        token,
        binary: program.digest.clone(),
    };
    let executable = format!("{}/struo-{}", preparation::ROOT, program.digest);
    let length = tokio::fs::metadata(&program.path)
        .await
        .map_err(|source| crate::Error::FileAction {
            action: "read_worker_binary",
            path: "worker executable".into(),
            source,
        })?
        .len();
    http::check_size(
        length,
        client.limits.max_archive_bytes,
        "sprites",
        "worker executable bytes",
    )?;
    let _phase = phase_permit(lease, length).await?;
    client
        .upload(sprite, &program.path, length, cache, capture)
        .await?;
    setup(
        client,
        sprite,
        &[
            "mv".into(),
            "--".into(),
            INPUT_ARCHIVE.into(),
            executable.clone(),
        ],
        cache,
        capture,
    )
    .await?;
    setup(
        client,
        sprite,
        &["chmod".into(), "0700".into(), executable.clone()],
        cache,
        capture,
    )
    .await?;
    // Bootstrap dependencies belong to the prepared host, never to an action.
    setup(
        client,
        sprite,
        &["sudo".into(), "apt-get".into(), "update".into()],
        cache,
        capture,
    )
    .await?;
    setup(
        client,
        sprite,
        &[
            "sudo".into(),
            "apt-get".into(),
            "satisfy".into(),
            "-y".into(),
            "bubblewrap, libasound2 | libasound2t64".into(),
        ],
        cache,
        capture,
    )
    .await?;
    let options = crate::worker::ServeOptions {
        environment: environment.into(),
        preparation_root: Path::new(preparation::ROOT).into(),
        tool_environment: preparation::tool_environment(client, sprite).await?,
        tool_launcher: vec![
            preparation::MISE.into(),
            "exec".into(),
            "--locked".into(),
            "--".into(),
        ],
        memory_bytes: crate::ResourceLimits::default().memory_bytes,
        capacity_bytes: client.limits.max_staging_bytes,
    };
    let options_path = format!("{}/worker-options.json", preparation::ROOT);
    let token_path = format!("{}/worker-credential.json", preparation::ROOT);
    client
        .write_file(
            sprite,
            &options_path,
            serde_json::to_string(&options).map_err(|_| api_error("encoding worker options"))?,
        )
        .await?;
    client
        .write_file(
            sprite,
            &token_path,
            json!({"token": binding.token}).to_string(),
        )
        .await?;
    let response = client.request(reqwest::Method::PUT, client.url(&sprite.name, "services/struo-worker")?)
        .query(&[("duration", "1")]).json(&json!({
            "cmd": "sudo",
            "args": ["-n", "--preserve-env=HOME", "--", executable, "runtime", "worker", "--private-proc-mounts", "--root", format!("{}/service", preparation::ROOT), "--options", options_path, "--token-file", token_path],
            "needs": [], "http_port": 8080,
        })).timeout(std::time::Duration::from_secs(60)).send().await.map_err(|error| http::http_error("sprites", error))?;
    http::body(
        http::checked(response, "sprites")?,
        "sprites",
        client.limits.control_bytes(),
        client.limits,
    )
    .await?;
    if restart {
        let response = client
            .request(
                reqwest::Method::POST,
                client.url(&sprite.name, "services/struo-worker/restart")?,
            )
            .query(&[("duration", "1")])
            .timeout(std::time::Duration::from_secs(60))
            .send()
            .await
            .map_err(|error| http::http_error("sprites", error))?;
        http::body(
            http::checked(response, "sprites")?,
            "sprites",
            client.limits.control_bytes(),
            client.limits,
        )
        .await?;
    }
    let response = client
        .request(reqwest::Method::PUT, client.url(&sprite.name, "")?)
        .json(&json!({"url_settings":{"auth":"public"}}))
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|error| http::http_error("sprites", error))?;
    http::body(
        http::checked(response, "sprites")?,
        "sprites",
        client.limits.control_bytes(),
        client.limits,
    )
    .await?;
    let service = Service::new(&binding, client.limits)?;
    let startup = tokio::time::Instant::now();
    loop {
        match service.ready(environment).await {
            Ok(()) => break,
            Err(error) if startup.elapsed() >= std::time::Duration::from_secs(60) => {
                return Err(error)
            }
            Err(_) => tokio::time::sleep(std::time::Duration::from_secs(1)).await,
        }
    }
    Ok(binding)
}
