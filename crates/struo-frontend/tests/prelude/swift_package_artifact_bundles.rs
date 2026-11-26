use tempfile::TempDir;

use super::{eval_prelude_source_to_repr, store_for, with_active_store, xcode_prelude_source};

const CHECKSUM: &str = "de2ba0a4b8205475efa5815e27439e83204b106b28815a9d7707fd06d8c256b3";

/// Writes the layout of the reported `CHegel` artifact bundle: one static
/// library per triple, shared headers, and a module map named by info.json.
fn write_bundle(root: &std::path::Path) {
    let bundle = root.join("CHegel.artifactbundle");
    std::fs::create_dir_all(bundle.join("include")).unwrap();
    for triple in ["arm64-apple-macosx", "x86_64-unknown-linux-gnu"] {
        std::fs::create_dir_all(bundle.join(triple)).unwrap();
        std::fs::write(bundle.join(triple).join("libhegel_c.a"), "!<arch>\n").unwrap();
    }
    std::fs::write(bundle.join("include/hegel.h"), "int hegel(void);\n").unwrap();
    std::fs::write(
        bundle.join("include/module.modulemap"),
        "module CHegel {\n  header \"hegel.h\"\n  export *\n}\n",
    )
    .unwrap();
    let variant = |triple: &str| {
        serde_json::json!({
            "path": format!("{triple}/libhegel_c.a"),
            "supportedTriples": [triple],
            "staticLibraryMetadata": {
                "headerPaths": ["include"],
                "moduleMapPath": "include/module.modulemap",
            },
        })
    };
    let info = serde_json::json!({
        "schemaVersion": "1.0",
        "artifacts": {
            "CHegel": {
                "type": "staticLibrary",
                "version": "0.41.0",
                "variants": [variant("x86_64-unknown-linux-gnu"), variant("arm64-apple-macosx")],
            },
        },
    });
    std::fs::write(bundle.join("info.json"), info.to_string()).unwrap();
}

/// A binary target whose archive is a Swift Package Manager artifact bundle
/// must not be lowered as an `.xcframework`: the bundle has `info.json`, not
/// `Info.plist`.
#[test]
fn binary_targets_backed_by_artifact_bundles_lower_to_artifact_bundle_imports() {
    let workspace = TempDir::new().unwrap();
    write_bundle(&workspace.path().join("Packages/Local"));
    let source = format!(
        r#"{prelude}
def info(target):
    return {{"products": [], "targets": [dict({{"name": "CHegel", "type": "binary"}}, **target)]}}
infos = [
    {{"identity": "hegel-swift", "path": "Packages/Remote", "info": info({{
        "url": "https://github.com/Semantical/hegel-swift/releases/download/artifacts-0.41.0/CHegel-0.41.0.artifactbundle.zip",
        "checksum": "{CHECKSUM}",
    }})}},
    {{"identity": "local", "path": "Packages/Local", "info": info({{"path": "CHegel.artifactbundle"}})}},
]
graph = _xcode_local_swift_package_specs({{"label": {{"package": "", "id": "root"}}, "attr": {{}}}}, infos, "macos", "13.0", "simulator")
result = repr([[spec["name"], spec["kind"], spec["deps"], spec["attrs"].get("bundle")] for spec in graph["specs"]])
"#,
        prelude = xcode_prelude_source(),
    );
    let (_, result) = with_active_store(store_for(workspace.path(), ""), || {
        eval_prelude_source_to_repr(source)
    });
    assert_eq!(
        result.unwrap(),
        r#"[["SwiftPackage_hegel-swift_CHegel_Artifact", "archive_download", [], None], ["SwiftPackage_hegel-swift_CHegel", "apple_artifact_bundle_import", ["./SwiftPackage_hegel-swift_CHegel_Artifact"], ".struo/out/SwiftPackage_hegel-swift_CHegel_Artifact/archive"], ["SwiftPackage_local_CHegel", "apple_artifact_bundle_import", [], "Packages/Local/CHegel.artifactbundle"]]"#
    );
}

/// The import reads info.json, selects the variant whose triple matches the
/// destination, and exposes that static library with the bundle's headers
/// and module map, whether `bundle` names the bundle or the extracted
/// archive that contains it.
#[test]
fn artifact_bundle_import_selects_the_destination_variant() {
    let workspace = TempDir::new().unwrap();
    let archive = ".struo/out/CHegel_Artifact/archive";
    write_bundle(&workspace.path().join(archive));
    let source = format!(
        r#"{prelude}
def import_bundle(bundle, platform = "macos", arch = "arm64"):
    provider = _apple_artifact_bundle_import_impl({{
        "label": {{"package": "", "name": "CHegel", "id": "CHegel"}},
        "attr": {{"bundle": bundle, "platform": platform, "sdk_variant": "simulator", "arch": arch}},
        "configuration": {{"tokens": []}},
        "deps": [],
        "srcs": [],
        "build_dir": ".struo/out/CHegel",
        "capability": "build",
    }})
    return [
        provider["framework_module_name"],
        provider["transitive_archives"],
        provider["transitive_modulemaps"],
        provider["transitive_exported_header_dirs"],
        sorted(provider["framework_files"]),
    ]
result = repr([
    import_bundle("{archive}"),
    import_bundle("{archive}/CHegel.artifactbundle"),
])
"#,
        prelude = xcode_prelude_source(),
    );
    let (_, result) = with_active_store(store_for(workspace.path(), ""), || {
        eval_prelude_source_to_repr(source)
    });
    let bundle = format!("{archive}/CHegel.artifactbundle");
    let expected = format!(
        r#"["CHegel", ["{bundle}/arm64-apple-macosx/libhegel_c.a"], ["{bundle}/include/module.modulemap"], ["{bundle}/include"], ["{bundle}/arm64-apple-macosx/libhegel_c.a", "{bundle}/include/hegel.h", "{bundle}/include/module.modulemap"]]"#
    );
    assert_eq!(result.unwrap(), format!("[{expected}, {expected}]"));
}

/// A destination no variant supports fails during analysis with the
/// triples the bundle does offer, instead of picking another platform's
/// library.
#[test]
fn artifact_bundle_import_reports_unsupported_destinations() {
    let workspace = TempDir::new().unwrap();
    write_bundle(workspace.path());
    let source = format!(
        r#"{prelude}
_apple_artifact_bundle_import_impl({{
    "label": {{"package": "", "name": "CHegel", "id": "CHegel"}},
    "attr": {{"bundle": "CHegel.artifactbundle", "platform": "ios", "sdk_variant": "simulator", "arch": "arm64"}},
    "configuration": {{"tokens": []}},
    "deps": [],
    "srcs": [],
    "build_dir": ".struo/out/CHegel",
    "capability": "build",
}})
result = ""
"#,
        prelude = xcode_prelude_source(),
    );
    let (_, result) = with_active_store(store_for(workspace.path(), ""), || {
        eval_prelude_source_to_repr(source)
    });
    let error = result.unwrap_err();
    assert!(
        error.contains("has no variant for arm64-apple-ios-simulator")
            && error.contains("arm64-apple-macosx, x86_64-unknown-linux-gnu"),
        "{error}"
    );
}
