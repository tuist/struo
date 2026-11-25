use struo_frontend::{
    load_infrastructure_config, resolve_cache_provider, InfrastructureProviderConfig,
    ResolvedCacheProviderConfig, SpritesProviderConfig,
};
use tempfile::TempDir;

#[test]
fn sprites_provider_can_be_selected_for_execution_and_caching() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(
        tmp.path().join("struo.toml"),
        r#"
[infrastructures.cloud]
kind = "sprites"
cache_max_size_bytes = 4096
read_only = true
[infrastructure.execution]
name = "cloud"
[infrastructure.cache]
name = "cloud"
"#,
    )
    .unwrap();
    let infrastructure = load_infrastructure_config(tmp.path()).unwrap();
    assert_eq!(infrastructure.execution.unwrap().name, "cloud");
    let expected = SpritesProviderConfig {
        cache_max_size_bytes: 4096,
        read_only: true,
    };
    assert_eq!(
        infrastructure.providers["cloud"],
        InfrastructureProviderConfig::Sprites(expected.clone())
    );
    assert_eq!(
        resolve_cache_provider(tmp.path(), &tmp.path().join("missing.toml"), None).unwrap(),
        ResolvedCacheProviderConfig::Sprites(expected)
    );
}

#[test]
fn execution_only_sprites_keeps_defaults_and_does_not_select_remote_cache() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(
        tmp.path().join("struo.toml"),
        "[infrastructures.cloud]\nkind = 'sprites'\n[infrastructure.execution]\nname = 'cloud'",
    )
    .unwrap();
    let infrastructure = load_infrastructure_config(tmp.path()).unwrap();
    assert_eq!(
        infrastructure.providers["cloud"],
        InfrastructureProviderConfig::Sprites(SpritesProviderConfig {
            cache_max_size_bytes: 20 * 1024 * 1024 * 1024,
            read_only: false,
        })
    );
    assert_eq!(
        resolve_cache_provider(tmp.path(), &tmp.path().join("missing.toml"), None).unwrap(),
        ResolvedCacheProviderConfig::Local
    );
}

#[test]
fn sprites_user_selection_preserves_current_and_legacy_config_conventions() {
    for (binding, providers) in [
        ("infrastructure.cache", "infrastructures"),
        ("cache_provider", "cache_providers"),
    ] {
        let tmp = TempDir::new().unwrap();
        let user_path = tmp.path().join("user.toml");
        std::fs::write(
            &user_path,
            format!(
                "[{binding}]\nname = 'cloud'\n[{providers}.cloud]\nkind = 'sprites'\ncache_max_size_bytes = 8192\nread_only = true"
            ),
        )
        .unwrap();
        assert_eq!(
            resolve_cache_provider(tmp.path(), &user_path, None).unwrap(),
            ResolvedCacheProviderConfig::Sprites(SpritesProviderConfig {
                cache_max_size_bytes: 8192,
                read_only: true,
            })
        );
    }
}

#[test]
fn sprites_options_reject_unknown_fields_and_invalid_types() {
    for options in [
        "cache_max_size_byte = 123",
        "cache_max_size_bytes = -1",
        "cache_max_size_bytes = '123'",
        "read_only = 'true'",
        "image = 'custom'",
    ] {
        let tmp = TempDir::new().unwrap();
        let body = format!("[infrastructures.cloud]\nkind = 'sprites'\n{options}");
        std::fs::write(tmp.path().join("struo.toml"), &body).unwrap();
        assert!(load_infrastructure_config(tmp.path()).is_err(), "{options}");
        std::fs::remove_file(tmp.path().join("struo.toml")).unwrap();
        let user_path = tmp.path().join("user.toml");
        std::fs::write(
            &user_path,
            format!("[infrastructure.cache]\nname = 'cloud'\n{body}"),
        )
        .unwrap();
        assert!(
            resolve_cache_provider(tmp.path(), &user_path, None).is_err(),
            "{options}"
        );
    }
}
