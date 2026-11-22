use std::collections::{BTreeMap, BTreeSet};

use struo_frontend::{ResolutionRecord, ResolverInputs};

use super::touched;

fn record(package: &str, patterns: &[&str], paths: &[&str]) -> ResolutionRecord {
    ResolutionRecord {
        resolvers: vec![ResolverInputs {
            package: package.to_string(),
            patterns: patterns.iter().map(|value| (*value).to_string()).collect(),
            excludes: Vec::new(),
            paths: paths.iter().map(|value| (*value).to_string()).collect(),
        }],
        observations: struo_frontend::analysis::AnalysisObservations::default(),
    }
}

fn changed(paths: &[&str]) -> BTreeSet<String> {
    paths.iter().map(|value| (*value).to_string()).collect()
}

#[test]
fn a_stored_graph_round_trips_and_is_invalidated_by_manifest_changes() {
    let workspace = tempfile::TempDir::new().unwrap();
    let cache = super::ResolutionCache {
        path: workspace.path().join(".struo/resolution.json"),
        enabled: true,
    };
    let position = crate::commands::change_tracker::ChangePosition {
        instance_id: "tracker".to_string(),
        source_generation: 3,
        output_generation: 7,
    };
    let graph = struo_frontend::graph_from_targets(&[struo_frontend::Target {
        package: "app".to_string(),
        kind: "script".to_string(),
        name: "build".to_string(),
        deps: Vec::new(),
        dependency_edges: BTreeMap::new(),
        srcs: vec!["src/**".to_string()],
        visibility: Vec::new(),
        attrs: BTreeMap::new(),
        typed_attrs: BTreeMap::from([(
            "metadata".to_string(),
            struo_frontend::AttrValue::String("λ\n\"".repeat(3_000)),
        )]),
        resolver_input_exclude: Vec::new(),
    }]);
    let record = record("", &["project.lock"], &["project.lock"]);
    cache.store("graph-key", Some(&position), &graph, &record);
    let analyzer = struo_frontend::analysis::AnalysisEngine::from_source("").unwrap();
    let changes = |sources| super::KnownChanges::Since {
        sources,
        outputs: BTreeSet::new(),
    };
    assert_eq!(
        cache.reuse(
            &analyzer,
            workspace.path(),
            "graph-key",
            &changes(changed(&["app/src/main"]))
        ),
        Some(graph)
    );
    assert!(cache
        .reuse(
            &analyzer,
            workspace.path(),
            "other-key",
            &changes(BTreeSet::new())
        )
        .is_none());
    assert!(cache
        .reuse(
            &analyzer,
            workspace.path(),
            "graph-key",
            &changes(changed(&["project.lock"]))
        )
        .is_none());
}

#[test]
fn an_edited_source_file_does_not_touch_the_derivation() {
    let record = record(
        "",
        &["Cargo.lock", "**/Cargo.toml"],
        &["Cargo.toml", "Cargo.lock"],
    );

    assert!(!touched(&record, &changed(&["crates/core/main.rs"])));
}

#[test]
fn editing_a_file_the_derivation_read_touches_it() {
    let record = record(
        "",
        &["Cargo.lock", "**/Cargo.toml"],
        &["Cargo.toml", "Cargo.lock"],
    );

    assert!(touched(&record, &changed(&["Cargo.lock"])));
}

/// The case a list of read files misses on its own: a manifest that did not
/// exist when the graph was derived is not in the list, and yet it changes what
/// the derivation would produce.
#[test]
fn a_manifest_that_appears_touches_the_derivation() {
    let record = record(
        "",
        &["Cargo.lock", "**/Cargo.toml"],
        &["Cargo.toml", "Cargo.lock"],
    );

    assert!(touched(&record, &changed(&["crates/new/Cargo.toml"])));
}

#[test]
fn a_resolver_in_a_package_matches_paths_under_that_package() {
    let record = record("apps/tool", &["**/Cargo.toml"], &["Cargo.toml"]);

    assert!(touched(&record, &changed(&["apps/tool/nested/Cargo.toml"])));
    assert!(!touched(&record, &changed(&["apps/other/Cargo.toml"])));
    assert!(
        touched(&record, &changed(&["apps/tool/Cargo.toml"])),
        "the package-relative path a resolver read is named without its package prefix"
    );
}

#[test]
fn nothing_changed_touches_nothing() {
    let record = record("", &["**/Cargo.toml"], &["Cargo.toml"]);

    assert!(!touched(&record, &changed(&[])));
}
