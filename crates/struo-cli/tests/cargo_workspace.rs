use std::process::{Command, Output};

fn workspace() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    for (path, contents) in [
        (
            "Cargo.toml",
            r#"[package]
name = "derive_demo"
version = "0.1.0"
edition = "2021"
[lib]
proc-macro = true
[dependencies]
helper = { path = "helper" }
shared = { path = "shared" }
[workspace]
members = ["helper", "shared", "leaf"]
resolver = "2"
"#,
        ),
        (
            "src/lib.rs",
            r"use proc_macro::TokenStream;
#[proc_macro]
pub fn identity(input: TokenStream) -> TokenStream { input }
#[test]
fn dependency_values() { assert_eq!(helper::value() + shared::value(), 42); }
",
        ),
        (
            "helper/Cargo.toml",
            "[package]\nname = \"helper\"\nversion = \"0.1.0\"\n[dependencies]\nshared = { path = \"../shared\" }\n",
        ),
        ("helper/src/lib.rs", "pub fn value() -> u8 { shared::value() }\n"),
        (
            "shared/Cargo.toml",
            "[package]\nname = \"shared\"\nversion = \"0.1.0\"\n[features]\ndefault = [\"extra\"]\nextra = []\n[dependencies]\nleaf = { path = \"../leaf\" }\n",
        ),
        ("shared/src/lib.rs", "pub fn value() -> u8 { 21 }\n"),
        (
            "leaf/Cargo.toml",
            "[package]\nname = \"leaf\"\nversion = \"0.1.0\"\n",
        ),
        ("leaf/src/lib.rs", ""),
    ] {
        let path = root.path().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }
    root
}

fn struo(root: &std::path::Path, color: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_struo"))
        .current_dir(root)
        .args(["--format", "json", "--cpu-limit", "1"])
        .args(args)
        .env("CARGO_TERM_COLOR", color)
        .env("CARGO", env!("CARGO"))
        .env("STRUO_CACHE_PROVIDER", "local")
        .env("STRUO_CHANGE_TRACKER", "0")
        .output()
        .unwrap()
}

fn success(output: &Output) {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn cargo_feature_resolution_is_independent_of_terminal_color() {
    let root = workspace();
    let mut features = Vec::new();
    for color in ["always", "never"] {
        let output = struo(root.path(), color, &["query", "target", "cargo_shared"]);
        success(&output);
        let target: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        features.push(target["attrs"]["features"].clone());
    }
    assert_eq!(features[0], serde_json::json!(["default", "extra"]));
    assert_eq!(features[0], features[1]);
}

#[test]
fn cargo_proc_macro_unit_tests_can_import_the_compiler_builtin() {
    let root = workspace();
    let output = struo(
        root.path(),
        "never",
        &["test", "cargo_derive_demo_unit_tests"],
    );
    success(&output);
    let output = struo(
        root.path(),
        "never",
        &["query", "test-results", "cargo_derive_demo_unit_tests"],
    );
    success(&output);
    let results: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(results["summary"]["passed"], 1);

    let source = root.path().join("src/lib.rs");
    let original = std::fs::read_to_string(&source).unwrap();
    std::fs::write(
        source,
        format!("{original}\n#[test]\nfn changed_test_fails() {{ panic!(\"invalidation control\"); }}\n"),
    )
    .unwrap();
    let output = struo(
        root.path(),
        "never",
        &["test", "cargo_derive_demo_unit_tests"],
    );
    assert!(!output.status.success());
    let output = struo(
        root.path(),
        "never",
        &["query", "test-results", "cargo_derive_demo_unit_tests"],
    );
    success(&output);
    let results: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(results["summary"]["passed"], 1);
    assert_eq!(results["summary"]["failed"], 1);
}

#[test]
fn cargo_workspace_can_explicitly_run_host_dependent_tests_without_caching() {
    let root = workspace();
    std::fs::write(
        root.path().join("struo.toml"),
        r#"[[target]]
name = "cargo"
kind = "cargo_workspace"
srcs = ["Cargo.toml"]
[target.attrs]
test_env = { STRUO_TEST_VALUE = "declared" }
test_env_inherit = ["CARGO"]
test_cacheable = false
"#,
    )
    .unwrap();
    let source = root.path().join("src/lib.rs");
    let original = std::fs::read_to_string(&source).unwrap();
    std::fs::write(
        source,
        format!(
            r#"{original}
#[test]
fn host_dependent_test() {{
    assert_eq!(std::env::var("STRUO_TEST_VALUE").unwrap(), "declared");
    let cargo = std::env::var_os("CARGO").unwrap();
    assert!(std::process::Command::new(cargo).arg("--version").status().unwrap().success());
}}
"#
        ),
    )
    .unwrap();
    for _ in 0..2 {
        let output = struo(
            root.path(),
            "never",
            &["test", "cargo_derive_demo_unit_tests"],
        );
        success(&output);
        assert!(String::from_utf8_lossy(&output.stdout).contains("\"cache\":\"bypass\""));
    }
    let output = struo(
        root.path(),
        "never",
        &["query", "test-results", "cargo_derive_demo_unit_tests"],
    );
    success(&output);
    let results: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(results["summary"]["passed"], 2);
}

fn write_files(root: &std::path::Path, files: &[(&str, &str)]) {
    for (path, contents) in files {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }
}

fn cache_state(output: &Output, target: &str) -> String {
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find(|record| record["target"] == target)
        .and_then(|record| record["cache"].as_str().map(str::to_string))
        .unwrap_or_else(|| {
            panic!(
                "no record for {target}: {}",
                String::from_utf8_lossy(&output.stdout)
            )
        })
}

#[test]
fn root_package_rebuilds_for_what_it_reads_not_for_its_whole_tree() {
    let root = tempfile::tempdir().unwrap();
    write_files(
        root.path(),
        &[
            (
                "Cargo.toml",
                "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
            ),
            (".git/HEAD", "ref: refs/heads/main\n"),
            // A crate that no manifest refers to, such as a test fixture.
            ("fixtures/sample/Cargo.toml", "[package]\nname = \"sample\"\n"),
            ("fixtures/sample/value.txt", "one\n"),
            ("dev/example.toml", "port = 1\n"),
            ("docs/guide.md", "# Guide\n"),
            (
                "build.rs",
                r#"use std::path::Path;
fn main() {
    let out = Path::new(&std::env::var("OUT_DIR").unwrap()).join("value.rs");
    let value = std::fs::read_to_string("fixtures/sample/value.txt").unwrap();
    let head = std::fs::read_to_string(".git/HEAD").unwrap();
    std::fs::write(&out, format!("pub const VALUE: &str = {:?};", format!("{} {}", value.trim(), head.trim()))).unwrap();
    println!("cargo:rerun-if-changed=fixtures/sample/value.txt");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed={}", out.display());
}
"#,
            ),
            (
                "src/main.rs",
                "include!(concat!(env!(\"OUT_DIR\"), \"/value.rs\"));\nfn main() { println!(\"{VALUE} {}\", include_str!(\"../dev/example.toml\").trim()); }\n",
            ),
        ],
    );
    let build = || {
        let output = struo(root.path(), "never", &["build", "cargo_app_bin_app"]);
        success(&output);
        let binary = root.path().join(".struo/out/cargo_app_bin_app/app");
        let printed = String::from_utf8(Command::new(binary).output().unwrap().stdout).unwrap();
        (cache_state(&output, "cargo_app_bin_app"), printed)
    };
    assert_eq!(build().1, "one ref: refs/heads/main port = 1\n");

    // A file in the package that neither the compiler nor the build script
    // read leaves every cached result in place.
    std::fs::write(root.path().join("docs/guide.md"), "# Guide\n\nMore.\n").unwrap();
    std::fs::write(root.path().join("docs/new.md"), "new\n").unwrap();
    assert_eq!(
        build(),
        (
            "hit".to_string(),
            "one ref: refs/heads/main port = 1\n".to_string()
        )
    );

    // A file the compiler embeds rebuilds the crate.
    std::fs::write(root.path().join("dev/example.toml"), "port = 2\n").unwrap();
    assert_eq!(
        build(),
        (
            "miss".to_string(),
            "one ref: refs/heads/main port = 2\n".to_string()
        )
    );
    // Files the build script names rerun it, including `.git/HEAD`, which is
    // not a package input. Its own `OUT_DIR` needs no declaration.
    std::fs::write(root.path().join("fixtures/sample/value.txt"), "two\n").unwrap();
    assert_eq!(build().1, "two ref: refs/heads/main port = 2\n");
    std::fs::write(root.path().join(".git/HEAD"), "ref: refs/heads/next\n").unwrap();
    assert_eq!(build().1, "two ref: refs/heads/next port = 2\n");

    // Reverting an edit reuses the build from before it.
    std::fs::write(root.path().join("dev/example.toml"), "port = 3\n").unwrap();
    assert_eq!(build().0, "miss");
    std::fs::write(root.path().join("dev/example.toml"), "port = 2\n").unwrap();
    assert_eq!(
        build(),
        (
            "hit".to_string(),
            "two ref: refs/heads/next port = 2\n".to_string()
        )
    );
}

#[cfg(unix)]
#[test]
fn build_script_tools_run_the_binary_a_mise_shim_selects() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let root = tempfile::tempdir().unwrap();
    let tools = tempfile::tempdir().unwrap();
    let real = tools.path().join("installs/protoc");
    let manager = tools.path().join("bin/mise");
    write_files(
        tools.path(),
        &[
            ("installs/protoc", "#!/bin/sh\necho real protoc\n"),
            (
                "bin/mise",
                &format!(
                    "#!/bin/sh\n[ \"$1\" = which ] && [ \"$2\" = protoc ] && echo {} && exit 0\nexit 1\n",
                    real.display()
                ),
            ),
        ],
    );
    for script in [&real, &manager] {
        std::fs::set_permissions(script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    std::fs::create_dir(tools.path().join("shims")).unwrap();
    symlink(&manager, tools.path().join("shims/protoc")).unwrap();
    write_files(
        root.path(),
        &[
            (
                "Cargo.toml",
                "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
            ),
            (
                "build.rs",
                r#"fn main() {
    let output = std::process::Command::new("protoc").output().unwrap();
    assert!(output.status.success(), "protoc failed");
    let out = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("tool.rs");
    let text = String::from_utf8(output.stdout).unwrap();
    std::fs::write(out, format!("const TOOL: &str = {:?};", text.trim())).unwrap();
}
"#,
            ),
            (
                "src/main.rs",
                "include!(concat!(env!(\"OUT_DIR\"), \"/tool.rs\"));\nfn main() { println!(\"{TOOL}\"); }\n",
            ),
        ],
    );
    let path = format!(
        "{}:{}",
        tools.path().join("shims").display(),
        std::env::var("PATH").unwrap()
    );
    let output = Command::new(env!("CARGO_BIN_EXE_struo"))
        .current_dir(root.path())
        .args([
            "--format",
            "json",
            "--cpu-limit",
            "1",
            "build",
            "cargo_app_bin_app",
        ])
        .env("PATH", path)
        .env("CARGO", env!("CARGO"))
        .env("STRUO_CACHE_PROVIDER", "local")
        .env("STRUO_CHANGE_TRACKER", "0")
        .output()
        .unwrap();
    success(&output);
    let binary = root.path().join(".struo/out/cargo_app_bin_app/app");
    let printed = Command::new(binary).output().unwrap().stdout;
    assert_eq!(String::from_utf8(printed).unwrap(), "real protoc\n");
}

#[test]
fn member_build_script_reruns_for_files_it_reports_outside_its_package() {
    let root = tempfile::tempdir().unwrap();
    write_files(
        root.path(),
        &[
            ("Cargo.toml", "[workspace]\nresolver = \"2\"\nmembers = [\"crates/gen\"]\n"),
            ("proto/value.txt", "one\n"),
            (
                "crates/gen/Cargo.toml",
                "[package]\nname = \"gen\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
            ),
            (
                "crates/gen/build.rs",
                r#"use std::path::PathBuf;
fn main() {
    // Built from pieces, so only the script's own report names it.
    let proto = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join(["..", "..", "proto"].join("/"));
    println!("cargo:rerun-if-changed={}", proto.display());
    let input = proto.join("value.txt");
    println!("cargo:rustc-env=GEN_JOBS={}", std::env::var("NUM_JOBS").unwrap());
    let value = std::fs::read_to_string(&input).unwrap();
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("value.rs");
    std::fs::write(out, format!("pub const VALUE: &str = {:?};", value.trim())).unwrap();
}
"#,
            ),
            (
                "crates/gen/src/lib.rs",
                "include!(concat!(env!(\"OUT_DIR\"), \"/value.rs\"));\npub const JOBS: &str = env!(\"GEN_JOBS\");\n",
            ),
            (
                "crates/gen/src/main.rs",
                "fn main() { println!(\"{} {}\", gen::VALUE, gen::JOBS); }\n",
            ),
        ],
    );
    let printed = || {
        let output = struo(root.path(), "never", &["build", "cargo_gen_bin_gen"]);
        success(&output);
        let binary = root.path().join(".struo/out/cargo_gen_bin_gen/gen");
        String::from_utf8(Command::new(binary).output().unwrap().stdout).unwrap()
    };

    // The script reports its outside input with `rerun-if-changed`, so it
    // needs no `build_script_data` entry.
    // A script that does not build native code with `cc`, `cmake`, or
    // `autotools` runs with one job.
    assert_eq!(printed(), "one 1\n");
    std::fs::write(root.path().join("proto/value.txt"), "two\n").unwrap();
    assert_eq!(printed(), "two 1\n");
    assert_eq!(
        std::fs::read_to_string(root.path().join(".struo/.gitignore")).unwrap(),
        "*\n"
    );
}

#[test]
fn cargo_dev_profile_settings_reach_the_compiler_and_build_script() {
    let root = tempfile::tempdir().unwrap();
    // What a build script sees, in the form Cargo produces: `OPT_LEVEL`,
    // `DEBUG`, `CARGO_CFG_DEBUG_ASSERTIONS`, and `CARGO_ENCODED_RUSTFLAGS`,
    // which carries RUSTFLAGS only, never profile settings.
    let build = r#"fn main() {
    let var = |key: &str| std::env::var(key).unwrap_or_else(|_| "unset".to_string());
    println!("cargo:rustc-env=PROFILE_ENV={} {} {} [{}]", var("OPT_LEVEL"), var("DEBUG"), var("CARGO_CFG_DEBUG_ASSERTIONS"), var("CARGO_ENCODED_RUSTFLAGS"));
}
"#;
    let library = "pub const ENV: &str = env!(\"PROFILE_ENV\");\npub const ASSERTIONS: bool = cfg!(debug_assertions);\n";
    write_files(
        root.path(),
        &[
            (
                "Cargo.toml",
                "[workspace]\nresolver = \"2\"\nmembers = [\"hot\", \"cold\"]\n\n[profile.dev.package.hot]\nopt-level = 3\n\n[profile.dev.package.cold]\ndebug = false\ndebug-assertions = false\n",
            ),
            (
                "hot/Cargo.toml",
                "[package]\nname = \"hot\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
            ),
            ("hot/build.rs", build),
            ("hot/src/lib.rs", library),
            (
                "hot/tests/profile.rs",
                "#[test]\nfn optimized_package_keeps_debug_assertions() {\n    assert_eq!(hot::ENV, \"3 true  []\");\n    assert!(hot::ASSERTIONS);\n}\n",
            ),
            (
                "cold/Cargo.toml",
                "[package]\nname = \"cold\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
            ),
            ("cold/build.rs", build),
            ("cold/src/lib.rs", library),
            (
                "cold/tests/profile.rs",
                "#[test]\nfn disabled_debug_settings_reach_the_build_script() {\n    assert_eq!(cold::ENV, \"0 false unset []\");\n    assert!(!cold::ASSERTIONS);\n}\n",
            ),
        ],
    );
    for target in ["cargo_hot_test_profile", "cargo_cold_test_profile"] {
        success(&struo(root.path(), "never", &["test", target]));
    }
}

#[test]
fn root_package_ignores_cargo_output_and_member_sources() {
    let root = workspace();
    success(&struo(
        root.path(),
        "never",
        &["build", "cargo_derive_demo"],
    ));
    // What `cargo build` and Git leave in the tree is not a root input.
    write_files(
        root.path(),
        &[
            ("target/debug/build-output", "x"),
            (".git/HEAD", "ref: refs/heads/main\n"),
        ],
    );
    let output = struo(root.path(), "never", &["build", "cargo_derive_demo"]);
    success(&output);
    assert_eq!(cache_state(&output, "cargo_derive_demo"), "hit");

    let output = struo(
        root.path(),
        "never",
        &["query", "affected-tests", "--changed-path", "src/lib.rs"],
    );
    success(&output);
    let selected: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let ids = selected
        .as_array()
        .unwrap()
        .iter()
        .map(|test| test["id"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(ids, ["cargo_derive_demo_unit_tests"]);

    // Control: a root source change still rebuilds the root crate.
    let source = root.path().join("src/lib.rs");
    let original = std::fs::read_to_string(&source).unwrap();
    std::fs::write(&source, format!("{original}\nfn added() {{}}\n")).unwrap();
    let output = struo(root.path(), "never", &["build", "cargo_derive_demo"]);
    success(&output);
    assert_eq!(cache_state(&output, "cargo_derive_demo"), "miss");
}

#[cfg(unix)]
#[test]
fn integration_tests_share_one_copy_of_each_package_binary() {
    let root = tempfile::tempdir().unwrap();
    let test = r#"#[test]
fn runs_the_package_binary() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_tool")).output().unwrap();
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "tool\n");
    // Cargo's layout: the test binary in `deps/`, package binaries beside it.
    let sibling = std::env::current_exe().unwrap().parent().unwrap().join("../tool");
    let out = std::process::Command::new(sibling).output().unwrap();
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "tool\n");
}
"#;
    write_files(
        root.path(),
        &[
            (
                "Cargo.toml",
                "[package]\nname = \"pkg\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
            ),
            ("src/lib.rs", ""),
            ("src/bin/tool.rs", "fn main() { println!(\"tool\"); }\n"),
            ("tests/a.rs", test),
            ("tests/b.rs", test),
        ],
    );
    for target in ["cargo_pkg_test_a", "cargo_pkg_test_b"] {
        success(&struo(root.path(), "never", &["test", target]));
        let out = root.path().join(".struo/out").join(target);
        let staged = std::fs::symlink_metadata(out.join("tool")).unwrap();
        assert!(staged.file_type().is_symlink(), "{target}/tool is a copy");
        let tests = std::fs::read_dir(&out)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect::<Vec<_>>();
        assert!(
            !tests.contains(&target[target.len() - 1..].to_string()),
            "{target} stores its test binary twice: {tests:?}"
        );
        assert!(out.join("deps").join(&target[target.len() - 1..]).is_file());
    }
}

#[test]
fn shared_files_a_build_script_joins_select_its_dependents_tests() {
    let root = tempfile::tempdir().unwrap();
    write_files(
        root.path(),
        &[
            (
                "Cargo.toml",
                "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[dependencies]\napi = { path = \"crates/api/types\" }\n[workspace]\nmembers = [\"crates/api/types\", \"crates/consumer\"]\n",
            ),
            ("src/lib.rs", ""),
            ("proto/api/ref.proto", "syntax = \"proto3\";\n"),
            (
                "crates/api/types/Cargo.toml",
                "[package]\nname = \"api\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
            ),
            ("crates/api/types/src/lib.rs", ""),
            (
                "crates/api/types/build.rs",
                r#"fn main() {
    let root = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../..");
    println!("cargo:rerun-if-changed={}", root.join("proto").display());
}
"#,
            ),
            (
                "crates/consumer/Cargo.toml",
                "[package]\nname = \"consumer\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[dependencies]\napi = { path = \"../api/types\" }\n",
            ),
            ("crates/consumer/src/lib.rs", ""),
        ],
    );
    let selected = |path: &str| {
        let output = struo(
            root.path(),
            "never",
            &["query", "affected-tests", "--changed-path", path],
        );
        success(&output);
        let tests: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let mut ids = tests
            .as_array()
            .unwrap()
            .iter()
            .map(|test| test["id"].as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        ids.sort();
        ids
    };
    // `proto` lies in the root package's tree, but the API's build script
    // reads it, so the API's dependents are selected too.
    assert_eq!(
        selected("proto/api/ref.proto"),
        [
            "cargo_api_unit_tests",
            "cargo_app_unit_tests",
            "cargo_consumer_unit_tests"
        ]
    );
    assert!(selected("target/debug/app").is_empty());
}
