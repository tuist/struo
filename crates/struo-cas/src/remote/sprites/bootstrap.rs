use std::time::Duration;

use serde_json::json;

use super::{
    protocol, state::State, HttpCacheRemote, SpritesCacheRemote, PORT, PROVIDER, ROOT, SERVER,
};
use crate::{Error, ErrorKind, HttpCacheAuth, RemoteStore, Result};

pub(super) async fn provision(store: &SpritesCacheRemote) -> Result<HttpCacheRemote> {
    let mut state = State::open(&store.state_root).await?;
    let name = state.record.name.clone();
    let api = &store.api;
    let sprite = if let Some(sprite) = api.find(&name).await? {
        if state.record.id.as_deref() != Some(&sprite.id) {
            return Err(protocol(format!(
                "cache Sprite `{name}` has an unrecognized ID; inspect it before removing local cache state"
            )));
        }
        sprite
    } else {
        tracing::info!(provider = PROVIDER, sprite = %name, "creating retained cache sandbox");
        let sprite = api.create(&name).await?;
        if sprite.name != name || sprite.id.is_empty() {
            return Err(protocol(
                "Sprites create returned a different cache identity",
            ));
        }
        state.record.id = Some(sprite.id.clone());
        state.save()?;
        sprite
    };
    if sprite.name != name || sprite.id.is_empty() {
        return Err(protocol(
            "Sprites lookup returned a different cache identity",
        ));
    }
    let remote = HttpCacheRemote::new(
        &sprite.url,
        None,
        HttpCacheAuth::Bearer(state.record.token.clone()),
    )?;
    let script = format!(
        "{ROOT}/server-{}.py",
        blake3::hash(SERVER.as_bytes()).to_hex()
    );
    let token_file = format!("{ROOT}/credential.json");
    let definition = json!({
        "cmd": "python3",
        "args": [script, token_file, store.capacity.to_string(), PORT.to_string(), format!("{ROOT}/data")],
        "needs": [],
        "http_port": PORT,
    });
    let existing = api.service(&name).await?;
    let matches = existing.as_ref().is_some_and(|service| {
        ["cmd", "args", "needs", "http_port"]
            .iter()
            .all(|field| service.get(field) == definition.get(field))
    });
    if !matches {
        api.install_file(&name, &script, SERVER.as_bytes().to_vec())
            .await?;
        api.install_file(
            &name,
            &token_file,
            serde_json::to_vec(&json!({"token": state.record.token}))
                .map_err(|_| protocol("encoding cache credential"))?,
        )
        .await?;
        api.configure(&name, &definition, existing.is_some())
            .await?;
    }
    // The guest checks a cache-only token. Workers never need the API token.
    if sprite
        .url_settings
        .get("auth")
        .and_then(|value| value.as_str())
        != Some("public")
    {
        api.publish(&name).await?;
    }
    tokio::time::timeout(Duration::from_secs(30), ready(&remote))
        .await
        .map_err(|_| {
            Error::remote(
                PROVIDER,
                "provision",
                ErrorKind::Unavailable,
                "cache service did not become ready",
            )
        })??;
    tracing::debug!(provider = PROVIDER, sprite = %name, "retained cache ready");
    Ok(remote)
}

async fn ready(remote: &HttpCacheRemote) -> Result<()> {
    let key = format!("cas/{}", "0".repeat(64));
    for attempt in 0..8 {
        match remote.head(&key).await {
            Ok(_) => return Ok(()),
            Err(error) if error.kind() == ErrorKind::Unavailable && attempt < 7 => {
                tokio::time::sleep(Duration::from_millis(250 * (attempt + 1))).await;
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!()
}
