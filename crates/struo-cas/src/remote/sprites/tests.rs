use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;

use bytes::Bytes;
use tempfile::TempDir;

use super::*;
use crate::{ActionResult, CacheProvider, Digest, RemoteAccess};

mod mock;

#[tokio::test]
async fn lazy_cache_survives_new_clients_and_a_cold_service_restart() {
    let fixture = mock::Fixture::new().await;
    let credentials = TempDir::new().unwrap();
    let root = credentials.path().join("not-created-yet");
    let remote = fixture.remote(&root);
    assert!(!root.exists());
    assert!(fixture.state.lock().unwrap().calls.is_empty());
    let first_local = TempDir::new().unwrap();
    let local_only = CacheProvider::open_local(first_local.path())
        .put_blob(b"local only")
        .await
        .unwrap();
    let first = CacheProvider::remote(
        first_local.path(),
        Arc::new(remote),
        RemoteAccess::ReadWrite,
    );
    assert_eq!(first.get_blob(&local_only).await.unwrap(), b"local only");
    assert!(!root.exists());
    assert!(fixture.state.lock().unwrap().calls.is_empty());
    let data: Vec<_> = (0..256 * 1024 + 17)
        .map(|i| u8::try_from(i % 251).unwrap())
        .collect();
    let digest = first.put_stream(&data[..]).await.unwrap();
    let action = Digest::of_bytes(b"shared-action");
    let result = ActionResult {
        exit_code: 0,
        stdout: None,
        stderr: None,
        outputs: [("build/result".into(), digest)].into(),
    };
    first.put_action_result(&action, &result).await.unwrap();
    let writes = fixture.state.lock().unwrap().writes();
    fixture.state.lock().unwrap().stop();
    drop(first);

    let second_local = TempDir::new().unwrap();
    let second = CacheProvider::remote(
        second_local.path(),
        Arc::new(fixture.remote(&root)),
        RemoteAccess::ReadWrite,
    );
    let hit = second.lookup_action_result(&action).await.unwrap().unwrap();
    assert_eq!(hit.result, result);
    assert!(hit.blobs_available);
    let output = second_local.path().join("restored");
    second.copy_blob_to_file(&digest, &output).await.unwrap();
    assert_eq!(std::fs::read(output).unwrap(), data);
    assert_eq!(fixture.state.lock().unwrap().writes(), writes);
    for token in [None, Some("api-secret-not-a-cache-token")] {
        let mut request = reqwest::Client::new().get(format!(
            "{}/v1.cas.{}",
            fixture.cache_url(),
            digest.to_hex()
        ));
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        assert_eq!(request.send().await.unwrap().status(), 401);
    }
    let directory = root.join("sprites-cache");
    let scope = std::fs::read_dir(directory)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(
        std::fs::metadata(scope.join("state.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[tokio::test]
async fn a_capacity_change_preserves_recent_objects_within_the_new_budget() {
    let fixture = mock::Fixture::new().await;
    let credentials = TempDir::new().unwrap();
    let remote = fixture.remote(credentials.path());
    let small = format!("cas/{}", "1".repeat(64));
    let large = format!("cas/{}", "2".repeat(64));
    remote
        .put_bytes(&small, Bytes::from_static(b"abc"))
        .await
        .unwrap();
    remote
        .put_bytes(&large, Bytes::from(vec![7; 4097]))
        .await
        .unwrap();
    assert_eq!(remote.head(&small).await.unwrap(), Some(3));
    let resized = SpritesCacheRemote::new(
        credentials.path(),
        4096,
        &fixture.endpoint,
        "api-secret-not-a-cache-token".into(),
    )
    .unwrap();
    assert_eq!(resized.head(&small).await.unwrap(), Some(3));
    assert_eq!(resized.head(&large).await.unwrap(), None);
    let mut body = resized.get(&small).await.unwrap().unwrap();
    let mut bytes = Vec::new();
    tokio::io::AsyncReadExt::read_to_end(&mut body.reader, &mut bytes)
        .await
        .unwrap();
    assert_eq!(bytes, b"abc");
}

#[tokio::test]
async fn concurrent_clients_converge_and_replacements_are_not_adopted() {
    let fixture = mock::Fixture::new().await;
    let credentials = TempDir::new().unwrap();
    let (a, b) = (
        fixture.remote(credentials.path()),
        fixture.remote(credentials.path()),
    );
    let key = format!("cas/{}", "a".repeat(64));
    let (first, second) = tokio::join!(a.head(&key), b.head(&key));
    assert_eq!(first.unwrap(), None);
    assert_eq!(second.unwrap(), None);
    let writes = fixture.state.lock().unwrap().writes();
    fixture.state.lock().unwrap().sprite.as_mut().unwrap()["id"] =
        serde_json::json!("replacement-id");
    let error = fixture
        .remote(credentials.path())
        .head(&key)
        .await
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Protocol);
    assert!(error.to_string().contains("unrecognized ID"));
    assert_eq!(fixture.state.lock().unwrap().writes(), writes);
}

#[tokio::test]
async fn an_ambiguous_create_is_not_repeated_or_adopted() {
    let fixture = mock::Fixture::new().await;
    fixture.state.lock().unwrap().create_status = 503;
    let credentials = TempDir::new().unwrap();
    let remote = fixture.remote(credentials.path());
    let key = format!("cas/{}", "b".repeat(64));
    assert_eq!(
        remote.head(&key).await.unwrap_err().kind(),
        ErrorKind::Unavailable
    );
    assert_eq!(
        remote.head(&key).await.unwrap_err().kind(),
        ErrorKind::Protocol
    );
    assert_eq!(fixture.state.lock().unwrap().writes(), 1);
    assert!(fixture.state.lock().unwrap().definition.is_none());
}

#[tokio::test]
async fn rejected_lookups_are_not_misses_and_do_not_create_resources() {
    let fixture = mock::Fixture::new().await;
    let credentials = TempDir::new().unwrap();
    let remote = fixture.remote(credentials.path());
    for (status, kind) in [
        (401, ErrorKind::Refused),
        (403, ErrorKind::Refused),
        (429, ErrorKind::Unavailable),
        (503, ErrorKind::Unavailable),
    ] {
        fixture.state.lock().unwrap().lookup_status = Some(status);
        let error = remote
            .put_bytes("cas/test", Bytes::from_static(b"not published"))
            .await
            .unwrap_err();
        assert_eq!(error.kind(), kind);
        assert!(!format!("{error:?} {remote:?}").contains("api-secret"));
    }
    assert_eq!(fixture.state.lock().unwrap().writes(), 0);
}

#[test]
fn scopes_credentials_and_rejects_unsafe_configuration_without_side_effects() {
    let root = TempDir::new().unwrap();
    let a = SpritesCacheRemote::new(root.path(), 4096, "https://api.sprites.dev", "first".into())
        .unwrap();
    let b = SpritesCacheRemote::new(
        root.path(),
        4096,
        "https://api.sprites.dev/",
        "first".into(),
    )
    .unwrap();
    let c = SpritesCacheRemote::new(
        root.path(),
        4096,
        "https://api.sprites.dev",
        "second".into(),
    )
    .unwrap();
    assert_eq!(a.state_root, b.state_root);
    assert_ne!(a.state_root, c.state_root);
    for (capacity, url) in [
        (0, "https://api.sprites.dev"),
        (4095, "https://api.sprites.dev"),
        (4096, "http://api.sprites.dev"),
        (4096, "https://api.sprites.dev/?token=secret"),
        (4096, "https://user:secret@api.sprites.dev"),
    ] {
        let error =
            SpritesCacheRemote::new(root.path(), capacity, url, "first".into()).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Misconfigured);
        assert!(!error.to_string().contains("secret"));
    }
    assert!(std::fs::read_dir(root.path()).unwrap().next().is_none());
}

#[test]
fn service_http_contract() {
    let output = std::process::Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/remote/sprites/server_test.py"
        ))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
