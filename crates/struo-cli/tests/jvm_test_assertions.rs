use std::process::Command;

#[test]
fn kotlin_jvm_tests_run_with_assertions_enabled_like_gradle() {
    let workspace = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    std::fs::write(
        workspace.path().join("AssertionsTest.kt"),
        "class AssertionsTest {\n    fun testAssertionsEnabled() {\n        check(AssertionsTest::class.java.desiredAssertionStatus()) { \"assertions disabled\" }\n    }\n}\n",
    )
    .unwrap();
    std::fs::write(
        workspace.path().join("struo.toml"),
        "[[target]]\nname = \"Tests\"\nkind = \"kotlin_jvm_test\"\nsrcs = [\"*.kt\"]\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_struo"))
        .arg("-C")
        .arg(workspace.path())
        .args([
            "--memory-limit",
            "512MiB",
            "--cpu-limit",
            "1",
            "test",
            "Tests",
        ])
        .env("HOME", home.path())
        .env("XDG_CACHE_HOME", home.path().join("cache"))
        .env("XDG_CONFIG_HOME", home.path().join("config"))
        .env("XDG_DATA_HOME", home.path().join("data"))
        .env("XDG_STATE_HOME", home.path().join("state"))
        .env("STRUO_CACHE_PROVIDER", "local")
        .env("STRUO_CHANGE_TRACKER", "0")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}
