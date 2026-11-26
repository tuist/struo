use std::collections::BTreeMap;

use serde_json::json;
use tempfile::TempDir;

use super::target;
use crate::analysis::{AnalysisEngine, SharedProvider};

#[test]
fn shared_providers_isolate_nested_mutations_across_consumers_and_roles() {
    let engine = AnalysisEngine::from_source(
        r#"
def _impl(ctx):
    dep = ctx["deps"][0]
    dep["flat"][1] = "edited"
    dep["nested"][1]["items"].append("added")
    dep["nested"][3].append(7)
    dep["nested"][4]["inserted"] = True
    dep["mapping"]["a"]["new"] = [1]
    dep["mapping"]["z"].clear()
    dep["new"] = "field"
    return {
        "mutated": dep,
        "alias": ctx["deps_by_role"]["deps"][0],
        "other": ctx["deps"][1],
        "named": ctx["deps_by_role"]["tools"][0],
        "arithmetic": int(dep["flat"][5] * 4),
    }
demo_kind = {"_struo_target_kind": True, "impl": _impl}
"#,
    )
    .unwrap();
    let root = TempDir::new().unwrap();
    let original = json!({
        "flat": ["first", "λ\n", true, null, -55, 3.25],
        "nested": [false, {"items": ["left", "right"]}, "sentinel", [], {}],
        "mapping": {"z": ["keep"], "a": {}},
    });
    let mutated = json!({
        "flat": ["first", "edited", true, null, -55, "3.25"],
        "nested": [false, {"items": ["left", "right", "added"]}, "sentinel", [7], {"inserted": true}],
        "mapping": {"z": [], "a": {"new": [1]}},
        "new": "field",
    });
    let mut exported = original.clone();
    exported["flat"][5] = json!("3.25");
    let expected = json!({"mutated": mutated, "alias": mutated, "other": exported, "named": exported, "arithmetic": 13});
    let baseline = engine
        .analyze_target_capability_with_dependency_roles(
            &target("demo_kind"),
            root.path(),
            &[original.clone(), original.clone()],
            &BTreeMap::from([("tools".to_string(), vec![original.clone()])]),
            "build",
        )
        .unwrap();
    assert_eq!(baseline.provider, expected);
    let restored =
        serde_json::from_slice::<SharedProvider>(&serde_json::to_vec(&original).unwrap()).unwrap();
    for provider in [SharedProvider::from(original.clone()), restored] {
        let deps = [provider.clone(), provider.clone()];
        let roles = BTreeMap::from([("tools".to_string(), vec![provider.clone()])]);
        let target = target("demo_kind");
        let key = engine.analysis_key(&target, root.path(), &deps, &roles, "build", "test");
        std::thread::scope(|scope| {
            for _ in 0..4 {
                scope.spawn(|| {
                    for _ in 0..3 {
                        let result = engine
                            .analyze_target_capability_with_shared_dependency_roles(
                                &target,
                                root.path(),
                                &deps,
                                &roles,
                                "build",
                            )
                            .unwrap();
                        assert_eq!(result.provider, expected);
                    }
                });
            }
        });
        assert_eq!(provider.as_ref(), &original);
        assert_eq!(
            key,
            engine.analysis_key(&target, root.path(), &deps, &roles, "build", "test")
        );
    }
}

#[test]
fn prepared_references_resolve_in_the_consumers_workspace_and_retry_missing_sets() {
    let source = r#"
def _produce(ctx):
    return {"nested": [transitive_set(direct = ["first", "second"])]}
def _consume(ctx):
    return {"items": ctx["deps"][0]["nested"][0].to_list()}
producer = {"_struo_target_kind": True, "impl": _produce}
consumer = {"_struo_target_kind": True, "impl": _consume}
"#;
    let producer = AnalysisEngine::from_source(source).unwrap();
    let first = TempDir::new().unwrap();
    let second = TempDir::new().unwrap();
    let result = producer
        .analyze_target(&target("producer"), first.path(), &[])
        .unwrap();
    let provider = SharedProvider::from(result.provider);
    let consume = |root: &std::path::Path| {
        AnalysisEngine::from_source(source)
            .unwrap()
            .analyze_target_capability_with_shared_dependency_roles(
                &target("consumer"),
                root,
                std::slice::from_ref(&provider),
                &BTreeMap::new(),
                "build",
            )
    };
    assert_eq!(
        consume(first.path()).unwrap().provider,
        json!({"items": ["first", "second"]})
    );
    assert!(consume(second.path()).is_err());
    AnalysisEngine::from_source(source)
        .unwrap()
        .analyze_target(&target("producer"), second.path(), &[])
        .unwrap();
    assert_eq!(
        consume(second.path()).unwrap().provider,
        json!({"items": ["first", "second"]})
    );
}

#[test]
fn cached_provider_arrays_are_isolated_between_consumers() {
    let engine = AnalysisEngine::from_source(
        r#"
def _impl(ctx):
    ctx["deps"][0]["nested"][0].append("added")
    return {"first": ctx["deps"][0], "second": ctx["deps"][1]}
demo_kind = {"_struo_target_kind": True, "impl": _impl}
"#,
    )
    .unwrap();
    let root = TempDir::new().unwrap();
    let provider: SharedProvider =
        serde_json::from_str(r#"{"nested":[["left","right"]]}"#).unwrap();
    for _ in 0..2 {
        let result = engine
            .analyze_target_capability_with_shared_dependency_roles(
                &target("demo_kind"),
                root.path(),
                &[provider.clone(), provider.clone()],
                &BTreeMap::new(),
                "build",
            )
            .unwrap();
        assert_eq!(
            result.provider,
            json!({
                "first": {"nested": [["left", "right", "added"]]},
                "second": {"nested": [["left", "right"]]},
            })
        );
    }
    assert_eq!(provider.as_ref(), &json!({"nested": [["left", "right"]]}));
}
