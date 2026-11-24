use crate::{Action, Error, NetworkPolicy, RemoteExecution, Result};

/// The capacity an executor consumes, not its execution-platform identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ResourceDomain {
    Host,
    /// Outstanding client requests. The service owns physical worker admission.
    Remote,
}

/// A resolved execution adapter. Aliases and tenancy remain in `RemoteExecution`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Executor {
    Local,
    Microsandbox,
    E2b,
    Daytona,
    Boat,
    LangSmith,
    Together,
    Northflank,
    Cloudflare,
    Sprites,
    Tensorlake,
    OpenSandbox,
    Vercel,
    Runloop,
    Blaxel,
    Docker,
    /// A Tuist project's Remote Execution API service.
    Tuist,
}

impl Executor {
    pub fn resolve(action: &Action) -> Result<Self> {
        let Action::RunCommand {
            remote: Some(binding),
            network,
            stdout_path,
            stderr_path,
            output_symlink_mode,
            ..
        } = action
        else {
            return Ok(Self::Local);
        };
        let executor = Self::from_binding(binding)?;
        if executor == Self::Blaxel
            && binding
                .properties
                .get("workspace")
                .and_then(serde_json::Value::as_str)
                .is_none_or(|workspace| workspace.trim().is_empty())
        {
            return Err(Error::RemoteProviderConfig {
                provider: binding.provider.clone(),
                message: "Blaxel requires properties.workspace before cache lookup; set STRUO_BLAXEL_WORKSPACE or BL_WORKSPACE for the CLI, or bind the workspace explicitly".into(),
            });
        }
        if executor == Self::Vercel
            && [&binding.account, &binding.project]
                .iter()
                .any(|value| value.as_deref().is_none_or(|value| value.trim().is_empty()))
        {
            return Err(Error::RemoteProviderConfig {
                provider: binding.provider.clone(),
                message: "Vercel requires nonempty account and project in the execution binding before cache lookup; the CLI can bind VERCEL_TEAM_ID and VERCEL_PROJECT_ID".into(),
            });
        }
        if executor != Self::Local {
            if *network == NetworkPolicy::Deny {
                return Err(executor.unsupported("network denial"));
            }
            if executor != Self::Tuist && (stdout_path.is_some() || stderr_path.is_some()) {
                return Err(executor.unsupported("stdout/stderr path redirection"));
            }
            if !output_symlink_mode.is_default() {
                return Err(executor.unsupported("non-default output symlink policy"));
            }
        }
        Ok(executor)
    }

    pub(crate) fn unsupported(self, requirement: &'static str) -> Error {
        Error::UnsupportedExecutorRequirement {
            executor: self.name().to_string(),
            requirement,
        }
    }

    #[allow(clippy::too_many_lines)]
    pub fn from_binding(binding: &RemoteExecution) -> Result<Self> {
        let executor = match binding.executor.as_deref().unwrap_or(&binding.provider) {
            "local" if binding.environment.is_none() => Ok(Self::Local),
            "local" => Err(Error::RemoteProviderConfig {
                provider: binding.provider.clone(),
                message: "local execution cannot supply an image or template; select an executor that provides the requested environment".to_string(),
            }),
            "microsandbox" => Ok(Self::Microsandbox),
            "e2b" => Ok(Self::E2b),
            "daytona" => Ok(Self::Daytona),
            "boat" => Ok(Self::Boat),
            "langsmith" => Ok(Self::LangSmith),
            "together" => Ok(Self::Together),
            "northflank" => Ok(Self::Northflank),
            "cloudflare" => Ok(Self::Cloudflare),
            "sprites" => Ok(Self::Sprites),
            "tensorlake" => Ok(Self::Tensorlake),
            "opensandbox" => Ok(Self::OpenSandbox),
            "vercel" => Ok(Self::Vercel),
            "runloop" => Ok(Self::Runloop),
            "blaxel" => Ok(Self::Blaxel),
            "docker" => Ok(Self::Docker),
            "tuist" => Ok(Self::Tuist),
            adapter => Err(Error::UnsupportedRemoteProvider {
                provider: adapter.to_string(),
            }),
        }?;
        if !matches!(executor, Self::Tuist | Self::Northflank | Self::Vercel)
            && (binding.account.is_some() || binding.project.is_some())
        {
            return Err(executor.unsupported("explicit account/project execution scoping"));
        }
        if executor == Self::LangSmith
            && binding
                .environment
                .as_deref()
                .is_none_or(|snapshot| snapshot.trim().is_empty())
        {
            return Err(Error::RemoteProviderConfig {
                provider: binding.provider.clone(),
                message: "LangSmith requires an explicit prepared snapshot in environment".into(),
            });
        }
        if executor == Self::Northflank
            && (binding
                .environment
                .as_deref()
                .is_none_or(|image| image.trim().is_empty())
                || binding
                    .project
                    .as_deref()
                    .is_none_or(|project| project.trim().is_empty())
                || binding
                    .properties
                    .get("deployment_plan")
                    .and_then(serde_json::Value::as_str)
                    .is_none_or(|plan| plan.trim().is_empty()))
        {
            return Err(Error::RemoteProviderConfig {
                provider: binding.provider.clone(),
                message:
                    "Northflank requires an explicit image, project, and properties.deployment_plan"
                        .into(),
            });
        }
        if executor == Self::Sprites && binding.environment.is_some() {
            return Err(executor.unsupported("custom Sprite images or templates"));
        }
        if executor == Self::Tensorlake
            && (binding
                .environment
                .as_deref()
                .is_none_or(|value| value.trim().is_empty())
                || binding
                    .properties
                    .get("namespace")
                    .and_then(serde_json::Value::as_str)
                    .is_none_or(|value| value.trim().is_empty())
                || binding
                    .properties
                    .get("environment_kind")
                    .is_some_and(|value| !matches!(value.as_str(), Some("image" | "snapshot"))))
        {
            return Err(Error::RemoteProviderConfig {
                provider: binding.provider.clone(),
                message: "Tensorlake requires an explicit environment, properties.namespace, and environment_kind of image or snapshot".into(),
            });
        }
        if executor == Self::Docker
            && !binding.environment.as_deref().is_some_and(|image| {
                image.strip_prefix("sha256:").is_some_and(|digest| {
                    digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
            })
        {
            return Err(Error::RemoteProviderConfig {
                provider: binding.provider.clone(),
                message: "Docker requires an explicit immutable sha256:<64 hex digits> image ID in environment; warm the image first, then resolve it with: docker image inspect --format '{{.Id}}' IMAGE".into(),
            });
        }
        if executor == Self::Tuist
            && binding
                .environment
                .as_deref()
                .is_none_or(|image| image.trim().is_empty())
        {
            return Err(Error::RemoteProviderConfig {
                provider: binding.provider.clone(),
                message: "Tuist remote execution has no worker image; Struo binds the image the project's execution service advertises when `struo build --remote` starts, or set `execution_image` on a `kind = \"tuist\"` infrastructure in the workspace struo.toml to pin one, such as sha-<commit>".into(),
            });
        }
        Self::validate_default_environment(binding, executor, |name| {
            std::env::var_os(name).is_some_and(|value| !value.is_empty())
        })?;
        Ok(executor)
    }

    fn validate_default_environment(
        binding: &RemoteExecution,
        executor: Self,
        overridden: impl Fn(&str) -> bool,
    ) -> Result<()> {
        let variable = match executor {
            Self::E2b => Some("STRUO_E2B_TEMPLATE"),
            Self::Microsandbox => Some("STRUO_MICROSANDBOX_IMAGE"),
            Self::Local
            | Self::Daytona
            | Self::Boat
            | Self::LangSmith
            | Self::Together
            | Self::Northflank
            | Self::Cloudflare
            | Self::Sprites
            | Self::Tensorlake
            | Self::OpenSandbox
            | Self::Vercel
            | Self::Runloop
            | Self::Blaxel
            | Self::Docker
            | Self::Tuist => None,
        };
        if binding.environment.is_none() && variable.is_some_and(overridden) {
            return Err(Error::RemoteProviderConfig {
                provider: binding.provider.clone(),
                message: "declare an explicit environment in the execution binding when overriding the provider default".into(),
            });
        }
        Ok(())
    }

    pub fn resource_domain(self) -> ResourceDomain {
        match self {
            Self::Local | Self::Microsandbox | Self::Docker => ResourceDomain::Host,
            Self::E2b
            | Self::Daytona
            | Self::Boat
            | Self::LangSmith
            | Self::Together
            | Self::Northflank
            | Self::Cloudflare
            | Self::Sprites
            | Self::Tensorlake
            | Self::OpenSandbox
            | Self::Vercel
            | Self::Runloop
            | Self::Blaxel
            | Self::Tuist => ResourceDomain::Remote,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Microsandbox => "microsandbox",
            Self::E2b => "e2b",
            Self::Daytona => "daytona",
            Self::Boat => "boat",
            Self::LangSmith => "langsmith",
            Self::Together => "together",
            Self::Northflank => "northflank",
            Self::Cloudflare => "cloudflare",
            Self::Sprites => "sprites",
            Self::Tensorlake => "tensorlake",
            Self::OpenSandbox => "opensandbox",
            Self::Vercel => "vercel",
            Self::Runloop => "runloop",
            Self::Blaxel => "blaxel",
            Self::Docker => "docker",
            Self::Tuist => "tuist",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapter_resolution_uses_the_binding_not_the_alias() {
        let mut binding = RemoteExecution::provider("test-fleet");
        binding.executor = Some("e2b".to_string());
        assert_eq!(Executor::from_binding(&binding).unwrap(), Executor::E2b);
    }

    #[test]
    fn virtual_machines_share_the_host_domain() {
        assert_eq!(Executor::Local.resource_domain(), ResourceDomain::Host);
        assert_eq!(
            Executor::Microsandbox.resource_domain(),
            ResourceDomain::Host
        );
        assert_eq!(Executor::E2b.resource_domain(), ResourceDomain::Remote);
        assert_eq!(Executor::Daytona.resource_domain(), ResourceDomain::Remote);
        assert_eq!(Executor::Boat.resource_domain(), ResourceDomain::Remote);
        assert_eq!(Executor::Vercel.resource_domain(), ResourceDomain::Remote);
        let mut binding = RemoteExecution::provider("vercel");
        binding.account = Some("team-a".into());
        binding.project = Some("project-a".into());
        assert_eq!(Executor::from_binding(&binding).unwrap(), Executor::Vercel);
        assert_eq!(Executor::Together.resource_domain(), ResourceDomain::Remote);
        assert_eq!(
            Executor::Cloudflare.resource_domain(),
            ResourceDomain::Remote
        );
        assert_eq!(
            Executor::OpenSandbox.resource_domain(),
            ResourceDomain::Remote
        );
        assert_eq!(Executor::Runloop.resource_domain(), ResourceDomain::Remote);
    }

    #[test]
    fn unenforced_tenant_requirements_fail_closed() {
        for adapter in [
            "local",
            "microsandbox",
            "e2b",
            "daytona",
            "boat",
            "together",
            "cloudflare",
            "opensandbox",
            "runloop",
        ] {
            for project in [false, true] {
                let mut binding = RemoteExecution::provider(adapter);
                if project {
                    binding.project = Some("required-project".into());
                } else {
                    binding.account = Some("required-account".into());
                }
                assert!(matches!(
                    Executor::from_binding(&binding),
                    Err(Error::UnsupportedExecutorRequirement { .. })
                ));
            }
        }
    }

    #[test]
    fn implicit_provider_overrides_cannot_reuse_an_unbound_environment() {
        let mut binding = RemoteExecution::provider("e2b");
        assert!(Executor::validate_default_environment(&binding, Executor::E2b, |_| true).is_err());
        binding.environment = Some("bound-template".into());
        assert!(Executor::validate_default_environment(&binding, Executor::E2b, |_| true).is_ok());
        let local = RemoteExecution::provider("local");
        assert!(
            Executor::validate_default_environment(&local, Executor::Local, |_| panic!(
                "native execution has no image"
            ))
            .is_ok()
        );
    }

    #[test]
    fn tuist_execution_requires_a_pinned_image() {
        let mut binding = RemoteExecution::provider("tuist");
        binding.account = Some("acme".into());
        binding.project = Some("app".into());
        assert!(matches!(
            Executor::from_binding(&binding),
            Err(Error::RemoteProviderConfig { .. })
        ));
        binding.environment = Some("sha-0123abc".into());
        assert_eq!(Executor::from_binding(&binding).unwrap(), Executor::Tuist);
    }

    #[test]
    fn unknown_adapters_fail_closed() {
        for name in ["unknown", "kura", ""] {
            assert!(matches!(
                Executor::from_binding(&RemoteExecution::provider(name)),
                Err(Error::UnsupportedRemoteProvider { .. })
            ));
        }
    }

    #[test]
    fn northflank_requires_scoped_configuration_and_roundtrips_at_runtime() {
        let mut binding = RemoteExecution::provider("tests");
        binding.executor = Some("northflank".into());
        assert!(Executor::from_binding(&binding).is_err());
        binding.environment = Some("node:22".into());
        binding.project = Some("project".into());
        assert!(Executor::from_binding(&binding).is_err());
        binding.account = Some("team".into());
        binding
            .properties
            .insert("deployment_plan".into(), "nf-compute-200".into());
        let encoded = serde_json::to_vec(&binding).unwrap();
        let decoded: RemoteExecution = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded, binding);
        assert_eq!(
            Executor::from_binding(&decoded).unwrap(),
            Executor::Northflank
        );
        assert_eq!(
            Executor::Northflank.resource_domain(),
            ResourceDomain::Remote
        );
    }
}
