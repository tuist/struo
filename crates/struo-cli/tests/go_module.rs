use std::path::Path;
use std::process::{Command, Output};

fn workspace() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    for (path, contents) in [
        ("go.mod", "module example.com/native\n\ngo 1.26.0\n"),
        ("README.md", "Native Go module.\n"),
        (
            "greeting/greeting.go",
            "package greeting\n\nfunc Message() string { return \"hello\" }\n",
        ),
        (
            "greeting/greeting_test.go",
            r#"package greeting

import (
	"os"
	"strings"
	"testing"
)

func TestGolden(t *testing.T) {
	for _, path := range []string{"testdata/golden.txt", "expected.txt"} {
		want, err := os.ReadFile(path)
		if err != nil {
			t.Fatal(err)
		}
		if Message() != strings.TrimSpace(string(want)) {
			t.Fatal("mismatch")
		}
	}
}
"#,
        ),
        ("greeting/testdata/golden.txt", "hello\n"),
        ("greeting/expected.txt", "hello\n"),
        (
            "other/other.go",
            "package other\n\nimport _ \"embed\"\n\n//go:embed value.txt\nvar value string\n\nfunc Value() int { return len(value) }\n",
        ),
        ("other/value.txt", "1"),
        (
            "other/other_test.go",
            r#"package other

import (
	"os"
	"path"
	"runtime"
	"testing"
)

// The fixtures directory is reached from this file's own path, as tests do
// with runtime.Caller.
func fixtures() string {
	_, file, _, _ := runtime.Caller(0)
	return path.Join(path.Dir(file), "/../fixtures")
}

func TestValue(t *testing.T) {
	data, err := os.ReadFile("../fixtures/data.txt")
	if Value() != 1 || fixtures() == "" || err != nil || string(data) != "data\n" {
		t.Fatal("value")
	}
}
"#,
        ),
        ("fixtures/data.txt", "data\n"),
        (
            "nested/go.mod",
            "module example.com/nested\n\ngo 1.26.0\n\nrequire example.com/native v0.0.0\n\nreplace example.com/native => ../\n",
        ),
        (
            "nested/nested_test.go",
            "package nested\n\nimport (\n\t\"testing\"\n\n\t\"example.com/native/other\"\n)\n\nfunc TestUsesRoot(t *testing.T) {\n\tif other.Value() != 1 {\n\t\tt.Fatal(\"value\")\n\t}\n}\n",
        ),
    ] {
        let path = root.path().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }
    root
}

fn struo(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_struo"))
        .current_dir(root)
        .args(["--format", "json"])
        .args(args)
        .env("STRUO_CACHE_PROVIDER", "local")
        .env("STRUO_CHANGE_TRACKER", "0")
        .env("XDG_CACHE_HOME", root.join(".xdg-cache"))
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

fn affected(root: &Path, path: &str) -> Vec<String> {
    let output = struo(root, &["query", "affected-tests", "--changed-path", path]);
    success(&output);
    let tests: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    tests
        .as_array()
        .unwrap()
        .iter()
        .map(|test| test["id"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn go_module_without_struo_toml_tests_in_package_directory_and_selects_by_package() {
    let root = workspace();
    let output = struo(root.path(), &["test"]);
    success(&output);

    assert_eq!(affected(root.path(), "README.md"), Vec::<String>::new());
    assert_eq!(
        affected(root.path(), "greeting/testdata/golden.txt"),
        ["go_greeting_test"]
    );
    assert_eq!(
        affected(root.path(), "other/other.go"),
        ["go_other_test", "nested/go_nested_test"]
    );
    assert_eq!(
        affected(root.path(), "fixtures/data.txt"),
        ["go_other_test"]
    );
    assert_eq!(
        affected(root.path(), "other/value.txt"),
        ["go_other_test", "nested/go_nested_test"]
    );
    assert_eq!(
        affected(root.path(), "greeting/expected.txt"),
        ["go_greeting_test"]
    );
    assert_eq!(
        affected(root.path(), "go.mod"),
        ["go_greeting_test", "go_other_test", "nested/go_nested_test"]
    );

    // A file the test reads from its package directory is an input of the
    // test run, so a cached pass does not outlive a change to it.
    std::fs::write(root.path().join("greeting/expected.txt"), "bye\n").unwrap();
    assert!(!struo(root.path(), &["test", "go_greeting_test"])
        .status
        .success());

    // So is a file the test reaches above its package directory.
    std::fs::write(root.path().join("fixtures/data.txt"), "changed\n").unwrap();
    assert!(!struo(root.path(), &["test", "go_other_test"])
        .status
        .success());
}

#[test]
fn go_vendored_module_builds_and_tests() {
    let root = tempfile::tempdir().unwrap();
    for (path, contents) in [
        (
            "go.mod",
            "module example.com/app\n\ngo 1.26.0\n\nrequire (\n\texample.com/a/b_c v0.0.0\n\texample.com/a_b/c v0.0.0\n)\n\nreplace example.com/a/b_c => ./x\n\nreplace example.com/a_b/c => ./y\n",
        ),
        // Both module paths flatten to the same target name.
        ("x/go.mod", "module example.com/a/b_c\n\ngo 1.26.0\n"),
        ("x/x.go", "package b_c\n\nfunc Two() int { return 2 }\n"),
        ("y/go.mod", "module example.com/a_b/c\n\ngo 1.26.0\n"),
        ("y/y.go", "package c\n\nfunc Two() int { return 2 }\n"),
        (
            "app.go",
            "package app\n\nimport (\n\t\"example.com/a/b_c\"\n\t\"example.com/a_b/c\"\n)\n\nfunc Four() int { return b_c.Two() + c.Two() }\n",
        ),
        (
            "app_test.go",
            "package app\n\nimport \"testing\"\n\nfunc TestFour(t *testing.T) {\n\tif Four() != 4 {\n\t\tt.Fatal(\"four\")\n\t}\n}\n",
        ),
    ] {
        let path = root.path().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }
    let vendor = Command::new("go")
        .args(["mod", "vendor"])
        .current_dir(root.path())
        .env("GOWORK", "off")
        .env("GOFLAGS", "-mod=mod")
        .env("GOTOOLCHAIN", "local")
        .output()
        .unwrap();
    success(&vendor);
    success(&struo(root.path(), &["test"]));
}

fn cgo_workspace(parent: &Path, name: &str) -> std::path::PathBuf {
    let root = parent.join(name);
    for (path, contents) in [
        ("go.mod", "module example.com/cgo\n\ngo 1.26.0\n"),
        (
            "add.go",
            "package cgo\n\n// #cgo CFLAGS: -I${SRCDIR}/include\n// #include \"offset.h\"\n// int add(int a, int b) { return a + b + OFFSET; }\nimport \"C\"\n\nfunc Add(a, b int) int { return int(C.add(C.int(a), C.int(b))) }\n",
        ),
        ("include/offset.h", "#define OFFSET 0\n"),
        (
            "sub/sub.go",
            "package sub\n\n// #include \"../include/offset.h\"\n// int offset(void) { return OFFSET; }\nimport \"C\"\n\nfunc Offset() int { return int(C.offset()) }\n",
        ),
        (
            "sub/sub_test.go",
            "package sub\n\nimport \"testing\"\n\nfunc TestOffset(t *testing.T) {\n\tif Offset() != 0 {\n\t\tt.Fatal(\"offset\")\n\t}\n}\n",
        ),
        (
            "add_test.go",
            "package cgo\n\nimport \"testing\"\n\nfunc TestAdd(t *testing.T) {\n\tif Add(1, 2) != 3 {\n\t\tt.Fatal(\"add\")\n\t}\n}\n",
        ),
    ] {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }
    root
}

#[test]
fn go_cgo_test_binary_is_identical_across_checkout_paths() {
    let parent = tempfile::tempdir().unwrap();
    let binaries = ["first", "second-checkout"].map(|name| {
        let root = cgo_workspace(parent.path(), name);
        success(&struo(&root, &["build", "go_cgo_test"]));
        std::fs::read(root.join(".struo/out/go_cgo_test/go_cgo_test.test")).unwrap()
    });
    assert!(
        binaries[0] == binaries[1],
        "cgo test binaries differ between checkouts"
    );

    // A header the cgo flags include from outside the package directory is a
    // compile input, so changing it selects and rebuilds the test.
    let root = parent.path().join("first");
    // A header a preamble includes by a relative path above the package is too.
    assert_eq!(
        affected(&root, "include/offset.h"),
        ["go_cgo_test", "go_sub_test"]
    );
    success(&struo(&root, &["test", "go_sub_test"]));
    std::fs::write(root.join("include/offset.h"), "#define OFFSET 1\n").unwrap();
    assert!(!struo(&root, &["test", "go_cgo_test"]).status.success());
    assert!(!struo(&root, &["test", "go_sub_test"]).status.success());
}
