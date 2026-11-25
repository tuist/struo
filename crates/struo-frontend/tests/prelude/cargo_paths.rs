use super::*;

fn workspace_library(workspace: &str, owner: &str, package_root: &str) -> Result<String, String> {
    let source = format!(
        r#"{}
package = {{
    "id": "app", "name": "app", "version": "0.1.0", "edition": "2024",
    "manifest_path": {manifest:?},
    "targets": [{{"name": "app", "kind": ["lib"], "src_path": {src:?}}}],
}}
def _cargo_resolved_metadata(ctx, default_vendor_dir, materialize_sources):
    return {{"metadata": {{"packages": [package], "workspace_members": ["app"]}}, "specs": [], "split_host": False}}

ctx = {{
    "label": {{"package": {owner:?}, "name": "cargo", "id": "cargo"}},
    "attr": {{}}, "files": {{"Cargo.toml": "[workspace]"}},
}}
target = _cargo_workspace_resolver(ctx)["targets"][0]
result = repr([target["srcs"], target["attrs"]["crate_root"]])
"#,
        all_prelude_source(),
        manifest = format!("{package_root}/Cargo.toml"),
        src = format!("{package_root}/src/lib.rs"),
    );
    let store = store_for(Path::new(workspace), owner);
    with_active_store(store, || eval_prelude_source_to_repr(source)).1
}

#[test]
fn cargo_coverage_opt_in_reaches_generated_tests_and_product_compilation() {
    let source = format!(
        r#"{}
package = {{
    "id": "app", "name": "app", "version": "0.1.0", "edition": "2024",
    "manifest_path": "/workspace/Cargo.toml",
    "targets": [{{"name": "app", "kind": ["lib"], "src_path": "/workspace/src/lib.rs", "test": True}}],
}}
def _cargo_resolved_metadata(ctx, default_vendor_dir, materialize_sources):
    return {{"metadata": {{"packages": [package], "workspace_members": ["app"]}}, "specs": [], "split_host": False}}
def configurations():
    results = []
    for enabled in [False, True]:
        ctx = {{"label": {{"package": "", "name": "cargo", "id": "cargo"}},
            "attr": {{"coverage_per_test": enabled, "coverage_sources": ["src/product.rs"], "llvm_cov": "cov", "llvm_profdata": "profdata"}},
            "files": {{"Cargo.toml": "[workspace]"}}}}
        targets = _cargo_workspace_resolver(ctx)["targets"]
        results.append([[target["kind"], target["attrs"].get("coverage"), target["attrs"].get("coverage_per_test"),
            target["attrs"].get("coverage_sources"), target["attrs"].get("llvm_cov"), target["attrs"].get("llvm_profdata")] for target in targets])
    return results
result = repr(configurations())
"#,
        all_prelude_source()
    );
    let result = with_active_store(store_for(Path::new("/workspace"), ""), || {
        eval_prelude_source_to_repr(source)
    })
    .1
    .unwrap();
    assert_eq!(
        result,
        r#"[[["rust_library", None, None, None, None, None], ["rust_test", None, None, None, None, None]], [["rust_library", True, None, None, None, None], ["rust_test", None, True, ["src/product.rs"], "cov", "profdata"]]]"#
    );
}

#[test]
fn cargo_workspace_accepts_windows_verbatim_and_metadata_path_spellings() {
    let drive = r"D:\a\_temp\desktop-package\source";
    let verbatim_drive = r"\\?\D:\a\_temp\desktop-package\source";
    let unc = r"\\server\share\desktop-package\source";
    let verbatim_unc = r"\\?\UNC\server\share\desktop-package\source";
    let long_member = format!("crates/{}", "member".repeat(50));
    for (workspace, metadata_root, owner, member) in [
        (drive, drive, "", "crates/credentials"),
        (verbatim_drive, drive, "", "crates/credentials"),
        (drive, verbatim_drive, "", "crates/credentials"),
        (verbatim_drive, verbatim_drive, "", "crates/credentials"),
        (unc, unc, "", "crates/credentials"),
        (verbatim_unc, unc, "", "crates/credentials"),
        (unc, verbatim_unc, "", "crates/credentials"),
        (verbatim_unc, verbatim_unc, "", "crates/credentials"),
        (verbatim_drive, drive, "apps/desktop", ""),
        (verbatim_unc, unc, "apps/desktop", long_member.as_str()),
        (verbatim_drive, drive, "", ""),
        ("/workspace", "/workspace", "apps/desktop", "member"),
    ] {
        let project_root = if owner.is_empty() {
            metadata_root.to_string()
        } else {
            format!("{metadata_root}/{owner}")
        };
        let package_root = if member.is_empty() {
            project_root
        } else {
            format!("{project_root}/{member}")
        };
        let root = if member.is_empty() { "." } else { member };
        let crate_root = if member.is_empty() {
            "src/lib.rs".to_string()
        } else {
            format!("{member}/src/lib.rs")
        };
        assert_eq!(
            workspace_library(workspace, owner, &package_root).unwrap(),
            format!(r#"[["{root}/**/*"], "{crate_root}"]"#),
            "workspace={workspace}, owner={owner}, package={package_root}",
        );
    }
}

#[test]
fn cargo_materialized_sources_keep_verbatim_paths_but_compare_source_spellings() {
    for (root, source_root) in [
        (r"\\?\D:\cargo", "D:/cargo"),
        (r"\\?\UNC\server\share\cargo", "//server/share/cargo"),
    ] {
        let member = "long-package/".repeat(25);
        let source = format!(
            r#"{}
package = {{
    "id": "dependency", "name": "dependency", "version": "1.0.0",
    "source": "registry+https://github.com/rust-lang/crates.io-index",
    "manifest_path": {manifest:?},
    "targets": [{{"name": "dependency", "kind": ["lib"], "src_path": {src:?}}}],
}}
ctx = {{"label": {{"package": "", "id": "cargo"}}, "attrs": {{"split_host_variants": False}}}}
metadata = {{"packages": [package], "workspace_members": ["app"]}}
attrs = _cargo_metadata_resolution(ctx, metadata, materialize_sources = True)["specs"][0]["attrs"]
result = repr([attrs["_cargo_source_root"], attrs["crate_root"]])
"#,
            all_prelude_source(),
            manifest = format!("{root}/{member}Cargo.toml"),
            src = format!("{source_root}/{member}lib/entry.rs"),
        );
        let preserved_root = format!("{root}/{member}").replace('\\', "/");
        let preserved_root = preserved_root.trim_end_matches('/');
        assert_eq!(
            eval_prelude_source_to_repr(source).unwrap(),
            format!(r#"["{preserved_root}", ".struo/out/dependency-1.0.0/source/lib/entry.rs"]"#),
        );
    }
}

#[test]
fn cargo_workspace_loads_path_dependencies_from_a_canonical_root() {
    let workspace = TempDir::new().unwrap();
    for (path, contents) in [
        ("Cargo.toml", "[workspace]\nmembers = ['crates/*']\nresolver = '2'\n"),
        ("crates/app/Cargo.toml", "[package]\nname = 'app'\nversion = '0.1.0'\n[dependencies]\ncredentials = { path = '../credentials' }\n"),
        ("crates/credentials/Cargo.toml", "[package]\nname = 'credentials'\nversion = '0.1.0'\n"),
        ("crates/app/src/lib.rs", "pub fn answer() -> u32 { credentials::answer() }\n"),
        ("crates/credentials/src/lib.rs", "pub fn answer() -> u32 { 42 }\n"),
        ("struo.toml", "[workspace]\ninclude = ['struo.toml']\nexclude = ['**/Cargo.toml']\n[[target]]\nname = 'Desktop'\nkind = 'cargo_workspace'\nsrcs = ['Cargo.toml', 'Cargo.lock', 'crates/*/Cargo.toml', 'crates/*/src/**/*.rs']\n"),
    ] {
        let path = workspace.path().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }
    let output = Command::new("cargo")
        .args(["generate-lockfile", "--offline"])
        .current_dir(workspace.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let root = workspace.path().canonicalize().unwrap();
    let graph = struo_frontend::load_graph_workspace(&root).unwrap();
    let app = graph
        .iter()
        .find(|target| target.label.id == "cargo_app")
        .unwrap();
    assert_eq!(app.deps, ["cargo_credentials"]);
    let credentials = graph
        .iter()
        .find(|target| target.label.id == "cargo_credentials")
        .unwrap();
    assert_eq!(credentials.srcs, ["crates/credentials/**/*"]);
}

#[test]
fn cargo_workspace_still_rejects_packages_outside_the_project() {
    for (workspace, owner, package_root) in [
        (r"\\?\D:\source", "", "D:/source-other/crate"),
        (r"\\?\D:\source", "apps/desktop", "D:/source/crates/app"),
        (r"\\?\D:\source", "", "E:/source/crate"),
        (
            r"\\?\UNC\server\share\source",
            "",
            "//other/share/source/crate",
        ),
        (
            r"\\?\UNC\server\share\source",
            "",
            "//server/other/source/crate",
        ),
        (r"\\?\Volume{abc}\source", "", "Volume{abc}/source/crate"),
        ("/workspace", "", "/Workspace/crate"),
    ] {
        let error = workspace_library(workspace, owner, package_root).unwrap_err();
        assert!(
            error.contains("outside the adapted project root"),
            "{error}"
        );
    }
}

#[test]
fn cargo_first_party_roots_stay_relative_to_the_owning_package() {
    let source = format!(
        r#"{}
package = {{
    "name": "app", "version": "0.1.0", "edition": "2024",
    "manifest_path": "/workspace/apps/desktop/Cargo.toml",
    "targets": [{{"kind": ["custom-build"], "src_path": "/workspace/apps/desktop/build.rs"}}],
}}
target = {{"name": "app", "kind": ["bin"], "src_path": "/workspace/apps/desktop/src/main.rs"}}
def check_roots():
    results = []
    for owner in ["", "apps/desktop"]:
        for root in [".", "member"]:
            attrs = _cargo_workspace_attrs(package, target, {{"features": []}}, root, {{}})
            ctx = {{"label": {{"package": owner, "id": "app"}}, "attr": attrs}}
            results.append([
                attrs["crate_root"],
                _rust_crate_root(ctx, "src/main.rs"),
                _package_relative(ctx, attrs["build_script"]),
            ])
    return results
result = repr(check_roots())
"#,
        all_prelude_source()
    );
    assert_eq!(
        eval_prelude_source_to_repr(source).unwrap(),
        r#"[["src/main.rs", "src/main.rs", "build.rs"], ["member/src/main.rs", "member/src/main.rs", "member/build.rs"], ["src/main.rs", "apps/desktop/src/main.rs", "apps/desktop/build.rs"], ["member/src/main.rs", "apps/desktop/member/src/main.rs", "apps/desktop/member/build.rs"]]"#
    );
}

fn root_package_workspace(manifest: &str, build_script_data: &str) -> Result<String, String> {
    let source = format!(
        r#"{}
def package(name, root, kinds):
    return {{
        "id": name, "name": name, "version": "0.1.0", "edition": "2021",
        "manifest_path": "/workspace/" + root + "Cargo.toml",
        "targets": [{{"name": name, "kind": [kind], "src_path": "/workspace/" + root + path, "test": kind == "lib"}} for kind, path in kinds],
    }}
packages = [
    package("app", "", [("lib", "src/lib.rs"), ("custom-build", "build.rs")]),
    package("util", "crates/util/", [("lib", "src/lib.rs")]),
    package("leaf", "crates/util/leaf/", [("lib", "src/lib.rs")]),
    package("derive", "crates/derive/", [("proc-macro", "src/lib.rs")]),
]
def _cargo_resolved_metadata(ctx, default_vendor_dir, materialize_sources):
    return {{
        "metadata": {{"packages": packages, "workspace_members": ["app", "util", "leaf", "derive"], "target_directory": "/workspace/build/cargo"}},
        "specs": [
            {{"name": "serde", "kind": "rust_crate", "attrs": {{"package_name": "serde", "version": "1.0.0"}}}},
            {{"name": "zstd_sys", "kind": "rust_crate", "attrs": {{"package_name": "zstd-sys", "version": "2.0.0"}}}},
            {{"name": "quote", "kind": "rust_proc_macro", "attrs": {{"package_name": "quote", "version": "1.0.0"}}}},
        ],
        "split_host": False,
    }}
ctx = {{
    "label": {{"package": "", "name": "cargo", "id": "cargo"}},
    "attr": {{"build_script_data": {build_script_data}}},
    "files": {{
        "Cargo.toml": {manifest:?},
        "crates/util/Cargo.toml": "", "crates/util/leaf/Cargo.toml": "", "crates/derive/Cargo.toml": "",
        "crates/util/fixtures/sample/Cargo.toml": "",
        ".cargo/config.toml": "[profile.dev.package.util]\ncodegen-units = 1\n",
    }},
}}
result = repr([
    [target["name"], target["attrs"].get("srcs_exclude"), target["attrs"].get("_cargo_profile_flags"), target["attrs"].get("build_script_data")]
    for target in _cargo_workspace_resolver(ctx)["targets"]
])
"#,
        all_prelude_source(),
    );
    with_active_store(store_for(Path::new("/workspace"), ""), || {
        eval_prelude_source_to_repr(source)
    })
    .1
}

const ROOT_EXCLUDES: &str =
    r#"[".git/**", "build/cargo/**", "crates/derive/**", "crates/util/**", "target/**"]"#;

#[test]
fn cargo_workspace_bounds_each_package_by_cargo_rules() {
    let result = root_package_workspace("[workspace]", "{}").unwrap();
    for expected in [
        format!(r#"["cargo_app", {ROOT_EXCLUDES}, None, None]"#),
        // A crate no manifest refers to, such as a test fixture, stays an
        // input of the package around it.
        r#"["cargo_util", ["crates/util/leaf/**", "crates/util/target/**"], ["-Ccodegen-units=1"], None]"#.to_string(),
        r#"["cargo_leaf", ["crates/util/leaf/target/**"], None, None]"#.to_string(),
    ] {
        assert!(result.contains(&expected), "missing {expected} in {result}");
    }
}

#[test]
fn cargo_dev_profile_reaches_workspace_and_external_crates() {
    let manifest = r#"[workspace]
[profile.dev]
debug = "line-tables-only"
[profile.dev.package."*"]
opt-level = 2
[profile.dev.package.app]
opt-level = 3
[profile.dev.package.zstd-sys]
opt-level = "s"
debug-assertions = false
[profile.dev.build-override]
opt-level = 1
overflow-checks = false
"#;
    let result = root_package_workspace(manifest, "{}").unwrap();
    for expected in [
        // `package."*"` reaches dependencies but not workspace members, and
        // rustc ties overflow checks to debug assertions.
        r#"["serde", None, ["-Copt-level=2", "-Cdebuginfo=line-tables-only", "-Cdebug-assertions=on"], None]"#.to_string(),
        // Turning debug assertions off keeps overflow checks on, as Cargo does.
        r#"["zstd_sys", None, ["-Copt-level=s", "-Cdebuginfo=line-tables-only", "-Coverflow-checks=on"], None]"#.to_string(),
        // Host crates drop the base debug info; `"*"` outranks `build-override`.
        r#"["quote", None, ["-Copt-level=2", "-Cdebuginfo=0", "-Cdebug-assertions=on", "-Coverflow-checks=off"], None]"#.to_string(),
        format!(r#"["cargo_app", {ROOT_EXCLUDES}, ["-Copt-level=3", "-Cdebuginfo=line-tables-only", "-Cdebug-assertions=on"], None]"#),
        r#"["cargo_leaf", ["crates/util/leaf/target/**"], ["-Cdebuginfo=line-tables-only"], None]"#.to_string(),
        // A workspace proc macro takes `build-override` but not `"*"`.
        r#"["cargo_derive", ["crates/derive/target/**"], ["-Copt-level=1", "-Cdebuginfo=0", "-Cdebug-assertions=on", "-Coverflow-checks=off"], None]"#.to_string(),
    ] {
        assert!(result.contains(&expected), "missing {expected} in {result}");
    }
}

#[test]
fn cargo_host_crates_stay_unoptimized_and_test_targets_use_the_test_profile() {
    let manifest = r#"[workspace]
[profile.dev]
opt-level = 3
codegen-units = 4
[profile.dev.package."serde@1.0.0"]
debug = 0
[profile.dev.package."serde@9.0.0"]
opt-level = 1
[profile.test]
opt-level = 1
"#;
    let result = root_package_workspace(manifest, "{}").unwrap();
    for expected in [
        // Cargo's built-in `build-override` resets the optimization level,
        // codegen units, and debug info of proc macros, external and
        // workspace alike.
        r#"["quote", None, ["-Cdebuginfo=0"], None]"#.to_string(),
        r#"["cargo_derive", ["crates/derive/target/**"], ["-Cdebuginfo=0"], None]"#.to_string(),
        // A `name@version` override applies only to that version.
        r#"["serde", None, ["-Copt-level=3", "-Cdebuginfo=0", "-Cdebug-assertions=on", "-Ccodegen-units=4"], None]"#.to_string(),
        format!(r#"["cargo_app", {ROOT_EXCLUDES}, ["-Copt-level=3", "-Cdebug-assertions=on", "-Ccodegen-units=4"], None]"#),
        format!(r#"["cargo_app_unit_tests", {ROOT_EXCLUDES}, ["-Copt-level=1", "-Cdebug-assertions=on", "-Ccodegen-units=4"], None]"#),
    ] {
        assert!(result.contains(&expected), "missing {expected} in {result}");
    }
}

#[test]
fn cargo_build_script_data_reaches_only_the_named_build_script_package() {
    let result = root_package_workspace("[workspace]", r#"{"app": ["proto/**"]}"#).unwrap();
    assert!(
        result.contains(&format!(
            r#"["cargo_app", {ROOT_EXCLUDES}, None, ["proto/**"]]"#
        )),
        "{result}"
    );
    assert!(
        result.contains(r#"["cargo_util", ["crates/util/leaf/**", "crates/util/target/**"], ["-Ccodegen-units=1"], None]"#),
        "{result}"
    );

    let error = root_package_workspace("[workspace]", r#"{"utill": ["proto/**"]}"#).unwrap_err();
    assert!(error.contains("cargo.build_script_data"), "{error}");
    assert!(error.contains("`utill`"), "{error}");
    assert!(error.contains("Use one of: app"), "{error}");
}
