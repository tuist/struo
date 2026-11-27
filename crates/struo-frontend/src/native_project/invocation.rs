use std::path::{Path, PathBuf};

use crate::{error::Result, Target, TOML_BUILD_FILE_NAME};

use super::{native_project_schemas_for_workspace_with_target_kinds, synthesized_workspace_seeds};

#[derive(Debug, Clone)]
pub struct NativeInvocation {
    pub workspace: PathBuf,
    pub package: String,
    pub targets: Vec<Target>,
    pub local_targets: Vec<Target>,
    /// Seeds of enclosing projects of the same family, and of their nested
    /// projects, that the graph loads only when a loaded target needs one.
    pub deferred_targets: Vec<Target>,
}

/// Detect an invocation-local native project without discovering unrelated
/// projects elsewhere in its repository. Explicit Struo manifests take priority.
pub fn native_invocation(directory: &Path) -> Result<Option<NativeInvocation>> {
    let directory = std::fs::canonicalize(directory).unwrap_or_else(|_| directory.to_path_buf());
    if directory.join(TOML_BUILD_FILE_NAME).exists() {
        return Ok(None);
    }
    let kinds = crate::target_kind_schemas_for_workspace(&directory)?;
    let schemas = native_project_schemas_for_workspace_with_target_kinds(&directory, &kinds)?;
    let boundary = crate::workspace::load_workspace_scan(&directory)?;
    let shallow = schemas
        .iter()
        .filter(|schema| !schema.workspace_markers.is_empty())
        .cloned()
        .map(|mut schema| {
            schema.max_depth = 1;
            schema
        })
        .collect::<Vec<_>>();
    let direct = synthesized_workspace_seeds(&directory, &shallow, &boundary)?
        .into_iter()
        .filter(|(_, target)| target.package.is_empty())
        .collect::<Vec<_>>();
    if direct.len() != 1 {
        return Ok(None);
    }
    let matched = &direct[0].0;
    let schema = schemas
        .iter()
        .find(|schema| schema.name == matched.native_project)
        .unwrap();
    let workspace = directory
        .ancestors()
        .find(|ancestor| {
            schema
                .workspace_markers
                .iter()
                .any(|marker| ancestor.join(marker).exists())
        })
        .unwrap_or(&directory)
        .to_path_buf();
    if directory
        .ancestors()
        .take_while(|ancestor| ancestor.starts_with(&workspace))
        .any(|ancestor| ancestor.join(TOML_BUILD_FILE_NAME).exists())
    {
        return Ok(None);
    }
    let package = directory
        .strip_prefix(&workspace)
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/");
    let mut targets = scoped_seeds(&workspace, &directory, schema, &boundary)?;
    let deferred_targets = if schema.enclosing_on_demand {
        let shallow_schema = shallow
            .iter()
            .find(|candidate| candidate.name == schema.name)
            .unwrap();
        enclosing_seeds(
            &workspace,
            &directory,
            shallow_schema,
            schema,
            &boundary,
            &targets,
        )?
    } else {
        Vec::new()
    };
    targets.sort_by_key(Target::id);
    let local_schemas = schemas
        .into_iter()
        .map(|mut schema| {
            schema.max_depth = 1;
            schema
        })
        .collect::<Vec<_>>();
    let mut local_targets = synthesized_workspace_seeds(&directory, &local_schemas, &boundary)?
        .into_iter()
        .filter(|(_, target)| target.package.is_empty())
        .map(|(_, mut target)| {
            target.package.clone_from(&package);
            target
        })
        .collect::<Vec<_>>();
    local_targets.sort_by_key(Target::id);
    Ok(Some(NativeInvocation {
        workspace,
        package,
        targets,
        local_targets,
        deferred_targets,
    }))
}

/// Seeds of the outermost enclosing project of `schema`'s family and of its
/// nested projects, other than the `local` ones.
fn enclosing_seeds(
    workspace: &Path,
    directory: &Path,
    shallow_schema: &super::NativeProjectSchema,
    schema: &super::NativeProjectSchema,
    boundary: &crate::workspace::WorkspaceScan,
    local: &[Target],
) -> Result<Vec<Target>> {
    let mut outermost = None;
    for ancestor in directory.ancestors().skip(1) {
        if !ancestor.starts_with(workspace) {
            break;
        }
        if synthesized_workspace_seeds(ancestor, std::slice::from_ref(shallow_schema), boundary)?
            .iter()
            .any(|(_, target)| target.package.is_empty())
        {
            outermost = Some(ancestor);
        }
    }
    let Some(scope) = outermost else {
        return Ok(Vec::new());
    };
    let local = local
        .iter()
        .map(Target::id)
        .collect::<std::collections::BTreeSet<_>>();
    let mut seeds = scoped_seeds(workspace, scope, schema, boundary)?
        .into_iter()
        .filter(|target| !local.contains(&target.id()))
        .collect::<Vec<_>>();
    seeds.sort_by_key(Target::id);
    Ok(seeds)
}

fn scoped_seeds(
    workspace: &Path,
    scope: &Path,
    schema: &super::NativeProjectSchema,
    boundary: &crate::workspace::WorkspaceScan,
) -> Result<Vec<Target>> {
    let package = scope
        .strip_prefix(workspace)
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/");
    Ok(
        synthesized_workspace_seeds(scope, std::slice::from_ref(schema), boundary)?
            .into_iter()
            .map(|(_, mut target)| {
                target.package = match (package.is_empty(), target.package.is_empty()) {
                    (true, _) => target.package,
                    (false, true) => package.clone(),
                    (false, false) => format!("{package}/{}", target.package),
                };
                target
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes_nested_native_projects_without_evaluating_manifests() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join(".git")).unwrap();
        std::fs::create_dir_all(temp.path().join("server/native")).unwrap();
        std::fs::write(
            temp.path().join("server/mix.exs"),
            "raise \"not evaluated\"",
        )
        .unwrap();
        std::fs::write(temp.path().join("server/native/Package.swift"), "invalid").unwrap();
        std::fs::write(temp.path().join("Package.swift"), "invalid").unwrap();
        let invocation = native_invocation(&temp.path().join("server"))
            .unwrap()
            .unwrap();
        assert_eq!(
            invocation.workspace,
            std::fs::canonicalize(temp.path()).unwrap()
        );
        assert_eq!(invocation.package, "server");
        assert_eq!(invocation.targets.len(), 1);
        assert_eq!(invocation.targets[0].id(), "server/mix");
    }

    #[test]
    fn nested_projects_defer_their_enclosing_projects() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join(".git")).unwrap();
        for project in [
            "app",
            "app/apps/greeter",
            "app/apps/helper",
            "app/deps/vendored",
        ] {
            std::fs::create_dir_all(temp.path().join(project)).unwrap();
            std::fs::write(
                temp.path().join(project).join("mix.exs"),
                "raise \"not evaluated\"",
            )
            .unwrap();
        }
        let invocation = native_invocation(&temp.path().join("app/apps/greeter"))
            .unwrap()
            .unwrap();
        let ids = |targets: &[Target]| targets.iter().map(Target::id).collect::<Vec<_>>();
        assert_eq!(ids(&invocation.targets), ["app/apps/greeter/mix"]);
        assert_eq!(
            ids(&invocation.deferred_targets),
            ["app/apps/helper/mix", "app/mix"]
        );
    }

    #[test]
    fn does_not_select_a_child_project_from_a_polyglot_repository_root() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join(".git")).unwrap();
        std::fs::create_dir(temp.path().join("server")).unwrap();
        std::fs::write(
            temp.path().join("server/mix.exs"),
            "raise \"not evaluated\"",
        )
        .unwrap();
        assert!(native_invocation(temp.path()).unwrap().is_none());
        std::fs::write(temp.path().join("Package.swift"), "invalid").unwrap();
        assert!(native_invocation(temp.path()).unwrap().is_none());
    }

    #[test]
    fn supports_repository_marker_files_and_standalone_projects() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("mix.exs"), "raise \"not evaluated\"").unwrap();
        let invocation = native_invocation(temp.path()).unwrap().unwrap();
        assert_eq!(invocation.package, "");
        assert_eq!(invocation.targets[0].id(), "mix");
        std::fs::write(temp.path().join(".git"), "gitdir: elsewhere").unwrap();
        assert!(native_invocation(temp.path()).unwrap().is_some());
    }

    #[test]
    fn intermediate_explicit_manifests_keep_their_workspace_boundary() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join(".git")).unwrap();
        std::fs::create_dir_all(temp.path().join("sub/server")).unwrap();
        std::fs::write(temp.path().join("sub/struo.toml"), "").unwrap();
        std::fs::write(
            temp.path().join("sub/server/mix.exs"),
            "raise \"not evaluated\"",
        )
        .unwrap();
        assert!(native_invocation(&temp.path().join("sub/server"))
            .unwrap()
            .is_none());
    }

    #[test]
    fn explicit_manifests_keep_their_workspace_boundary() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("mix.exs"), "raise \"not evaluated\"").unwrap();
        std::fs::write(temp.path().join("struo.toml"), "").unwrap();
        assert!(native_invocation(temp.path()).unwrap().is_none());
    }
}
