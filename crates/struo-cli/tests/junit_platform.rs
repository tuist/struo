use std::path::Path;
use std::process::Command;

const TESTS: &str = r#"import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assumptions.assumeTrue
import org.junit.jupiter.api.BeforeAll
import org.junit.jupiter.api.BeforeEach
import org.junit.jupiter.api.Disabled
import org.junit.jupiter.api.DynamicTest.dynamicTest
import org.junit.jupiter.api.Nested
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.TestFactory
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.ValueSource

class PlatformTest {
    @Test fun passes() {}
    @Test fun fails() { assertEquals(1, 2) }
    @Disabled @Test fun disabled() { error("disabled test ran") }
    @Test fun aborted() { assumeTrue(false) }
    @ParameterizedTest @ValueSource(ints = [1, 2]) fun even(value: Int) { assertEquals(0, value % 2) }
    @TestFactory fun dynamic() = listOf(dynamicTest("a") {}, dynamicTest("b") {})
    @Nested inner class Inner { @Test fun nested() {} }
}

@Disabled
class DisabledTest {
    @Test fun one() { error("disabled class ran") }
    @Test fun two() { error("disabled class ran") }
}

class BrokenSetupTest {
    companion object { @JvmStatic @BeforeAll fun setUp() { error("class setup failed") } }
    @Test fun never() {}
}

class LifecycleTest {
    private var value = 0
    @BeforeEach fun setUp() { value = 1 }
    @Test fun usesSetUp() { assertEquals(1, value) }
    fun testConvention() { error("convention methods are not JUnit Platform tests") }
}
"#;

fn struo(workspace: &Path, home: &Path, target: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_struo"))
        .arg("-C")
        .arg(workspace)
        .args(["--memory-limit", "1GiB", "--cpu-limit", "2", "test", target])
        .env("HOME", home)
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_STATE_HOME", home.join("state"))
        .env("STRUO_CACHE_PROVIDER", "local")
        .env("STRUO_CHANGE_TRACKER", "0")
        .output()
        .unwrap()
}

fn cases(workspace: &Path, target: &str) -> Vec<(String, String)> {
    let results = walkdir(workspace.join(".struo"))
        .into_iter()
        .find(|path| {
            path.ends_with("test_results.json")
                && path.to_string_lossy().contains(&format!("/{target}/"))
        })
        .unwrap_or_else(|| panic!("no test results for {target}"));
    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(results).unwrap()).unwrap();
    let mut cases: Vec<_> = value["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|case| {
            (
                case["id"].as_str().unwrap().to_owned(),
                case["status"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    cases.sort();
    cases
}

fn walkdir(root: std::path::PathBuf) -> Vec<std::path::PathBuf> {
    let mut pending = vec![root];
    let mut files = Vec::new();
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                files.push(path);
            }
        }
    }
    files
}

#[test]
#[ignore = "requires JUNIT5_CLASSPATH containing the JUnit Jupiter, Platform launcher, and opentest4j jars"]
fn kotlin_jvm_tests_run_on_the_junit_platform_like_gradle() {
    let classpath = std::env::var_os("JUNIT5_CLASSPATH").unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let third_party = workspace.path().join("third_party");
    std::fs::create_dir_all(&third_party).unwrap();
    let mut jars = Vec::new();
    for jar in std::env::split_paths(&classpath) {
        let name = jar.file_name().unwrap().to_string_lossy().into_owned();
        std::fs::copy(&jar, third_party.join(&name)).unwrap();
        jars.push(format!("\"third_party/{name}\""));
    }
    std::fs::write(workspace.path().join("PlatformTest.kt"), TESTS).unwrap();
    std::fs::write(
        workspace.path().join("struo.toml"),
        format!(
            "[[target]]\nname = \"JUnit\"\nkind = \"java_import\"\n\n[target.attrs]\njars = [{}]\n\n\
             [[target]]\nname = \"Tests\"\nkind = \"kotlin_jvm_test\"\nsrcs = [\"*.kt\"]\ndeps = [\"./JUnit\"]\n\n\
             [target.attrs]\ntest_framework = \"junit5\"\n\n\
             [[target]]\nname = \"Filtered\"\nkind = \"kotlin_jvm_test\"\nsrcs = [\"*.kt\"]\ndeps = [\"./JUnit\"]\n\n\
             [target.attrs]\ntest_framework = \"junit5\"\nargs = [\"PlatformTest#even[2]\", \"PlatformTest$Inner\"]\n",
            jars.join(", ")
        ),
    )
    .unwrap();

    let output = struo(workspace.path(), home.path(), "Tests");
    assert!(!output.status.success(), "{output:?}");
    let expected: Vec<(String, String)> = [
        ("Tests::BrokenSetupTest.classMethod", "failed"),
        ("Tests::DisabledTest.one", "skipped"),
        ("Tests::DisabledTest.two", "skipped"),
        ("Tests::LifecycleTest.usesSetUp", "passed"),
        ("Tests::PlatformTest$Inner.nested", "passed"),
        ("Tests::PlatformTest.aborted", "skipped"),
        ("Tests::PlatformTest.disabled", "skipped"),
        ("Tests::PlatformTest.dynamic[1]", "passed"),
        ("Tests::PlatformTest.dynamic[2]", "passed"),
        ("Tests::PlatformTest.even[1]", "failed"),
        ("Tests::PlatformTest.even[2]", "passed"),
        ("Tests::PlatformTest.fails", "failed"),
        ("Tests::PlatformTest.passes", "passed"),
    ]
    .into_iter()
    .map(|(id, status)| (id.to_owned(), status.to_owned()))
    .collect();
    assert_eq!(cases(workspace.path(), "Tests"), expected);

    let output = struo(workspace.path(), home.path(), "Filtered");
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        cases(workspace.path(), "Filtered"),
        [
            (
                "Filtered::PlatformTest$Inner.nested".to_owned(),
                "passed".to_owned()
            ),
            (
                "Filtered::PlatformTest.even[2]".to_owned(),
                "passed".to_owned()
            ),
        ]
    );
}

#[test]
#[ignore = "requires JUNIT5_CLASSPATH, JACOCO_TOOLS containing jacocoagent.jar/jacococli.jar, and Linux strace"]
fn kotlin_junit5_per_test_coverage_measures_each_filtered_unit() {
    let classpath = std::env::var_os("JUNIT5_CLASSPATH").unwrap();
    let tools = std::path::PathBuf::from(std::env::var_os("JACOCO_TOOLS").unwrap());
    let workspace = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let third_party = workspace.path().join("third_party");
    std::fs::create_dir_all(&third_party).unwrap();
    let mut jars = Vec::new();
    for jar in std::env::split_paths(&classpath) {
        let name = jar.file_name().unwrap().to_string_lossy().into_owned();
        std::fs::copy(&jar, third_party.join(&name)).unwrap();
        jars.push(format!("\"third_party/{name}\""));
    }
    for jar in ["jacocoagent.jar", "jacococli.jar"] {
        std::fs::copy(tools.join(jar), third_party.join(jar)).unwrap();
    }
    std::fs::write(
        workspace.path().join("Lib.kt"),
        "object Lib {\n    fun first() = 41\n    fun second() = 17\n}\n",
    )
    .unwrap();
    std::fs::write(
        workspace.path().join("LibTest.kt"),
        r"import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.DynamicTest
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.TestFactory
import org.junit.jupiter.api.TestInfo
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.ValueSource

class LibTest {
    @Test fun first() { assertEquals(41, Lib.first()) }
    @Test fun over() { assertEquals(41, Lib.first()) }
    @Test fun over(info: TestInfo) { assertEquals(17, Lib.second()) }
    @ParameterizedTest @ValueSource(ints = [1, 2]) fun even(value: Int) { assertEquals(0, value % 2) }
    @TestFactory fun empty(): List<DynamicTest> = emptyList()
}
",
    )
    .unwrap();
    std::fs::write(
        workspace.path().join("struo.toml"),
        format!(
            "[[target]]\nname = \"JUnit\"\nkind = \"java_import\"\n\n[target.attrs]\njars = [{}]\n\n\
             [[target]]\nname = \"Lib\"\nkind = \"kotlin_jvm_library\"\nsrcs = [\"Lib.kt\"]\ndeps = [\"./JUnit\"]\n\n\
             [[target]]\nname = \"Tests\"\nkind = \"kotlin_jvm_test\"\nsrcs = [\"LibTest.kt\"]\ndeps = [\"./Lib\"]\n\n\
             [target.attrs]\ntest_framework = \"junit5\"\ncoverage_per_test = true\ncoverage_sources = [\"Lib.kt\"]\n\
             jacoco_agent = \"third_party/jacocoagent.jar\"\njacoco_cli = \"third_party/jacococli.jar\"\n\
             args = [\"LibTest#first\", \"LibTest#over\", \"LibTest#even[2]\", \"LibTest#empty\"]\n",
            jars.join(", ")
        ),
    )
    .unwrap();

    let output = struo(workspace.path(), home.path(), "Tests");
    assert!(output.status.success(), "{output:?}");
    let results: serde_json::Value = serde_json::from_slice(
        &std::fs::read(
            workspace
                .path()
                .join(".struo/out/Tests/test/test_results.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        results["coverage_evidence"]["status"], "collected",
        "{results}"
    );
    assert_eq!(
        results["coverage"]["files"][0]["path"], "Lib.kt",
        "{results}"
    );
    let mut units: Vec<_> = results["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|case| case["id"].as_str().unwrap().to_owned())
        .collect();
    units.sort();
    assert_eq!(
        units,
        [
            "Tests::LibTest.empty",
            "Tests::LibTest.even",
            "Tests::LibTest.first",
            "Tests::LibTest.over()",
            "Tests::LibTest.over(org.junit.jupiter.api.TestInfo)",
        ]
    );
}

#[test]
#[ignore = "requires JUNIT5_CLASSPATH, JACOCO_TOOLS containing jacocoagent.jar/jacococli.jar, Linux strace, and the Android SDK"]
fn android_local_tests_run_on_the_junit_platform_with_per_test_coverage() {
    let classpath = std::env::var_os("JUNIT5_CLASSPATH").unwrap();
    let tools = std::path::PathBuf::from(std::env::var_os("JACOCO_TOOLS").unwrap());
    let workspace = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let third_party = workspace.path().join("third_party");
    std::fs::create_dir_all(&third_party).unwrap();
    let mut jars = Vec::new();
    for jar in std::env::split_paths(&classpath) {
        let name = jar.file_name().unwrap().to_string_lossy().into_owned();
        std::fs::copy(&jar, third_party.join(&name)).unwrap();
        jars.push(format!("\"third_party/{name}\""));
    }
    for jar in ["jacocoagent.jar", "jacococli.jar"] {
        std::fs::copy(tools.join(jar), third_party.join(jar)).unwrap();
    }
    std::fs::create_dir_all(workspace.path().join("lib")).unwrap();
    std::fs::write(
        workspace.path().join("lib/Lib.kt"),
        "object Lib {\n    fun first() = 41\n}\n",
    )
    .unwrap();
    std::fs::write(
        workspace.path().join("LibTest.kt"),
        r"import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Disabled
import org.junit.jupiter.api.Test
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.ValueSource

class LibTest {
    @Test fun first() { assertEquals(41, Lib.first()) }
    @Disabled @Test fun disabled() { error(1) }
    @ParameterizedTest @ValueSource(ints = [1, 2]) fun even(value: Int) { assertEquals(0, value % 2) }
}
",
    )
    .unwrap();
    let jars = jars.join(", ");
    std::fs::write(
        workspace.path().join("struo.toml"),
        format!(
            "[[target]]\nname = \"JUnit\"\nkind = \"java_import\"\n\n[target.attrs]\njars = [{jars}]\n\n\
             [[target]]\nname = \"Lib\"\nkind = \"android_library\"\nsrcs = [\"lib/*.kt\"]\ndeps = [\"./JUnit\"]\n\n\
             [target.attrs]\nnamespace = \"dev.struo.lib\"\n\n\
             [[target]]\nname = \"Tests\"\nkind = \"android_local_test\"\nsrcs = [\"*.kt\"]\ndeps = [\"./Lib\"]\n\n\
             [target.attrs]\ntest_framework = \"junit5\"\n\n\
             [[target]]\nname = \"Covered\"\nkind = \"android_local_test\"\nsrcs = [\"*.kt\"]\ndeps = [\"./Lib\"]\n\n\
             [target.attrs]\ntest_framework = \"junit5\"\nargs = [\"LibTest#first\", \"LibTest#even[2]\"]\n\
             coverage_per_test = true\ncoverage_sources = [\"lib/Lib.kt\"]\n\
             jacoco_agent = \"third_party/jacocoagent.jar\"\njacoco_cli = \"third_party/jacococli.jar\"\n"
        ),
    )
    .unwrap();

    let output = struo(workspace.path(), home.path(), "Tests");
    assert!(!output.status.success(), "{output:?}");
    assert_eq!(
        cases(workspace.path(), "Tests"),
        [
            ("Tests::LibTest.disabled", "skipped"),
            ("Tests::LibTest.even[1]", "failed"),
            ("Tests::LibTest.even[2]", "passed"),
            ("Tests::LibTest.first", "passed"),
        ]
        .map(|(id, status)| (id.to_owned(), status.to_owned()))
    );

    let output = struo(workspace.path(), home.path(), "Covered");
    assert!(output.status.success(), "{output:?}");
    let results: serde_json::Value = serde_json::from_slice(
        &std::fs::read(
            workspace
                .path()
                .join(".struo/out/Covered/test/test_results.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        results["coverage_evidence"]["status"], "collected",
        "{results}"
    );
    assert_eq!(results["summary"]["total"], 2, "{results}");
    assert_eq!(
        results["coverage"]["files"][0]["path"], "lib/Lib.kt",
        "{results}"
    );
}
