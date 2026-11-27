#shellcheck shell=bash

# A crates.io package with a build script, served from a local directory
# source so the example needs no network: Cargo still reports it as a
# `registry+` package whose sources live outside the workspace, like the
# extracted crates under the Cargo home.
Describe 'Cargo registry dependency with a build script'
  BeforeEach 'setup_workspace'
  AfterEach 'cleanup_workspace'

  create_registry_fixture() {
    registry="$SPEC_ROOT/registry"
    mkdir -p "$registry/gen-1.0.0/src" "$WORKSPACE/src" "$WORKSPACE/.cargo"
    cat > "$registry/gen-1.0.0/Cargo.toml" <<'TOML'
[package]
name = "gen"
version = "1.0.0"
edition = "2021"
build = "build.rs"
TOML
    printf 'fn main() { println!("cargo:rustc-env=GEN_VALUE=from-build-script"); }\n' > "$registry/gen-1.0.0/build.rs"
    printf 'pub fn value() -> &\x27static str { env!("GEN_VALUE") }\n' > "$registry/gen-1.0.0/src/lib.rs"
    checksum=0000000000000000000000000000000000000000000000000000000000000000
    printf '{"files":{},"package":"%s"}' "$checksum" > "$registry/gen-1.0.0/.cargo-checksum.json"
    cat > "$WORKSPACE/.cargo/config.toml" <<TOML
[source.crates-io]
replace-with = "fixture"
[source.fixture]
directory = "$registry"
TOML
    cat > "$WORKSPACE/Cargo.toml" <<'TOML'
[package]
name = "app"
version = "0.1.0"
edition = "2021"
[dependencies]
gen = "1"
TOML
    cat > "$WORKSPACE/Cargo.lock" <<TOML
version = 4

[[package]]
name = "app"
version = "0.1.0"
dependencies = [
 "gen",
]

[[package]]
name = "gen"
version = "1.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "$checksum"
TOML
    printf 'fn main() { println!("{}", gen::value()); }\n' > "$WORKSPACE/src/main.rs"
    cat > "$WORKSPACE/struo.toml" <<'TOML'
[[target]]
name = "cargo"
kind = "cargo_workspace"
srcs = ["Cargo.toml", "Cargo.lock"]
TOML
  }

  build_and_run() {
    struo build cargo_app_bin_app --quiet >/dev/null || return
    "$WORKSPACE/.struo/out/cargo_app_bin_app/app"
  }

  # The second build starts without `.struo`, so the source snapshot is a
  # cache hit whose files are not on disk when the build script's compile
  # action hashes `source/build.rs`.
  build_cold_then_from_cache() {
    create_registry_fixture
    build_and_run || return
    rm -rf "$WORKSPACE/.struo"
    build_and_run
  }

  It 'extracts the crate source before compiling its build script, cold and from cache'
    When call build_cold_then_from_cache
    The status should be success
    The output should equal 'from-build-script
from-build-script'
  End
End
