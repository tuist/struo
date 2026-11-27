use std::collections::BTreeMap;

use struo_frontend::{Capability, TargetLabel};
use tempfile::TempDir;

use super::*;
use crate::commands::query::test_plan::plan;

#[test]
fn deleted_source_still_matches_its_declared_pattern() {
    let workspace = TempDir::new().unwrap();
    let graph = vec![
        target("lib", "library", &["src/**/*.rs"], &[], false),
        target("tests", "test", &["tests/**/*.rs"], &["lib"], true),
    ];

    let report = selection_report(
        workspace.path(),
        &graph,
        &["src/removed/module.rs".to_string()],
    )
    .unwrap();

    assert_eq!(report.tests.len(), 1);
    assert_eq!(report.tests[0].id, "tests");
    assert_eq!(
        report.tests[0].reasons,
        vec!["changed dependency `lib` input `src/removed/module.rs`"]
    );
    assert!(report.unmatched_paths.is_empty());
}

#[test]
fn affected_deps_reach_tests_transitively_within_the_package() {
    let workspace = TempDir::new().unwrap();
    let affected_deps = |name: &str| {
        BTreeMap::from([(
            "_affected_deps".to_string(),
            AttrValue::List(vec![AttrValue::String(name.to_string())]),
        )])
    };
    let mut ui = target("ws/ui", "test", &["libs/ui/**/*"], &[], true);
    ui.attrs = affected_deps("./core");
    let mut app = target("ws/app", "test", &["apps/app/**/*"], &[], true);
    app.attrs = affected_deps("ui");
    let graph = vec![
        target("ws/core", "library", &["libs/core/**/*"], &[], false),
        ui,
        app,
        target("ws/other", "test", &["libs/other/**/*"], &[], true),
    ];

    let report =
        selection_report(workspace.path(), &graph, &["ws/libs/core/a.ts".to_string()]).unwrap();

    let reason = "changed dependency `ws/core` input `ws/libs/core/a.ts`".to_string();
    assert_eq!(
        report
            .tests
            .iter()
            .map(|test| (test.id.as_str(), test.reasons.clone()))
            .collect::<Vec<_>>(),
        vec![("ws/ui", vec![reason.clone()]), ("ws/app", vec![reason])]
    );
}

#[test]
fn graph_definition_changes_select_every_test() {
    let workspace = TempDir::new().unwrap();
    let graph = vec![
        target("unit", "test", &["unit.rs"], &[], true),
        target("integration", "test", &["integration.rs"], &[], true),
    ];

    let report = selection_report(
        workspace.path(),
        &graph,
        &["packages/core/struo.toml".to_string()],
    )
    .unwrap();

    assert_eq!(
        report
            .tests
            .iter()
            .map(|test| test.id.as_str())
            .collect::<Vec<_>>(),
        vec!["unit", "integration"]
    );
}

#[test]
fn test_plan_identity_is_stable_and_independent_of_changed_path_order() {
    let workspace = TempDir::new().unwrap();
    let graph = vec![target(
        "unit",
        "test",
        &["src/**/*.rs", "tests/**/*.rs"],
        &[],
        true,
    )];

    let first = plan(
        workspace.path(),
        &graph,
        &["tests/a.rs".to_string(), "src/a.rs".to_string()],
    )
    .unwrap();
    let second = plan(
        workspace.path(),
        &graph,
        &["src/a.rs".to_string(), "tests/a.rs".to_string()],
    )
    .unwrap();

    assert_eq!(first.id, second.id);
    assert_eq!(first.batches, second.batches);
}

#[test]
fn paths_outside_known_packages_conservatively_select_every_test() {
    let workspace = TempDir::new().unwrap();
    let graph = vec![target("tests/unit", "test", &["**/*.rs"], &[], true)];

    let report = selection_report(
        workspace.path(),
        &graph,
        &["documentation/README.md".to_string()],
    )
    .unwrap();

    assert_eq!(report.tests.len(), 1);
    assert!(report.tests[0].reasons[0].contains("has no declared owner"));
    assert_eq!(report.unmatched_paths, vec!["documentation/README.md"]);
}

#[test]
fn package_manifest_changes_select_only_package_dependents() {
    let workspace = TempDir::new().unwrap();
    let graph = vec![
        target("packages/core/Library", "library", &["src/**"], &[], false),
        target(
            "packages/core/UnitTests",
            "test",
            &["tests/**"],
            &["packages/core/Library"],
            true,
        ),
        target("apps/OtherTests", "test", &["tests/**"], &[], true),
    ];

    let report = selection_report(
        workspace.path(),
        &graph,
        &["packages/core/struo.toml".to_string()],
    )
    .unwrap();

    assert_eq!(
        report
            .tests
            .iter()
            .map(|test| test.id.as_str())
            .collect::<Vec<_>>(),
        vec!["packages/core/UnitTests"]
    );
    assert!(report.unmatched_paths.is_empty());
}

#[test]
fn unowned_files_use_the_nearest_package() {
    let workspace = TempDir::new().unwrap();
    let graph = vec![
        target("packages/core/UnitTests", "test", &["tests/**"], &[], true),
        target("apps/OtherTests", "test", &["tests/**"], &[], true),
    ];

    let report = selection_report(
        workspace.path(),
        &graph,
        &["packages/core/README.md".to_string()],
    )
    .unwrap();

    assert_eq!(report.tests[0].id, "packages/core/UnitTests");
    assert_eq!(report.tests.len(), 1);
    assert!(report.unmatched_paths.is_empty());
}

#[test]
fn changed_paths_that_are_not_workspace_relative_do_not_fail_selection() {
    let workspace = TempDir::new().unwrap();
    let graph = vec![target("unit", "test", &["tests/**/*.rs"], &[], true)];

    // Absolute paths and `..` escapes cannot be normalized to workspace-relative
    // form. They must degrade to conservative selection, not abort the command.
    let report = selection_report(
        workspace.path(),
        &graph,
        &["/etc/hosts".to_string(), "../outside/file.rs".to_string()],
    )
    .unwrap();

    assert_eq!(report.tests.len(), 1);
    assert!(report.tests[0].reasons[0].contains("has no declared owner"));
}

#[test]
fn configured_module_changes_select_every_test() {
    let workspace = TempDir::new().unwrap();
    std::fs::write(
        workspace.path().join("struo.toml"),
        "[modules]\npaths = [\"modules/**/*.star\"]\n",
    )
    .unwrap();
    let graph = vec![target("unit", "test", &["tests/**/*.rs"], &[], true)];

    let report = selection_report(
        workspace.path(),
        &graph,
        &["modules/testing.star".to_string()],
    )
    .unwrap();

    assert_eq!(report.tests.len(), 1);
    assert!(report.tests[0].reasons[0].contains("changed graph definition"));
}

#[test]
fn default_selection_uses_only_test_roots_reported_by_the_resolver() {
    let mut resolver = target("cargo", "cargo_workspace", &["Cargo.toml"], &[], false);
    resolver.attrs.insert(
        "_default_test_roots".to_string(),
        AttrValue::List(vec![AttrValue::String("first_party_tests".to_string())]),
    );
    let graph = vec![
        resolver,
        target(
            "first_party_tests",
            "rust_test",
            &["tests/main.rs"],
            &[],
            true,
        ),
        target(
            "dependency_tests",
            "rust_test",
            &["vendor/dependency/tests.rs"],
            &[],
            true,
        ),
    ];

    let report = default_selection_report(&graph, &BTreeSet::from(["cargo_workspace".to_string()]));

    assert_eq!(report.tests.len(), 1);
    assert_eq!(report.tests[0].id, "first_party_tests");
    assert_eq!(report.policy.evidence, "resolver_test_roots");
}

#[test]
fn a_changed_workspace_manifest_selects_the_first_party_tests_it_reports() {
    let workspace = TempDir::new().unwrap();
    let mut resolver = target(
        "packages/tool/swift_package",
        "swift_package_workspace",
        &["Package.swift"],
        &["packages/tool/Tool"],
        false,
    );
    resolver.attrs.insert(
        "_default_test_roots".to_string(),
        AttrValue::List(vec![AttrValue::String("ToolTests".to_string())]),
    );
    let graph = vec![
        resolver,
        target(
            "packages/tool/Tool",
            "apple_library",
            &["Sources/Tool/Tool.swift"],
            &[],
            false,
        ),
        target(
            "packages/tool/ToolTests",
            "apple_test_bundle",
            &["Tests/ToolTests/Test.swift"],
            &["packages/tool/Tool"],
            true,
        ),
        target(
            "packages/tool/DependencyTests",
            "apple_test_bundle",
            &[".struo/swift-package-packages/dependency/Tests/Test.swift"],
            &[],
            true,
        ),
    ];

    let selected = |graph: &[GraphTarget]| {
        selection_report(
            workspace.path(),
            graph,
            &["packages/tool/Package.swift".to_string()],
        )
        .unwrap()
        .tests
        .into_iter()
        .map(|test| (test.id, test.reasons))
        .collect::<Vec<_>>()
    };

    assert_eq!(
        selected(&graph),
        vec![(
            "packages/tool/ToolTests".to_string(),
            vec!["changed workspace manifest `packages/tool/Package.swift` of `packages/tool/swift_package`".to_string()]
        )]
    );

    // A generated target that claims the manifest itself owns the change.
    let mut claimed = graph.clone();
    claimed[1].srcs.push("Package.swift".to_string());
    let selected = selected(&claimed);
    assert_eq!(
        selected
            .iter()
            .map(|(id, _)| id.as_str())
            .collect::<Vec<_>>(),
        vec!["packages/tool/ToolTests"]
    );
    assert!(
        !selected[0]
            .1
            .iter()
            .any(|reason| reason.contains("workspace manifest")),
        "{selected:?}"
    );
}

#[test]
fn default_selection_resolves_reported_test_names_inside_the_seed_package() {
    let mut resolver = target(
        "packages/tool/swift_package",
        "swift_package_workspace",
        &["Package.swift"],
        &[],
        false,
    );
    resolver.attrs.insert(
        "_default_test_roots".to_string(),
        AttrValue::List(vec![AttrValue::String("ToolTests".to_string())]),
    );
    let graph = vec![
        resolver,
        target(
            "packages/tool/ToolTests",
            "apple_test_bundle",
            &["Tests/ToolTests/Test.swift"],
            &[],
            true,
        ),
    ];

    let report = default_selection_report(
        &graph,
        &BTreeSet::from(["swift_package_workspace".to_string()]),
    );

    assert_eq!(report.tests.len(), 1);
    assert_eq!(report.tests[0].id, "packages/tool/ToolTests");
}

#[test]
fn root_package_owns_its_tree_except_the_paths_it_excludes() {
    let workspace = TempDir::new().unwrap();
    let mut root_library = target("app", "library", &["./**/*"], &["util"], false);
    root_library.attrs.insert(
        "srcs_exclude".to_string(),
        AttrValue::List(vec![
            AttrValue::String("crates/util/**".to_string()),
            AttrValue::String("target/**".to_string()),
        ]),
    );
    // `gen`'s build script reads `proto`, which lies in the root package's tree.
    let mut generated = target("gen", "library", &["crates/gen/**/*"], &[], false);
    generated.attrs.insert(
        "build_script_data".to_string(),
        AttrValue::List(vec![AttrValue::String("proto".to_string())]),
    );
    let graph = vec![
        target("util", "library", &["crates/util/**/*"], &[], false),
        target("util_tests", "test", &["crates/util/**/*"], &["util"], true),
        generated,
        target("gen_consumer_tests", "test", &[], &["gen"], true),
        root_library,
        target("app_tests", "test", &[], &["app"], true),
    ];
    let selected = |path: &str| {
        let report = selection_report(workspace.path(), &graph, &[path.to_string()]).unwrap();
        (
            report
                .tests
                .iter()
                .map(|test| test.id.clone())
                .collect::<Vec<_>>(),
            report.unmatched_paths,
        )
    };

    assert_eq!(
        selected("src/lib.rs"),
        (vec!["app_tests".to_string()], vec![])
    );
    assert_eq!(
        selected("docs/guide.md"),
        (vec!["app_tests".to_string()], vec![])
    );
    assert_eq!(
        selected("crates/util/src/lib.rs"),
        (
            vec!["util_tests".to_string(), "app_tests".to_string()],
            vec![]
        )
    );
    assert_eq!(
        selected("proto/api/v1/ref.proto"),
        (
            vec!["gen_consumer_tests".to_string(), "app_tests".to_string()],
            vec![]
        )
    );
    // Build output the root package excludes is nobody's input.
    assert_eq!(selected("target/debug/app"), (vec![], vec![]));
}

#[test]
fn a_path_another_target_owns_still_selects_the_targets_that_name_it() {
    let workspace = TempDir::new().unwrap();
    std::fs::create_dir_all(workspace.path().join("src")).unwrap();
    std::fs::write(
        workspace.path().join("src/lib.rs"),
        "read(\"docs/reference/cli.md\")",
    )
    .unwrap();
    let mut app = target("app", "library", &["./**/*"], &[], false);
    for (name, pattern) in [
        ("_affected_srcs", "src/**"),
        ("_affected_mentions", "**/*.rs"),
    ] {
        app.attrs.insert(
            name.to_string(),
            AttrValue::List(vec![AttrValue::String(pattern.to_string())]),
        );
    }
    let graph = vec![
        app,
        target("app_tests", "test", &[], &["app"], true),
        target("docs_lint", "test", &["docs/**"], &[], true),
    ];
    let selected = |path: &str| {
        selection_report(workspace.path(), &graph, &[path.to_string()])
            .unwrap()
            .tests
            .into_iter()
            .map(|test| test.id)
            .collect::<Vec<_>>()
    };

    assert_eq!(
        selected("docs/reference/cli.md"),
        ["app_tests", "docs_lint"]
    );
    assert_eq!(selected("docs/guide.md"), ["docs_lint"]);
}

// Turborepo's seed claims every file through `_affected_srcs`, as turbo's
// root package does. Beside a Cargo workspace at the same root, that claim
// must not take a changed `Cargo.toml` from the Cargo seed, nor a file only
// the Cargo side would fall back to, while a file no task reads still selects
// none of the Turborepo tests.
#[test]
fn a_catch_all_seed_leaves_other_owners_their_tests() {
    let workspace = TempDir::new().unwrap();
    let strings = |values: &[&str]| {
        AttrValue::List(
            values
                .iter()
                .map(|value| AttrValue::String((*value).to_string()))
                .collect(),
        )
    };
    let mut cargo = target("cargo", "cargo_workspace", &["Cargo.toml"], &[], false);
    cargo
        .attrs
        .insert("_default_test_roots".to_string(), strings(&["rust_tests"]));
    let mut turbo = target("turbo", "turbo_workspace", &["turbo.json"], &[], false);
    turbo
        .attrs
        .insert("_default_test_roots".to_string(), strings(&["web__test"]));
    turbo
        .attrs
        .insert("_affected_srcs".to_string(), strings(&["**/*"]));
    let mut web = target(
        "web__test",
        "turbo_test",
        &["apps/web/**/*"],
        &["web__build"],
        true,
    );
    web.attrs
        .insert("_affected_srcs".to_string(), strings(&["apps/web/**"]));
    let mut build = target("web__build", "turbo_task", &["apps/web/**/*"], &[], false);
    build
        .attrs
        .insert("_affected_srcs".to_string(), strings(&["apps/web/**"]));
    let mut rust_tests = target(
        "rust_tests",
        "rust_test",
        &["crates/core/**/*.rs"],
        &[],
        true,
    );
    rust_tests
        .attrs
        .insert("_affected_srcs".to_string(), strings(&["crates/core/**"]));
    let graph = vec![
        cargo,
        turbo,
        web,
        build,
        rust_tests,
        target(
            "swift_tests",
            "apple_test_bundle",
            &["Sources/**/*.swift"],
            &[],
            true,
        ),
    ];
    let selected = |path: &str| {
        selection_report(workspace.path(), &graph, &[path.to_string()])
            .unwrap()
            .tests
            .into_iter()
            .map(|test| test.id)
            .collect::<Vec<_>>()
    };

    assert_eq!(selected("Cargo.toml"), ["rust_tests"]);
    assert_eq!(selected("turbo.json"), ["web__test"]);
    assert_eq!(selected("apps/web/page.tsx"), ["web__test"]);
    // Root-package targets without `_affected_srcs` keep the nearest-package
    // fallback; the Turborepo test, which declares what it reads, does not.
    assert_eq!(selected("README.md"), ["rust_tests", "swift_tests"]);
    // A path outside every package, such as documentation a doc-test
    // includes, keeps the conservative whole-scope fallback for every test
    // but the catch-all seed's own.
    let report =
        selection_report(workspace.path(), &graph, &["docs/guide.md".to_string()]).unwrap();
    assert_eq!(report.unmatched_paths, ["docs/guide.md"]);
    assert_eq!(
        report
            .tests
            .into_iter()
            .map(|test| test.id)
            .collect::<Vec<_>>(),
        ["rust_tests", "swift_tests"]
    );
    let mut turbo_only = graph.clone();
    turbo_only.retain(|target| target.kind.starts_with("turbo"));
    let report =
        selection_report(workspace.path(), &turbo_only, &["README.md".to_string()]).unwrap();
    assert!(report.tests.is_empty(), "{:?}", report.tests);
}

fn target(id: &str, kind: &str, srcs: &[&str], deps: &[&str], test: bool) -> GraphTarget {
    let (package, name) = id.rsplit_once('/').unwrap_or(("", id));
    GraphTarget {
        label: TargetLabel {
            package: package.to_string(),
            name: name.to_string(),
            id: id.to_string(),
        },
        kind: kind.to_string(),
        deps: deps.iter().map(ToString::to_string).collect(),
        dependency_edges: BTreeMap::new(),
        srcs: srcs.iter().map(ToString::to_string).collect(),
        visibility: Vec::new(),
        attrs: BTreeMap::new(),
        capabilities: test
            .then(|| Capability {
                name: "test".to_string(),
                output_groups: Vec::new(),
                requires_outputs: Vec::new(),
            })
            .into_iter()
            .collect(),
        providers: Vec::new(),
        tools: Vec::new(),
        diagnostics: Vec::new(),
    }
}
