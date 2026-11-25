use std::collections::BTreeMap;

use serde::Deserialize;

use crate::error::{Error, Result};

pub const DEFAULT_TUIST_URL: &str = "https://tuist.dev";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum CacheProviderConfig {
    #[default]
    Local,
    Named(NamedCacheProviderConfig),
    Tuist(TuistCacheProviderConfig),
    ObjectStore(ObjectStoreCacheProviderConfig),
    HttpCache(HttpCacheProviderConfig),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InfrastructureConfig {
    pub cache: Option<CacheProviderConfig>,
    pub execution: Option<ExecutionProviderConfig>,
    pub artifacts: Option<NamedCacheProviderConfig>,
    pub publish_on_build: Vec<String>,
    pub providers: BTreeMap<String, InfrastructureProviderConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionProviderConfig {
    pub name: String,
    pub account: Option<String>,
    pub project: Option<String>,
    pub properties: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedCacheProviderConfig {
    pub name: String,
    pub account: Option<String>,
    pub project: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TuistCacheProviderConfig {
    pub url: String,
    pub account: Option<String>,
    pub project: Option<String>,
    pub oauth_client_id: Option<String>,
    /// Worker image remote execution pins, such as `sha-<commit>`.
    pub execution_image: Option<String>,
}

/// A bucket, container, or directory that holds the shared cache.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectStoreCacheProviderConfig {
    /// `s3://bucket/prefix`, `gs://bucket/prefix`, `az://container/prefix`,
    /// or `file:///directory`.
    pub url: String,
    /// Store settings such as `endpoint` or `region`. Credentials belong in
    /// the environment, not here.
    #[serde(default)]
    pub options: BTreeMap<String, String>,
    /// Read the shared cache without publishing to it.
    #[serde(default)]
    pub read_only: bool,
}

/// An HTTP cache that stores values with `GET`, `HEAD`, and `PUT` on a key
/// path, such as a `WebDAV` build cache.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HttpCacheProviderConfig {
    /// Base URL the keys are appended to.
    pub url: String,
    /// Path prepended to every key, such as a tenant or cache name.
    #[serde(default)]
    pub key_prefix: Option<String>,
    /// Environment variable holding a bearer token.
    #[serde(default)]
    pub token_env: Option<String>,
    /// Environment variable holding the basic authentication user name.
    #[serde(default)]
    pub username_env: Option<String>,
    /// Environment variable holding the basic authentication password.
    #[serde(default)]
    pub password_env: Option<String>,
    /// Read the shared cache without publishing to it.
    #[serde(default)]
    pub read_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MicrosandboxExecutionProviderConfig {
    pub image: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct E2bExecutionProviderConfig {
    pub template: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaytonaExecutionProviderConfig {
    pub image: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoatExecutionProviderConfig {
    pub snapshot: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LangSmithExecutionProviderConfig {
    pub snapshot: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VercelExecutionProviderConfig {
    pub runtime: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TogetherExecutionProviderConfig {
    pub snapshot: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NorthflankExecutionProviderConfig {
    pub image: String,
    pub deployment_plan: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudflareExecutionProviderConfig {
    pub image: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenSandboxExecutionProviderConfig {
    pub image: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunloopExecutionProviderConfig {
    pub blueprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlaxelExecutionProviderConfig {
    pub image: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockerExecutionProviderConfig {
    pub image: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SpritesProviderConfig {
    pub cache_max_size_bytes: u64,
    pub read_only: bool,
}

impl Default for SpritesProviderConfig {
    fn default() -> Self {
        Self {
            cache_max_size_bytes: 20 * 1024 * 1024 * 1024,
            read_only: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InfrastructureProviderConfig {
    Local,
    Microsandbox(MicrosandboxExecutionProviderConfig),
    E2b(E2bExecutionProviderConfig),
    Daytona(DaytonaExecutionProviderConfig),
    Boat(BoatExecutionProviderConfig),
    LangSmith(LangSmithExecutionProviderConfig),
    Vercel(VercelExecutionProviderConfig),
    Together(TogetherExecutionProviderConfig),
    Northflank(NorthflankExecutionProviderConfig),
    Cloudflare(CloudflareExecutionProviderConfig),
    Sprites(SpritesProviderConfig),
    Tensorlake {
        environment: String,
        environment_kind: String,
        namespace: String,
    },
    OpenSandbox(OpenSandboxExecutionProviderConfig),
    Runloop(RunloopExecutionProviderConfig),
    Blaxel(BlaxelExecutionProviderConfig),
    Docker(DockerExecutionProviderConfig),
    Tuist(TuistCacheProviderConfig),
    ObjectStore(ObjectStoreCacheProviderConfig),
    HttpCache(HttpCacheProviderConfig),
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct InfrastructureToml {
    pub cache: Option<CacheProviderToml>,
    pub execution: Option<ExecutionProviderToml>,
    pub artifacts: Option<NamedCacheProviderToml>,
    pub publish_on_build: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExecutionProviderToml {
    #[serde(alias = "provider")]
    name: String,
    account: Option<String>,
    project: Option<String>,
    #[serde(default)]
    properties: BTreeMap<String, serde_json::Value>,
}

impl ExecutionProviderToml {
    pub(crate) fn into_config(
        self,
        display_name: &str,
        section_name: &str,
    ) -> Result<ExecutionProviderConfig> {
        let binding = NamedCacheProviderToml {
            name: self.name,
            account: self.account,
            project: self.project,
        }
        .into_config(display_name, section_name)?;
        Ok(ExecutionProviderConfig {
            name: binding.name,
            account: binding.account,
            project: binding.project,
            properties: self.properties,
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(crate) enum CacheProviderToml {
    Named(NamedCacheProviderToml),
    Direct(DirectCacheProviderToml),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NamedCacheProviderToml {
    #[serde(alias = "provider")]
    name: String,
    account: Option<String>,
    project: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum DirectCacheProviderToml {
    Local,
    Tuist {
        url: Option<String>,
        account: Option<String>,
        project: Option<String>,
        oauth_client_id: Option<String>,
    },
    ObjectStore(ObjectStoreCacheProviderConfig),
    HttpCache(HttpCacheProviderConfig),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum InfrastructureProviderToml {
    Local,
    Microsandbox {
        image: String,
    },
    E2b {
        template: String,
    },
    Daytona {
        image: String,
    },
    Boat {
        snapshot: String,
    },
    #[serde(rename = "langsmith")]
    LangSmith {
        snapshot: String,
    },
    Vercel {
        runtime: String,
    },
    Together {
        snapshot: String,
    },
    Northflank {
        image: String,
        deployment_plan: String,
    },
    Cloudflare {
        image: String,
    },
    Sprites(SpritesProviderConfig),
    Tensorlake {
        image: Option<String>,
        snapshot: Option<String>,
        namespace: String,
    },
    #[serde(rename = "opensandbox")]
    OpenSandbox {
        image: String,
    },
    Runloop {
        blueprint: String,
    },
    Blaxel {
        image: String,
    },
    Docker {
        image: String,
    },
    Tuist {
        url: Option<String>,
        account: Option<String>,
        project: Option<String>,
        oauth_client_id: Option<String>,
        execution_image: Option<String>,
    },
    ObjectStore(ObjectStoreCacheProviderConfig),
    HttpCache(HttpCacheProviderConfig),
}

impl CacheProviderToml {
    pub(crate) fn into_config(self, display_name: &str) -> Result<CacheProviderConfig> {
        match self {
            Self::Named(named) => Ok(CacheProviderConfig::Named(
                named.into_config(display_name, "cache_provider")?,
            )),
            Self::Direct(DirectCacheProviderToml::Local) => Ok(CacheProviderConfig::Local),
            Self::Direct(DirectCacheProviderToml::Tuist {
                url,
                account,
                project,
                oauth_client_id,
            }) => {
                let url = non_empty(url).unwrap_or_else(|| DEFAULT_TUIST_URL.to_string());
                Ok(CacheProviderConfig::Tuist(TuistCacheProviderConfig {
                    url,
                    account: non_empty(account),
                    project: non_empty(project),
                    oauth_client_id: non_empty(oauth_client_id),
                    execution_image: None,
                }))
            }
            Self::Direct(DirectCacheProviderToml::ObjectStore(config)) => {
                Ok(CacheProviderConfig::ObjectStore(config))
            }
            Self::Direct(DirectCacheProviderToml::HttpCache(config)) => {
                Ok(CacheProviderConfig::HttpCache(config))
            }
        }
    }
}

impl NamedCacheProviderToml {
    pub(crate) fn into_config(
        self,
        display_name: &str,
        section_name: &str,
    ) -> Result<NamedCacheProviderConfig> {
        let name = self.name.trim().to_string();
        if name.is_empty() {
            return Err(Error::Eval {
                path: display_name.to_string(),
                message: format!("{section_name} provider must not be empty"),
            });
        }
        Ok(NamedCacheProviderConfig {
            name,
            account: non_empty(self.account),
            project: non_empty(self.project),
        })
    }
}

impl InfrastructureProviderToml {
    #[allow(clippy::too_many_lines)]
    pub(crate) fn into_config(
        self,
        display_name: &str,
        provider_name: &str,
    ) -> Result<InfrastructureProviderConfig> {
        Ok(match self {
            Self::Local => InfrastructureProviderConfig::Local,
            Self::Sprites(config) => InfrastructureProviderConfig::Sprites(config),
            Self::Tensorlake {
                image,
                snapshot,
                namespace,
            } => {
                let (environment, environment_kind) = match (image, snapshot) {
                    (Some(image), None) if !image.trim().is_empty() => (image, "image"),
                    (None, Some(snapshot)) if !snapshot.trim().is_empty() => (snapshot, "snapshot"),
                    _ => return Err(Error::Eval {
                        path: display_name.into(),
                        message: format!("tensorlake infrastructure `{provider_name}` requires exactly one nonempty image or snapshot"),
                    }),
                };
                if namespace.trim().is_empty() {
                    return Err(Error::Eval {
                        path: display_name.into(),
                        message: format!("tensorlake infrastructure `{provider_name}` namespace must not be empty"),
                    });
                }
                InfrastructureProviderConfig::Tensorlake {
                    environment: environment.trim().into(),
                    environment_kind: environment_kind.into(),
                    namespace: namespace.trim().into(),
                }
            }
            Self::Microsandbox { image } => {
                let image = image.trim().to_string();
                if image.is_empty() {
                    return Err(Error::Eval {
                        path: display_name.to_string(),
                        message: format!(
                            "microsandbox infrastructure `{provider_name}` image must not be empty"
                        ),
                    });
                }
                InfrastructureProviderConfig::Microsandbox(MicrosandboxExecutionProviderConfig {
                    image,
                })
            }
            Self::E2b { template } => {
                let template =
                    required_value(display_name, provider_name, "e2b", "template", &template)?;
                InfrastructureProviderConfig::E2b(E2bExecutionProviderConfig { template })
            }
            Self::Daytona { image } => {
                let image =
                    required_value(display_name, provider_name, "daytona", "image", &image)?;
                InfrastructureProviderConfig::Daytona(DaytonaExecutionProviderConfig { image })
            }
            Self::Boat { snapshot } => {
                let snapshot =
                    required_value(display_name, provider_name, "boat", "snapshot", &snapshot)?;
                InfrastructureProviderConfig::Boat(BoatExecutionProviderConfig { snapshot })
            }
            Self::LangSmith { snapshot } => {
                let snapshot = required_value(
                    display_name,
                    provider_name,
                    "langsmith",
                    "snapshot",
                    &snapshot,
                )?;
                InfrastructureProviderConfig::LangSmith(LangSmithExecutionProviderConfig {
                    snapshot,
                })
            }
            Self::Vercel { runtime } => {
                let runtime =
                    required_value(display_name, provider_name, "vercel", "runtime", &runtime)?;
                InfrastructureProviderConfig::Vercel(VercelExecutionProviderConfig { runtime })
            }
            Self::Together { snapshot } => {
                let snapshot = required_value(
                    display_name,
                    provider_name,
                    "together",
                    "snapshot",
                    &snapshot,
                )?;
                InfrastructureProviderConfig::Together(TogetherExecutionProviderConfig { snapshot })
            }
            Self::Northflank {
                image,
                deployment_plan,
            } => {
                let image =
                    required_value(display_name, provider_name, "northflank", "image", &image)?;
                let deployment_plan = required_value(
                    display_name,
                    provider_name,
                    "northflank",
                    "deployment_plan",
                    &deployment_plan,
                )?;
                InfrastructureProviderConfig::Northflank(NorthflankExecutionProviderConfig {
                    image,
                    deployment_plan,
                })
            }
            Self::Cloudflare { image } => {
                let image =
                    required_value(display_name, provider_name, "cloudflare", "image", &image)?;
                InfrastructureProviderConfig::Cloudflare(CloudflareExecutionProviderConfig {
                    image,
                })
            }
            Self::Runloop { blueprint } => {
                let blueprint = required_value(
                    display_name,
                    provider_name,
                    "runloop",
                    "blueprint",
                    &blueprint,
                )?;
                InfrastructureProviderConfig::Runloop(RunloopExecutionProviderConfig { blueprint })
            }
            Self::Blaxel { image } => {
                let image = required_value(display_name, provider_name, "blaxel", "image", &image)?;
                InfrastructureProviderConfig::Blaxel(BlaxelExecutionProviderConfig { image })
            }
            Self::Docker { image } => {
                let image = required_value(display_name, provider_name, "docker", "image", &image)?;
                InfrastructureProviderConfig::Docker(DockerExecutionProviderConfig { image })
            }
            Self::OpenSandbox { image } => {
                let image =
                    required_value(display_name, provider_name, "opensandbox", "image", &image)?;
                InfrastructureProviderConfig::OpenSandbox(OpenSandboxExecutionProviderConfig {
                    image,
                })
            }
            Self::Tuist {
                url,
                account,
                project,
                oauth_client_id,
                execution_image,
            } => InfrastructureProviderConfig::Tuist(TuistCacheProviderConfig {
                url: non_empty(url).unwrap_or_else(|| DEFAULT_TUIST_URL.to_string()),
                account: non_empty(account),
                project: non_empty(project),
                oauth_client_id: non_empty(oauth_client_id),
                execution_image: non_empty(execution_image),
            }),
            Self::ObjectStore(config) => InfrastructureProviderConfig::ObjectStore(config),
            Self::HttpCache(config) => InfrastructureProviderConfig::HttpCache(config),
        })
    }
}

fn required_value(
    display_name: &str,
    provider_name: &str,
    kind: &str,
    field: &str,
    value: &str,
) -> Result<String> {
    let value = value.trim().to_string();
    if value.is_empty() {
        return Err(Error::Eval {
            path: display_name.to_string(),
            message: format!("{kind} infrastructure `{provider_name}` {field} must not be empty"),
        });
    }
    Ok(value)
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloudflare_requires_an_explicit_nonempty_image() {
        assert!(
            InfrastructureProviderToml::Cloudflare { image: "  ".into() }
                .into_config("struo.toml", "remote_tests")
                .is_err()
        );
        let config = InfrastructureProviderToml::Cloudflare {
            image: " pinned-v1 ".into(),
        }
        .into_config("struo.toml", "remote_tests")
        .unwrap();
        assert_eq!(
            config,
            InfrastructureProviderConfig::Cloudflare(CloudflareExecutionProviderConfig {
                image: "pinned-v1".into()
            })
        );
    }

    #[test]
    fn runloop_requires_a_nonempty_blueprint() {
        for value in ["", "  "] {
            assert!(InfrastructureProviderToml::Runloop {
                blueprint: value.into()
            }
            .into_config("struo.toml", "tests")
            .is_err());
        }
        assert_eq!(
            InfrastructureProviderToml::Runloop {
                blueprint: " bpt_node ".into()
            }
            .into_config("struo.toml", "tests")
            .unwrap(),
            InfrastructureProviderConfig::Runloop(RunloopExecutionProviderConfig {
                blueprint: "bpt_node".into()
            })
        );
    }

    #[test]
    fn tuist_config_defaults_url() {
        let config = CacheProviderToml::Direct(DirectCacheProviderToml::Tuist {
            url: None,
            account: Some("acme".to_string()),
            project: Some("app".to_string()),
            oauth_client_id: None,
        })
        .into_config("struo.toml")
        .unwrap();

        let CacheProviderConfig::Tuist(config) = config else {
            panic!("expected tuist config");
        };
        assert_eq!(config.url, DEFAULT_TUIST_URL);
        assert_eq!(config.oauth_client_id, None);
        assert_eq!(config.account.as_deref(), Some("acme"));
        assert_eq!(config.project.as_deref(), Some("app"));
    }

    #[test]
    fn named_config_keeps_scope_in_workspace() {
        let config = CacheProviderToml::Named(NamedCacheProviderToml {
            name: "tuist".to_string(),
            account: Some("acme".to_string()),
            project: Some("app".to_string()),
        })
        .into_config("struo.toml")
        .unwrap();

        assert_eq!(
            config,
            CacheProviderConfig::Named(NamedCacheProviderConfig {
                name: "tuist".to_string(),
                account: Some("acme".to_string()),
                project: Some("app".to_string()),
            })
        );
    }
}
