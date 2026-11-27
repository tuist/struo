use std::path::Path;
use std::process::Command;

fn write_workspace(root: &Path) {
    for (file, source) in [
        (
            "lib/src/commonMain/Platform.kt",
            "package dev.struo.lib\nexpect fun platformName(): String\nfun describe() = \"on \" + platformName()\n",
        ),
        (
            "lib/src/androidMain/Platform.android.kt",
            "package dev.struo.lib\nactual fun platformName() = \"android\"\n",
        ),
        (
            "lib/OtherManifest.xml",
            "<manifest xmlns:android=\"http://schemas.android.com/apk/res/android\"\n    package = \"dev.struo.other\" />\n",
        ),
        (
            "lib/LibManifest.xml",
            "<!-- copied from a <manifest> template -->\n<manifest xmlns:android=\"http://schemas.android.com/apk/res/android\" />\n",
        ),
        ("lib/src/other/Other.kt", "package dev.struo.other\nfun other() = 1\n"),
        ("lib/src/app/App.kt", "package dev.struo.app\nfun app() = 2\n"),
        ("lib/src/commonTest/Probe.kt", "package dev.struo.lib\nexpect fun probe(): String\n"),
        ("lib/src/test/Probe.android.kt", "package dev.struo.lib\nactual fun probe() = \"android\"\nclass ProbeTest { fun testProbe() { check(probe() == \"android\") } }\n"),
    ] {
        let path = root.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, source).unwrap();
    }
    std::fs::write(
        root.join("lib/struo.toml"),
        r#"[[target]]
name = "Lib"
kind = "android_library"
srcs = ["src/androidMain/**/*.kt"]

[target.attrs]
namespace = "dev.struo.lib"
manifest = "LibManifest.xml"
source_sets = { commonMain = ["src/commonMain/**/*.kt"] }

[[target]]
name = "Other"
kind = "android_library"
srcs = ["src/other/**/*.kt"]

[target.attrs]
namespace = "dev.struo.other"
manifest = "OtherManifest.xml"

[[target]]
name = "App"
kind = "android_binary"
srcs = ["src/app/**/*.kt"]

[target.attrs]
namespace = "dev.struo.app"
application_id = "dev.struo.app.debug"
signing = "none"

[[target]]
name = "LibTests"
kind = "android_local_test"
srcs = ["src/test/**/*.kt"]
deps = ["./Lib"]

[target.attrs]
source_sets = { commonTest = ["src/commonTest/**/*.kt"] }
"#,
    )
    .unwrap();
}

/// The package must be `application_id`, not the namespace written into a
/// generated manifest, as the Android Gradle plugin packages it.
fn assert_application_id(apk: &Path) {
    let aapt2 =
        std::fs::read_dir(Path::new(&std::env::var("ANDROID_HOME").unwrap()).join("build-tools"))
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path().join("aapt2"))
            .find(|path| path.is_file())
            .unwrap();
    let badging = Command::new(aapt2)
        .args(["dump", "badging"])
        .arg(apk)
        .output()
        .unwrap();
    let badging = String::from_utf8_lossy(&badging.stdout);
    assert!(
        badging.contains("package: name='dev.struo.app.debug'"),
        "{badging}"
    );
}

#[test]
fn android_library_compiles_kotlin_source_sets_with_a_namespace_only_manifest() {
    let workspace = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    write_workspace(workspace.path());
    let struo = |args: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_struo"))
            .arg("-C")
            .arg(workspace.path())
            .args(["--memory-limit", "1GiB", "--cpu-limit", "2"])
            .args(args)
            .env("HOME", home.path())
            .env("XDG_CACHE_HOME", home.path().join("cache"))
            .env("XDG_CONFIG_HOME", home.path().join("config"))
            .env("XDG_DATA_HOME", home.path().join("data"))
            .env("XDG_STATE_HOME", home.path().join("state"))
            .env("STRUO_CACHE_PROVIDER", "local")
            .env("STRUO_CHANGE_TRACKER", "0")
            .output()
            .unwrap();
        assert!(output.status.success(), "{args:?}: {output:?}");
        output
    };
    let validation = struo(&["--format", "json", "query", "validate-workspace"]);
    assert!(
        !String::from_utf8_lossy(&validation.stdout).contains("unknown_attr"),
        "{validation:?}"
    );
    struo(&["build", "lib/Lib"]);
    struo(&["build", "lib/Other"]);
    struo(&["build", "lib/App"]);
    struo(&["test", "lib/LibTests"]);

    let out = workspace.path().join(".struo/out/lib/Lib");
    let aar = std::fs::read_dir(&out)
        .unwrap()
        .filter_map(Result::ok)
        .any(|entry| entry.path().extension().is_some_and(|ext| ext == "aar"));
    assert!(aar, "no AAR in {}", out.display());
    assert_application_id(&workspace.path().join(".struo/out/lib/App/aligned.apk"));
}
