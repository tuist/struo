use super::*;

#[test]
fn bazel_test_query_selects_roots_without_removing_build_targets() {
    let source = format!(
        r#"{}
def host_which_optional(name):
    return "/tools/bazel"
def host_command(argv, cwd = None, env = None):
    if argv[1] == "--version":
        return "bazel 9"
    if "manual" in argv[2]:
        return ""
    if argv[2] == "selected":
        return "cc_test rule //:unit\ncc_test rule //:outside_graph"
    return "cc_library rule //:library\ncc_test rule //:unit\ncc_test rule //:benchmark"
graph = bazel_workspace["resolver"]({{"label": {{"id": "bazel", "package": ""}}, "attr": {{"test_query": "selected"}}}})
result = repr([graph["roots"], graph["attrs"]["_default_test_roots"], [target["name"] for target in graph["targets"]]])
"#,
        all_prelude_source(),
    );
    assert_eq!(
        eval_prelude_source_to_repr(source).unwrap(),
        r#"[["bz_library", "bz_unit", "bz_benchmark"], ["bz_unit"], ["bz_library", "bz_unit", "bz_benchmark"]]"#
    );
}

#[test]
fn bazel_workspace_forwards_declared_environment_to_generated_targets() {
    let source = format!(
        r#"{}
def host_env(name):
    return {{"HOME": "/home/test", "PATH": "/tools"}}.get(name)
def host_which_optional(name):
    return "/tools/bazelisk" if name == "bazelisk" else ""
def host_file_exists(path):
    return False
def workspace_root():
    return "/workspace"
def host_command(argv, cwd = None, env = None):
    if env.get("SDK_ROOT") != "" or env.get("CUSTOM") != "declared":
        fail("missing declared environment")
    if argv[0] != "/tools/bazelisk" or cwd != "/workspace/project":
        fail("wrong tool or workspace")
    if argv[1] == "query" and "manual" in argv[2]:
        return ""
    return "bazel 9" if argv[1] == "--version" else "cc_library rule //lib:core\ncc_test rule //lib:core_test"
ctx = {{"label": {{"id": "project/bazel", "package": "project"}}, "attr": {{"env": {{"SDK_ROOT": "", "CUSTOM": "declared"}}}}}}
graph = _bazel_workspace_resolver(ctx)
result = repr([graph["roots"], graph["attrs"]["_default_test_roots"], [target["attrs"]["env"] for target in graph["targets"]]])
"#,
        all_prelude_source(),
    );
    assert_eq!(
        eval_prelude_source_to_repr(source).unwrap(),
        r#"[["bz_lib_core", "bz_lib_core_test"], ["bz_lib_core_test"], [{"SDK_ROOT": "", "CUSTOM": "declared"}, {"SDK_ROOT": "", "CUSTOM": "declared"}]]"#
    );
}

#[test]
fn bazel_explicit_executable_does_not_fall_back_to_bazelisk() {
    let source = format!(
        r#"{}
def host_which_optional(name):
    return "/tools/bazelisk" if name == "bazelisk" else ""
def host_file_exists(path):
    return False
result = repr(_bazel_resolve_executable({{"label": {{"id": "bazel"}}, "attr": {{"bazel": "missing-bazel"}}}}))
"#,
        all_prelude_source(),
    );
    assert!(eval_prelude_source_to_repr(source)
        .unwrap_err()
        .contains("Bazel executable `missing-bazel` was not found"));
}

#[test]
fn bazel_actions_preserve_empty_overrides_and_native_action_environment() {
    for (mnemonic, native) in [("CppCompile", true), ("Unsupported", false)] {
        let workspace = TempDir::new().unwrap();
        let source = format!(
            r#"{}
def host_env(name):
    return "/tools" if name == "PATH" else ""
def host_which_optional(name):
    return "/tools/bazel"
def host_command(argv, cwd = None, env = None, max_output_bytes = None):
    if len(argv) > 1 and argv[1] == "aquery":
        if env.get("SDK_ROOT") != "" or env.get("CUSTOM") != "declared":
            fail("missing analysis environment")
        return '{{"actions": [{{"mnemonic": "{mnemonic}", "arguments": {arguments}, "environmentVariables": [{{"key": "CUSTOM", "value": "native"}}]}}]}}'
    return "/output-base" if len(argv) > 1 and argv[1] == "info" else ""
ctx = {{
    "label": {{"id": "library", "name": "library", "package": ""}},
    "attr": {{"_bazel_resolved": True, "bazel_label": "//:library", "bazel_rule_kind": "cc_library", "env": {{"SDK_ROOT": "", "CUSTOM": "declared"}}}},
    "capability": "build",
}}
result = repr(bazel_target["impl"](ctx))
"#,
            all_prelude_source(),
            arguments = if native {
                r#"["cc", "source.cc"]"#
            } else {
                "[]"
            },
        );
        let (store, result) = with_active_store(store_for(workspace.path(), ""), || {
            eval_prelude_source_to_repr(source)
        });
        result.unwrap();
        let action = store.actions.last().unwrap();
        assert_eq!(action.env.get("SDK_ROOT").map(String::as_str), Some(""));
        assert_eq!(
            action.env.get("CUSTOM").map(String::as_str),
            Some(if native { "native" } else { "declared" })
        );
    }
}

#[test]
fn bazel_settles_delegation_before_the_aquery_payload_and_keeps_native_flags() {
    // A required template expansion has no argv, so Struo delegates without
    // the payload. The same action in an unused output group does not count.
    for (template_output, delegated) in [("bazel-out/app.txt", true), ("bazel-out/unused", false)] {
        let workspace = TempDir::new().unwrap();
        let source = format!(
            r#"{}
def host_which_optional(name):
    return "/tools/bazel"
calls = []
def host_command(argv, cwd = None, env = None, max_output_bytes = None):
    calls.append(argv)
    if argv[1] == "aquery":
        copy = '' if {delegated} else '{{"mnemonic": "Copy", "arguments": ["cp"], "outputIds": [2]}}, '
        return '{{"pathFragments": [{{"id": 1, "label": "{template_output}"}}, {{"id": 2, "label": "bazel-out/app.txt"}}], "artifacts": [{{"id": 1, "pathFragmentId": 1}}, {{"id": 2, "pathFragmentId": 2}}], "actions": [' + copy + '{{"mnemonic": "TemplateExpand", "outputIds": [1]}}]}}'
    if argv[1] == "cquery":
        return "bazel-out/app.txt\n"
    return "/output-base" if argv[1] == "info" else ""
ctx = {{
    "label": {{"id": "app", "name": "app", "package": ""}},
    "attr": {{"_bazel_resolved": True, "bazel_label": "//:app", "bazel_rule_kind": "genrule"}},
    "capability": "build",
}}
mode = bazel_target["impl"](ctx)["execution_mode"]
result = repr([mode, len([argv for argv in calls if "--include_param_files" in argv])])
"#,
            all_prelude_source(),
            delegated = if delegated { "True" } else { "False" },
        );
        let (store, result) = with_active_store(store_for(workspace.path(), ""), || {
            eval_prelude_source_to_repr(source)
        });
        let argv = &store.actions.last().unwrap().argv;
        if delegated {
            assert_eq!(result.unwrap(), r#"["delegated", 0]"#);
            assert_eq!(
                argv,
                &[
                    "/tools/bazel",
                    "build",
                    "//:app",
                    "--noshow_progress",
                    "--experimental_convenience_symlinks=ignore"
                ]
            );
        } else {
            assert_eq!(result.unwrap(), r#"["own", 1]"#);
            assert_eq!(argv, &["cp"]);
        }
    }
}

fn package_source(source: &str) -> String {
    format!(
        "{}\n{}\n{source}",
        xcode_prelude_source(),
        include_str!("../../prelude/swift_package.star")
    )
}

#[test]
fn native_package_options_propagate_without_inferred_dependencies() {
    let source = package_source(
        r#"
def _resolve_swiftc(platform, variant, developer):
    return {"swiftc_path": "/swiftc", "env": {}}
def _swiftpm_absolute_package_path(ctx, path):
    return "/workspace"
def host_command(argv, env = None):
    return '{"name": "Root", "dependencies": [], "products": [], "targets": []}'
def _swift_package_metadata(argv, absolute, env = {}, check = True):
    return host_command(argv, env = env)
def _xcode_local_swift_package_specs(ctx, packages, platform, minimum_os, variant, root_identities = None, owner_package = None):
    return {"specs": [{"kind": "apple_library", "attrs": {}, "deps": ["./Declared"]}, {"kind": "archive_download", "attrs": {}}], "products": {}, "modules": {}}
graph = _swift_package_workspace_resolver({
    "attrs": {"explicit_modules": True, "dependency_check": "error"},
    "files": {"Package.swift": "manifest"}, "label": {"id": "root", "name": "root", "package": ""},
})
result = repr([[spec["attrs"], spec.get("deps")] for spec in graph["targets"]])
"#,
    );
    assert_eq!(
        eval_prelude_source_to_repr(source).unwrap(),
        r#"[[{"explicit_modules": True, "dependency_check": "error"}, ["./Declared"]], [{}, None]]"#
    );
}

#[test]
fn nested_package_source_membership_is_observed_for_graph_reuse() {
    use struo_frontend::analysis::{AnalysisEngine, CommandPolicy, UnchangedWorkspace};

    let workspace = TempDir::new().unwrap();
    let sources = workspace.path().join("Dependencies/Local/Sources/Local");
    std::fs::create_dir_all(&sources).unwrap();
    std::fs::write(sources.join("First.swift"), "public let first = 1").unwrap();
    let source = package_source(
        r#"result = repr(_xcode_swift_package_target_sources("Dependencies/Local", {"name": "Local", "type": "regular"}))"#,
    );
    let (store, result) = with_active_store(store_for(workspace.path(), ""), || {
        eval_prelude_source_to_repr(source)
    });
    assert_eq!(
        result.unwrap(),
        r#"["Dependencies/Local/Sources/Local/First.swift"]"#
    );
    let observations_hold = || {
        AnalysisEngine::new().unwrap().observations_hold(
            workspace.path(),
            &store.observations,
            CommandPolicy::TrustDeclaredInputs,
            &UnchangedWorkspace::Unknown,
        )
    };
    assert!(observations_hold());
    std::fs::write(sources.join("Second.swift"), "public let second = 2").unwrap();
    assert!(!observations_hold());
}

#[test]
fn product_dependencies_expand_all_modules_even_when_a_target_has_the_same_name() {
    let source = package_source(
        r#"
targets = {"network\x1fNIO": "NIO", "network\x1fNIOCore": "Core"}
products = {"network\x1fNIO": ["NIO", "Core"], "NIO": "Unrelated"}
result = repr([
    _xcode_swift_package_dependencies({"dependencies": [{"product": ["NIO", "network"]}]}, "app", targets, products, "macos"),
    _xcode_swift_package_dependencies({"dependencies": [{"target": ["NIO"]}]}, "network", targets, products, "macos"),
    _xcode_swift_package_dependencies({"dependencies": [{"product": ["NIO", "missing"]}]}, "app", targets, products, "macos"),
])
"#,
    );
    assert_eq!(
        eval_prelude_source_to_repr(source).unwrap(),
        r#"[["./NIO", "./Core"], ["./NIO"], []]"#
    );
}

#[test]
fn local_only_dependencies_do_not_resolve_or_require_a_lockfile() {
    let source = package_source(
        r#"
def host_command(argv, env = None):
    fail("local dependencies must not run package resolve")
result = repr(_swift_package_remote_infos(
    {"files": {}}, {"dependencies": [{"fileSystem": [{"identity": "local", "path": "/workspace/local"}]}]},
    "swift", {}, "/workspace", "",
))
"#,
    );
    assert_eq!(eval_prelude_source_to_repr(source).unwrap(), "[]");
}

#[test]
fn local_package_closure_is_deduplicated_and_ignores_nested_lockfiles() {
    let source = package_source(
        r#"
def workspace_root():
    return "/workspace"
def host_file_exists(path):
    return path.endswith("/Package.swift")
def _xcode_package_resolved_pins_at(path):
    fail("nested lockfiles are not the root dependency resolution")
def _xcode_swift_package_info(ctx, path, identity = "", cache = None):
    return {"identity": identity, "path": path, "info": {"dependencies": [
        {"fileSystem": [{"identity": "shared", "path": "/workspace/shared"}]},
    ] if identity == "left" or identity == "right" else []}}
result = repr([package["identity"] for package in _xcode_expand_swift_package_infos({}, [{
    "identity": "root", "path": "", "info": {"dependencies": [
        {"fileSystem": [{"identity": "left", "path": "/workspace/left"}]},
        {"fileSystem": [{"identity": "right", "path": "/workspace/right"}]},
    ]},
}], follow_resolved = False)])
"#,
    );
    assert_eq!(
        eval_prelude_source_to_repr(source).unwrap(),
        r#"["root", "left", "right", "shared"]"#
    );
}

#[test]
fn transitive_remote_dependencies_still_require_resolution() {
    let source = package_source(
        r#"
calls = []
def host_command(argv, env = None):
    calls.append(argv)
    return ""
def host_file_exists(path):
    return path.endswith("/Package.resolved")
def host_file_read(path):
    return '{"version": 3, "pins": []}'
_swift_package_remote_infos(
    {"files": {}}, {"dependencies": [{"fileSystem": [{"identity": "local"}]}]},
    "swift", {}, "/workspace", "", remote_dependencies = True,
)
result = repr(calls)
"#,
    );
    assert_eq!(
        eval_prelude_source_to_repr(source).unwrap(),
        r#"[["swift", "package", "resolve", "--package-path", "/workspace"]]"#
    );
}

#[test]
fn a_lockfile_without_a_declared_dependency_resolves_again() {
    let source = package_source(
        r#"
resolves = []
def _resolve_swiftc(platform, variant, developer):
    return {"swiftc_path": "/swiftc", "env": {}}
def _swiftpm_absolute_package_path(ctx, path):
    return "/workspace"
def host_command(argv, env = None):
    if argv[1:3] == ["package", "resolve"]:
        resolves.append(argv)
        return ""
    return _json_encode({"name": "Root", "products": [], "dependencies": [
        {"sourceControl": [{"identity": "New-Name"}]},
        {"sourceControl": [{"identity": "aliased-url", "nameForTargetDependencyResolutionOnly": "Aliased"}]},
        {"sourceControl": [{"identity": "trait-only"}]},
    ], "targets": [{"name": "Root", "dependencies": [
        {"product": ["Lib", "new-name", None, None]},
        {"product": ["Other", "Aliased", None, None]},
        {"product": ["Extra", "trait-only", None, {"traits": ["Extra"]}]},
    ]}]})
def _swift_package_metadata(argv, absolute, env = {}, check = True):
    return host_command(argv, env = env)
def host_file_exists(path):
    return path.endswith("/Package.resolved")
def host_file_read(path):
    return '{"version": 2, "pins": []}'
def _xcode_acquire_remote_swift_packages(pins, checkout_root = ""):
    pass
def _xcode_local_swift_package_specs(ctx, packages, platform, minimum_os, variant, root_identities = None, owner_package = None):
    return {"specs": [], "products": {}, "modules": {}}
def lockfile(identities):
    pins = [{"identity": pinned, "kind": "localSourceControl", "location": "/" + pinned, "state": {"revision": "abc"}} for pinned in identities]
    return _json_encode({"version": 2, "pins": pins})
def resolves_with(pinned, package_path = "", alias_pinned = True):
    resolves.clear()
    prefix = package_path + "/" if package_path else ""
    _swift_package_workspace_resolver({
        "attrs": {"package_path": package_path} if package_path else {},
        "label": {"id": "root", "name": "root", "package": ""},
        "files": {prefix + "Package.swift": "manifest", prefix + "Package.resolved": lockfile([pinned] + (["aliased-url"] if alias_pinned else []))},
    })
    return len(resolves)
result = repr([resolves_with("old-name"), resolves_with("new-name"), resolves_with("new-name", alias_pinned = False), resolves_with("old-name", "tools/Tool"), resolves_with("new-name", "tools/Tool")])
"#,
    );
    assert_eq!(
        eval_prelude_source_to_repr(source).unwrap(),
        "[1, 0, 1, 1, 0]"
    );
}

#[test]
fn unreferenced_local_products_reuse_the_manifest_cache() {
    let source = package_source(
        r#"
def workspace_root():
    return "/workspace"
def glob(patterns):
    return ["Modules/Core/Package.swift"]
def host_command(argv, env = None):
    fail("the manifest is already parsed")
cache = {"/workspace/Modules/Core": {"info": {
    "name": "DifferentDisplayName", "products": [{"name": "Core", "targets": ["Core"]}],
}}}
result = repr(_xcode_local_package_products({}, ["Core"], cache = cache)["Core"]["identity"])
"#,
    );
    assert_eq!(eval_prelude_source_to_repr(source).unwrap(), r#""Core""#);
}

#[test]
fn product_discovery_skips_invalid_manifests_without_weakening_explicit_loading() {
    let workspace = TempDir::new().unwrap();
    let source = package_source(
        r#"
def workspace_root():
    return "/workspace"
def glob(patterns):
    return ["bin/template/Package.swift", "Modules/Core/Package.swift"]
def host_which(name):
    return name
def host_command(argv, check = True):
    if argv == ["xcrun", "--find", "swift"]:
        return "swift"
    if "template" in argv[-1]:
        if check:
            fail("explicit invalid manifest must fail")
        return ""
    return '{"name":"Core","products":[{"name":"Core","targets":["Core"]}],"targets":[]}'
def _swift_package_metadata(argv, absolute, env = {}, check = True):
    return host_command(argv, check = check)
result = repr(_xcode_local_package_products({}, ["Core"])["Core"]["path"])
"#,
    );
    let (_, result) = with_active_store(store_for(workspace.path(), ""), || {
        eval_prelude_source_to_repr(source.clone())
    });
    assert_eq!(result.unwrap(), r#""Modules/Core""#);
    let source = format!("{source}\n_xcode_swift_package_info({{}}, \"bin/template\")");
    let (_, result) = with_active_store(store_for(workspace.path(), ""), || {
        eval_prelude_source_to_repr(source)
    });
    assert!(result
        .unwrap_err()
        .contains("explicit invalid manifest must fail"));
}

#[test]
fn package_destinations_include_target_overrides_and_ignore_non_native_products() {
    let source = package_source(
        r#"
project = {
    "native_targets": [
        {"productType": "com.apple.product-type.application", "buildConfigurationList": "Mac"},
        {"productType": "com.apple.product-type.application", "buildConfigurationList": "Phone"},
        {"productType": "com.apple.product-type.app-extension", "buildConfigurationList": "Phone"},
        {"productType": "unsupported", "buildConfigurationList": "Watch"},
    ],
    "objects": {"Mac": {}, "Phone": {"buildConfigurations": ["PhoneDebug"]}, "PhoneDebug": {"name": "Debug", "buildSettings": {"SDKROOT": "iphoneos"}}},
    "project_settings": {"SDKROOT": "macosx"},
    "path_maps": {},
}
def destinations(settings):
    project["project_settings"] = settings
    return _xcode_package_platforms({"attr": {}}, project, "Debug")
result = repr([destinations(settings) for settings in [{"SDKROOT": "macosx"}, {"MACOSX_DEPLOYMENT_TARGET": "13.0"}, {"SUPPORTED_PLATFORMS": "macosx"}]])
"#,
    );
    assert_eq!(
        eval_prelude_source_to_repr(source).unwrap(),
        r#"[["macos", "ios"], ["macos", "ios"], ["macos", "ios"]]"#
    );
}

#[test]
fn framework_aliases_from_one_project_are_available_for_every_workspace_destination() {
    let source = package_source(
        r#"
def _xcode_realpath(path):
    return path
def _xcode_workspace_relative(path):
    return path
def _xcode_workspace_xcframework_specs(projects, platform, sdk_variant):
    return [{"name": "Vendor", "kind": "apple_xcframework_import", "attrs": {"bundle": "Vendor.xcframework", "platform": platform}}]
def _xcode_register_absolute_xcframework_refs(objects, specs, names, platform, sdk_variant):
    pass
graphs = _xcode_workspace_framework_graphs({"attr": {}}, [
    {"objects": {"File": {"isa": "PBXFileReference", "name": "PublicModule.xcframework"}}, "file_paths": {"File": "Vendor.xcframework"}},
    {"objects": {}, "file_paths": {}},
], ["ios", "macos"])
result = repr([[platform, graph["specs"][0]["attrs"]["platform"], graph["modules"][_xcode_product_dependency_key("PublicModule")], graph["names"]["Vendor.xcframework"]] for platform, graph in graphs.items()])
"#,
    );
    assert_eq!(
        eval_prelude_source_to_repr(source).unwrap(),
        r#"[["ios", "ios", "Vendor_ios", "Vendor_ios"], ["macos", "macos", "Vendor_macos", "Vendor_macos"]]"#
    );
}

#[test]
fn shared_schemes_preserve_only_common_test_exclusions() {
    let source = package_source(
        r#"
def _xcode_scheme_paths(ctx):
    return ["first.xcscheme", "second.xcscheme"]
def _xcode_referenced_test_plan_paths(ctx):
    return []
def host_file_read(path):
    extra = '<Test Identifier="OnlyFirst"/>' if path == "first.xcscheme" else ''
    return '<TestableReference><BuildableReference BlueprintName = "Tests"/><SkippedTests><Test Identifier = "OfflineSuite"/>' + extra + '</SkippedTests></TestableReference><TestableReference><BuildableReference BlueprintName="OtherTests"/><SkippedTests><Test Identifier="OtherSuite/testCase"/></SkippedTests></TestableReference>'
settings = _xcode_test_plan_settings({})
result = repr([settings["Tests"]["skipped_tests"], settings["OtherTests"]["skipped_tests"]])
"#,
    );
    assert_eq!(
        eval_prelude_source_to_repr(source).unwrap(),
        r#"[["OfflineSuite"], ["OtherSuite/testCase"]]"#
    );
}

#[test]
fn scheme_test_configurations_apply_only_when_every_scheme_agrees() {
    let source = package_source(
        r#"
def _xcode_scheme_paths(ctx):
    return ["app.xcscheme", "rtl.xcscheme", "other.xcscheme", "launch.xcscheme"]
def _xcode_abs(path):
    return path
def host_file_exists(path):
    return path == "Plans/App.xctestplan"
def host_file_read(path):
    if path == "Plans/App.xctestplan":
        return '{"testTargets": [{"target": {"name": "PlanTests"}}]}'
    if path == "app.xcscheme":
        return '<TestAction buildConfiguration = "Test" shouldUseLaunchSchemeArgsEnv = "YES"><TestPlans><TestPlanReference reference = "container:Plans/App.xctestplan" default = "YES"></TestPlanReference></TestPlans><Testables><TestableReference skipped = "NO"><BuildableReference BlueprintName = "AppTests"></BuildableReference></TestableReference></Testables></TestAction><LaunchAction buildConfiguration = "Debug"></LaunchAction>'
    if path == "rtl.xcscheme":
        return '<TestAction buildConfiguration = "Test"><Testables><TestableReference><BuildableReference BlueprintName = "AppTests"></BuildableReference></TestableReference><TestableReference><BuildableReference BlueprintName = "SharedTests"></BuildableReference></TestableReference></Testables></TestAction>'
    if path == "other.xcscheme":
        return '<TestAction buildConfiguration = "Debug"><Testables><TestableReference><BuildableReference BlueprintName = "SharedTests"></BuildableReference></TestableReference></Testables></TestAction>'
    return '<LaunchAction buildConfiguration = "Release"><BuildableReference BlueprintName = "LaunchOnly"></BuildableReference></LaunchAction>'
configurations = _xcode_scheme_test_configurations({})
result = repr(sorted(configurations.items()))
"#,
    );
    assert_eq!(
        eval_prelude_source_to_repr(source).unwrap(),
        r#"[("AppTests", "Test"), ("PlanTests", "Test")]"#
    );
}

fn native_target(
    id: &str,
    name: &str,
    product: &str,
    deps: &[&str],
    test_settings: &[(&str, &str)],
) -> serde_json::Map<String, serde_json::Value> {
    let mut entries = serde_json::Map::new();
    let configs = format!("{id}_CONFIGS");
    let mut debug = serde_json::json!({"PRODUCT_NAME": name, "PRODUCT_BUNDLE_IDENTIFIER": format!("dev.example.{name}"), "GENERATE_INFOPLIST_FILE": "YES", "DEFINES_MODULE": "YES"});
    if name == "ReaderTests" {
        debug["TEST_HOST"] = "$(BUILT_PRODUCTS_DIR)/Reader.app/Contents/MacOS/Reader".into();
    }
    let mut test = debug.clone();
    for (key, value) in test_settings {
        test[*key] = (*value).into();
    }
    entries.insert(
        format!("{id}_DEBUG"),
        serde_json::json!({"isa": "XCBuildConfiguration", "name": "Debug", "buildSettings": debug}),
    );
    entries.insert(
        format!("{id}_TEST"),
        serde_json::json!({"isa": "XCBuildConfiguration", "name": "Test", "buildSettings": test}),
    );
    entries.insert(configs.clone(), serde_json::json!({"isa": "XCConfigurationList", "buildConfigurations": [format!("{id}_DEBUG"), format!("{id}_TEST")], "defaultConfigurationName": "Debug"}));
    entries.insert(format!("{id}_FILE"), serde_json::json!({"isa": "PBXFileReference", "sourceTree": "<group>", "path": format!("{name}.swift")}));
    entries.insert(
        format!("{id}_BUILD_FILE"),
        serde_json::json!({"isa": "PBXBuildFile", "fileRef": format!("{id}_FILE")}),
    );
    entries.insert(
        format!("{id}_SOURCES"),
        serde_json::json!({"isa": "PBXSourcesBuildPhase", "files": [format!("{id}_BUILD_FILE")]}),
    );
    let mut dependencies = Vec::new();
    for dep in deps {
        entries.insert(
            format!("{id}_DEP_{dep}"),
            serde_json::json!({"isa": "PBXTargetDependency", "target": dep}),
        );
        dependencies.push(format!("{id}_DEP_{dep}"));
    }
    entries.insert(id.to_string(), serde_json::json!({"isa": "PBXNativeTarget", "name": name, "productType": product, "buildConfigurationList": configs, "buildPhases": [format!("{id}_SOURCES")], "dependencies": dependencies}));
    entries
}

#[test]
fn xcode_preserves_quoted_swift_flags_across_setting_layers() {
    let mut objects = native_target(
        "APP",
        "ReproApp",
        "com.apple.product-type.application",
        &[],
        &[],
    );
    let macro_flag =
        r#"-enable-experimental-feature "AvailabilityMacro=ReproOS 1.0: macOS 14.0, iOS 17.0""#;
    objects["APP_DEBUG"]["buildSettings"]["OTHER_SWIFT_FLAGS"] =
        serde_json::json!(["$(inherited)", macro_flag]);
    objects.insert("ROOT".into(), serde_json::json!({"isa": "PBXProject", "buildConfigurationList": "PROJECT_CONFIGS", "targets": ["APP"]}));
    objects.insert(
        "PROJECT_CONFIGS".into(),
        serde_json::json!({"isa": "XCConfigurationList", "buildConfigurations": ["PROJECT_DEBUG"]}),
    );
    objects.insert("PROJECT_DEBUG".into(), serde_json::json!({"isa": "XCBuildConfiguration", "name": "Debug", "buildSettings": {
        "SDKROOT": "macosx", "MACOSX_DEPLOYMENT_TARGET": "14.0", "OTHER_SWIFT_FLAGS": r#"-D PROJECT -I "Vendor/My Modules""#,
    }}));
    let pbxproj = serde_json::json!({"rootObject": "ROOT", "objects": objects}).to_string();
    let source = package_source(&format!(
        r#"
def workspace_root():
    return "/repo"
def host_command(argv, env = None, cwd = None, merge_stderr = None, check = True):
    return {pbxproj:?}
def host_file_exists(path):
    return path.endswith("/ReproApp.swift")
ctx = {{"label": {{"package": "app", "name": "xcode", "id": "app/xcode"}}, "attr": {{"project": "Repro.xcodeproj"}}}}
app = [spec for spec in _xcode_workspace_resolver(ctx)["targets"] if spec["name"] == "ReproApp"][0]
flags = app["attrs"]["swift_flags"]
result = repr(flags[:flags.index("-enable-bare-slash-regex")])
"#,
    ));
    assert_eq!(
        eval_prelude_source_to_repr(source).unwrap(),
        r#"["-D", "PROJECT", "-I", "Vendor/My Modules", "-enable-experimental-feature", "AvailabilityMacro=ReproOS 1.0: macOS 14.0, iOS 17.0"]"#
    );
}

fn test_configuration_project(explicit_configuration: Option<&str>) -> String {
    let mut objects = serde_json::Map::new();
    objects.insert("ROOT".into(), serde_json::json!({"isa": "PBXProject", "buildConfigurationList": "PROJECT_CONFIGS", "targets": ["APP", "UTIL", "STORE", "OTHER", "TESTS", "OTHER_TESTS"], "mainGroup": "GROUP"}));
    objects.insert("GROUP".into(), serde_json::json!({"isa": "PBXGroup", "sourceTree": "<group>", "children": ["APP_FILE", "UTIL_FILE", "STORE_FILE", "OTHER_FILE", "TESTS_FILE", "OTHER_TESTS_FILE"]}));
    objects.insert("PROJECT_CONFIGS".into(), serde_json::json!({"isa": "XCConfigurationList", "buildConfigurations": ["PROJECT_DEBUG", "PROJECT_TEST"], "defaultConfigurationName": "Debug"}));
    objects.insert("PROJECT_DEBUG".into(), serde_json::json!({"isa": "XCBuildConfiguration", "name": "Debug", "buildSettings": {"SDKROOT": "macosx", "MACOSX_DEPLOYMENT_TARGET": "14.0"}}));
    objects.insert("PROJECT_TEST".into(), serde_json::json!({"isa": "XCBuildConfiguration", "name": "Test", "buildSettings": {"SDKROOT": "macosx", "MACOSX_DEPLOYMENT_TARGET": "14.0"}}));
    let framework = "com.apple.product-type.framework";
    // The test reaches its host only through TEST_HOST, the host reaches Util
    // only through an import, and only Store's settings differ under Test.
    // OtherTests has no sources under Test, so it keeps the project
    // configuration without holding ReaderTests back.
    objects.extend(native_target(
        "APP",
        "Reader",
        "com.apple.product-type.application",
        &["STORE"],
        &[],
    ));
    objects.extend(native_target("UTIL", "Util", framework, &["STORE"], &[]));
    objects.extend(native_target(
        "STORE",
        "Store",
        framework,
        &[],
        &[("SWIFT_ACTIVE_COMPILATION_CONDITIONS", "TEST")],
    ));
    objects.extend(native_target("OTHER", "Other", framework, &[], &[]));
    objects.extend(native_target(
        "TESTS",
        "ReaderTests",
        "com.apple.product-type.bundle.unit-test",
        &[],
        &[],
    ));
    objects.extend(native_target(
        "OTHER_TESTS",
        "OtherTests",
        "com.apple.product-type.bundle.unit-test",
        &["OTHER"],
        &[("EXCLUDED_SOURCE_FILE_NAMES", "OtherTests.swift")],
    ));
    let pbxproj = serde_json::json!({"rootObject": "ROOT", "objects": objects}).to_string();
    let attrs = match explicit_configuration {
        Some(configuration) => {
            format!(r#"{{"project": "Reader.xcodeproj", "configuration": "{configuration}"}}"#)
        }
        None => r#"{"project": "Reader.xcodeproj"}"#.to_string(),
    };
    let source = package_source(&format!(
        r#"
def workspace_root():
    return "/repo"
def host_command(argv, env = None, cwd = None, merge_stderr = None, check = True):
    return {pbxproj:?}
def _xcode_scheme_paths(ctx):
    return ["Reader.xcscheme"]
SOURCES = {{"/repo/Reader.swift": "import Util\n", "/repo/ReaderTests.swift": "import XCTest\n@testable import Reader\n"}}
def host_file_exists(path):
    return path in SOURCES or path.endswith(".swift")
def host_file_read(path):
    if path == "Reader.xcscheme":
        return '<TestAction buildConfiguration = "Test"><Testables><TestableReference><BuildableReference BlueprintName = "ReaderTests"></BuildableReference></TestableReference><TestableReference><BuildableReference BlueprintName = "OtherTests"></BuildableReference></TestableReference></Testables></TestAction>'
    return SOURCES.get(path, "")
ctx = {{"label": {{"package": "", "name": "xcode", "id": "xcode"}}, "attr": {attrs}}}
resolved = _xcode_workspace_resolver(ctx)
result = repr([resolved["roots"], resolved["attrs"]["_default_test_roots"], [[spec["name"], sorted(spec["deps"]), spec["attrs"].get("module_name")] for spec in resolved["targets"]]])
"#
    ));
    eval_prelude_source_to_repr(source).unwrap()
}

#[test]
fn tests_build_with_their_scheme_test_configuration() {
    assert_eq!(
        test_configuration_project(None),
        r#"[["Reader"], ["ReaderTests", "OtherTests"], [["Reader_Test", ["./Store_Test", "./Util_Test"], None], ["Store_Test", [], "Store"], ["Util_Test", ["./Store_Test"], "Util"], ["Reader", ["./Store", "./Util"], None], ["Util", ["./Store"], None], ["Store", [], None], ["Other", [], None], ["ReaderTests", ["./Reader_Test"], None], ["OtherTests", ["./Other"], None]]]"#
    );
}

#[test]
fn an_explicit_configuration_applies_to_tests() {
    assert_eq!(
        test_configuration_project(Some("Debug")),
        r#"[["Reader"], ["ReaderTests", "OtherTests"], [["Reader", ["./Store", "./Util"], None], ["Util", ["./Store"], None], ["Store", [], None], ["Other", [], None], ["ReaderTests", ["./Reader"], None], ["OtherTests", ["./Other"], None]]]"#
    );
}

#[test]
fn generated_only_package_targets_keep_their_build_tool_dependency() {
    let source = package_source(
        r#"
graph = _xcode_local_swift_package_specs({"label": {"package": ""}}, [{"identity": "app", "path": "", "info": {"targets": [
    {"name": "App", "type": "regular", "pluginUsages": [{"plugin": ["Generator", None]}]},
]}}], "macos", "13.0", "simulator")
result = repr([[spec["kind"], spec["deps"]] for spec in graph["specs"]])
"#,
    );
    assert_eq!(
        eval_prelude_source_to_repr(source).unwrap(),
        r#"[["swift_package_build_tools", []], ["apple_library", ["./SwiftPackage_app_App_BuildTools"]], ["swift_package_build_tools", []], ["apple_library", ["./SwiftPackage_app_App_MacroHost_BuildTools"]]]"#
    );
}

#[test]
fn xcode_paths_outside_the_project_package_resolve_against_the_project_directory() {
    let pbxproj = serde_json::json!({
        "rootObject": "ROOT",
        "objects": {
            "ROOT": {"isa": "PBXProject", "buildConfigurationList": "CONFIGS", "targets": ["APP"], "mainGroup": "GROUP"},
            "GROUP": {"isa": "PBXGroup", "sourceTree": "<group>", "children": ["SOURCES_GROUP", "SHARED_GROUP"]},
            "SOURCES_GROUP": {"isa": "PBXGroup", "sourceTree": "<group>", "path": "Sources", "children": ["APP_FILE"]},
            "SHARED_GROUP": {"isa": "PBXGroup", "sourceTree": "<group>", "name": "shared", "path": "../shared", "children": ["SHARED_FILE"]},
            "APP_FILE": {"isa": "PBXFileReference", "sourceTree": "<group>", "path": "App.swift"},
            "SHARED_FILE": {"isa": "PBXFileReference", "sourceTree": "<group>", "path": "Shared.swift"},
            "APP_SOURCE": {"isa": "PBXBuildFile", "fileRef": "APP_FILE"},
            "SHARED_SOURCE": {"isa": "PBXBuildFile", "fileRef": "SHARED_FILE"},
            "SOURCES": {"isa": "PBXSourcesBuildPhase", "files": ["APP_SOURCE", "SHARED_SOURCE"]},
            "CONFIGS": {"isa": "XCConfigurationList", "buildConfigurations": ["DEBUG"], "defaultConfigurationName": "Debug"},
            "DEBUG": {"isa": "XCBuildConfiguration", "name": "Debug", "buildSettings": {
                "SDKROOT": "macosx",
                "MACOSX_DEPLOYMENT_TARGET": "14.0",
                "PRODUCT_NAME": "$(TARGET_NAME)",
                "PRODUCT_BUNDLE_IDENTIFIER": "dev.example.repro",
                "INFOPLIST_FILE": "../shared/Info.plist",
            }},
            "APP": {"isa": "PBXNativeTarget", "name": "ReproApp", "productType": "com.apple.product-type.application", "buildConfigurationList": "CONFIGS", "buildPhases": ["SOURCES"], "dependencies": []},
        },
    })
    .to_string();
    let source = format!(
        r#"{}
def workspace_root():
    return "/repo"
def host_command(argv, env = None, cwd = None, merge_stderr = None, check = True):
    return {pbxproj:?}
def host_file_exists(path):
    return path == "/repo/shared/Info.plist"
def host_file_read(path):
    if path != "/repo/shared/Info.plist":
        fail("read unexpected host file " + path)
    return "<plist><dict/></plist>"
ctx = {{"label": {{"package": "app", "name": "xcode", "id": "app/xcode"}}, "attr": {{"project": "Repro.xcodeproj"}}}}
app = [spec for spec in _xcode_workspace_resolver(ctx)["targets"] if spec["name"] == "ReproApp"][0]
target = {{"label": {{"package": "app", "name": "ReproApp", "id": "app/ReproApp"}}}}
result = repr([app["srcs"], _package_relative(target, app["attrs"]["info_plist"])])
"#,
        xcode_prelude_source()
    );
    assert_eq!(
        eval_prelude_source_to_repr(source).unwrap(),
        r#"[["Sources/App.swift", "../shared/Shared.swift"], "shared/Info.plist"]"#
    );
}

#[test]
fn xcode_remote_package_header_search_paths_follow_srcroot() {
    let project_dir = "app/Tuist/.build/tuist-derived/Projects/CLib";
    let checkout_dir = "app/Tuist/.build/checkouts/CLib";
    for (srcroot, header_root) in [
        (None, project_dir),
        (Some("../../../checkouts/CLib"), checkout_dir),
        (Some("$(PROJECT_DIR)/../../../checkouts/CLib"), checkout_dir),
        (Some("/repo/app/Tuist/.build/checkouts/CLib"), checkout_dir),
    ] {
        let mut pbxproj = serde_json::json!({
            "rootObject": "ROOT",
            "objects": {
                "ROOT": {"isa": "PBXProject", "buildConfigurationList": "PROJECT_CONFIGS", "targets": ["LIB"], "mainGroup": "GROUP"},
                "GROUP": {"isa": "PBXGroup", "sourceTree": "<group>", "children": ["FILE"]},
                "FILE": {"isa": "PBXFileReference", "sourceTree": "<group>", "path": "../../../checkouts/CLib/Sources/CLib/clib.c"},
                "BUILD_FILE": {"isa": "PBXBuildFile", "fileRef": "FILE"},
                "SOURCES": {"isa": "PBXSourcesBuildPhase", "files": ["BUILD_FILE"]},
                "PROJECT_CONFIGS": {"isa": "XCConfigurationList", "buildConfigurations": ["PROJECT_DEBUG"], "defaultConfigurationName": "Debug"},
                "PROJECT_DEBUG": {"isa": "XCBuildConfiguration", "name": "Debug", "buildSettings": {"SDKROOT": "iphonesimulator"}},
                "TARGET_CONFIGS": {"isa": "XCConfigurationList", "buildConfigurations": ["TARGET_DEBUG"], "defaultConfigurationName": "Debug"},
                "TARGET_DEBUG": {"isa": "XCBuildConfiguration", "name": "Debug", "buildSettings": {
                    "HEADER_SEARCH_PATHS": ["$(inherited)", "$(SRCROOT)/Sources/CLib/include", "$(PROJECT_DIR)/OtherHeaders", "LocalHeaders"],
                }},
                "LIB": {"isa": "PBXNativeTarget", "name": "CLib", "productType": "com.apple.product-type.library.static", "buildConfigurationList": "TARGET_CONFIGS", "buildPhases": ["SOURCES"], "dependencies": []},
            },
        });
        if let Some(srcroot) = srcroot {
            pbxproj["objects"]["PROJECT_DEBUG"]["buildSettings"]["SRCROOT"] = srcroot.into();
        }
        let pbxproj = pbxproj.to_string();
        let source = format!(
            r#"{}
def workspace_root():
    return "/repo"
def host_command(argv, env = None, cwd = None, merge_stderr = None, check = True):
    return {pbxproj:?} if argv[0] == "plutil" else "/sdk"
ctx = {{"label": {{"package": "", "name": "xcode", "id": "xcode"}}, "attr": {{"project": "{project_dir}/CLib.xcodeproj"}}}}
lib = [spec for spec in _xcode_workspace_resolver(ctx)["targets"] if spec["name"] == "CLib"][0]
result = _json_encode([lib["srcs"], lib["attrs"]["clang_flags"], lib["attrs"]["private_header_dirs"]])
"#,
            xcode_prelude_source()
        );
        let result = eval_prelude_source_to_repr(source).unwrap();
        let result: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(
            result,
            serde_json::json!([
                [format!("{checkout_dir}/Sources/CLib/clib.c")],
                [
                    "-I",
                    &format!("{header_root}/Sources/CLib/include"),
                    "-I",
                    &format!("{project_dir}/OtherHeaders"),
                    "-I",
                    &format!("{project_dir}/LocalHeaders")
                ],
                [
                    format!("{checkout_dir}/Sources/CLib"),
                    format!("{header_root}/Sources/CLib/include"),
                    format!("{project_dir}/OtherHeaders"),
                    format!("{project_dir}/LocalHeaders")
                ],
            ]),
            "SRCROOT = {srcroot:?}"
        );
    }
}

#[test]
fn apple_inputs_outside_the_workspace_are_snapshotted_into_the_build_directory() {
    let root = TempDir::new().unwrap();
    let workspace = root.path().join("app");
    std::fs::create_dir_all(workspace.join("Sources")).unwrap();
    std::fs::create_dir_all(root.path().join("shared")).unwrap();
    std::fs::write(
        workspace.join("Sources/main.swift"),
        "print(sharedGreeting)\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join("shared/Shared.swift"),
        "let sharedGreeting = \"\"\n",
    )
    .unwrap();
    std::fs::write(root.path().join("shared/Shared.h"), "\n").unwrap();
    let source = format!(
        r#"{}
ctx = {{"label": {{"package": "", "name": "ReproApp", "id": "ReproApp"}}, "attr": {{}}, "srcs": ["Sources/main.swift", "../shared/Shared.swift"], "build_dir": ".struo/out/ReproApp"}}
result = repr([_apple_declared_source_paths(ctx), _apple_header_inputs(ctx, ["../shared"])])
"#,
        apple_prelude_source()
    );
    let (store, result) = with_active_store(store_for(&workspace, ""), || {
        eval_prelude_source_to_repr(source)
    });
    let result = result.unwrap();
    assert!(!result.contains(".."), "{result}");
    let shared = std::fs::canonicalize(root.path().join("shared")).unwrap();
    let sources = store
        .actions
        .iter()
        .filter_map(|action| match &action.operation {
            Some(
                DeclaredActionOperation::MaterializeHostFile {
                    source,
                    destination,
                    ..
                }
                | DeclaredActionOperation::MaterializeHostTree {
                    source,
                    destination,
                    ..
                },
            ) => {
                assert!(
                    struo_core::WorkspacePath::try_from(destination.as_str()).is_ok(),
                    "{destination}"
                );
                assert!(
                    result.contains(destination.as_str()),
                    "{result} lacks {destination}"
                );
                Some(std::fs::canonicalize(source).unwrap())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        sources,
        vec![shared.join("Shared.swift"), shared.join("Shared.h")]
    );
}

#[test]
fn apple_resources_and_asset_catalogs_outside_the_workspace_are_snapshotted() {
    let root = TempDir::new().unwrap();
    let workspace = root.path().join("app");
    let shared = root.path().join("shared");
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::create_dir_all(shared.join("Folder/Nested")).unwrap();
    std::fs::create_dir_all(shared.join("Assets.xcassets")).unwrap();
    std::fs::write(shared.join("greeting.txt"), "hello\n").unwrap();
    std::fs::write(shared.join("Folder/Nested/one.txt"), "one\n").unwrap();
    std::fs::write(shared.join("Assets.xcassets/Contents.json"), "{}\n").unwrap();
    let source = format!(
        r#"{}
ctx = {{"label": {{"package": "", "name": "ReproApp", "id": "ReproApp"}}, "attr": {{}}, "build_dir": ".struo/out/ReproApp"}}
files = _apple_materialize_resources(ctx, ["../shared/greeting.txt", "../shared/Folder"], "ReproApp.app/Contents/Resources", "macos", "14.0", "", "ReproApp", "apple_app_ReproApp")
catalog = _apple_external_input(ctx, _apple_resource_path(ctx, "../shared/Assets.xcassets"))
result = repr([sorted(files), catalog, _apple_is_directory(catalog)])
"#,
        apple_prelude_source()
    );
    let (store, result) = with_active_store(store_for(&workspace, ""), || {
        eval_prelude_source_to_repr(source)
    });
    let result = result.unwrap();
    assert_eq!(
        result,
        r#"[[".struo/out//ReproApp.app/Contents/Resources/Nested/one.txt", ".struo/out//ReproApp.app/Contents/Resources/greeting.txt"], ".struo/out//external-inputs/__parent__/shared/Assets.xcassets", True]"#
    );
    for action in &store.actions {
        for path in action.inputs.iter().chain(action.outputs.iter()) {
            assert!(
                struo_core::WorkspacePath::try_from(path.as_str()).is_ok(),
                "{path}"
            );
        }
    }
    let mut snapshots = store
        .actions
        .iter()
        .filter_map(|action| match &action.operation {
            Some(
                DeclaredActionOperation::MaterializeHostFile { source, .. }
                | DeclaredActionOperation::MaterializeHostTree { source, .. },
            ) => Some(std::fs::canonicalize(source).unwrap()),
            _ => None,
        })
        .collect::<Vec<_>>();
    snapshots.sort();
    let shared = std::fs::canonicalize(shared).unwrap();
    assert_eq!(
        snapshots,
        vec![
            shared.join("Assets.xcassets"),
            shared.join("Folder"),
            shared.join("greeting.txt"),
        ]
    );
}

#[test]
fn snapshot_paths_map_back_only_their_leading_parent_segments() {
    let source = format!(
        r#"{}
def workspace_root():
    return "/repo/app"
result = repr([
    _apple_host_path(".struo/out/App/external-inputs/__parent__/__parent__/shared/__parent__/a.png"),
    _apple_host_path(".struo/out/App/external-inputs/shared/a.png"),
    _apple_host_path("Sources/external-inputs/__parent__/a.png"),
])
"#,
        apple_prelude_source()
    );
    assert_eq!(
        eval_prelude_source_to_repr(source).unwrap(),
        r#"["/repo/app/../../shared/__parent__/a.png", "/repo/app/.struo/out/App/external-inputs/shared/a.png", "/repo/app/Sources/external-inputs/__parent__/a.png"]"#
    );
}
