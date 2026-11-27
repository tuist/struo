# Head Start project: Helix, a terminal text editor written in Rust.
# Sourced by ../run.sh; see ../AGENTS.md for what each variable means.

name_display=Helix
repository=https://github.com/helix-editor/helix
revision=1a3fdfda6dfdda4567af6093a6249bb2bb422fc6
root_target=cargo_helix_term_bin_hx
edits="helix-term/src/commands.rs helix-core/src/lib.rs"
manifests="Cargo.lock"

native_name=cargo
native_version="cargo --version"
native_fetch="cargo fetch --locked --quiet"
native_build="cargo build --locked --quiet -p helix-term --bin hx"
native_timeline_build="$native_build --timings"
native_timeline="node '$project/native-timeline.mjs' target/cargo-timings/cargo-timing.html"

# Settings from the caller's environment, such as CARGO_INCREMENTAL=0 or
# CARGO_PROFILE_DEV_DEBUG from a parent mise.toml, would change Cargo's build
# but not Struo's, which reads profiles from Cargo.toml.
isolate_env() {
  for var in $(env | sed -n 's/^\(CARGO_PROFILE_[^=]*\|CARGO_BUILD_[^=]*\|CARGO_TARGET_[^=]*\)=.*/\1/p'); do
    unset "$var"
  done
  unset CARGO_INCREMENTAL RUSTFLAGS RUSTDOCFLAGS CARGO_ENCODED_RUSTFLAGS RUSTC_WRAPPER RUSTC_WORKSPACE_WRAPPER
}

setup_notes="Disables Helix's tree-sitter grammar download (HELIX_DISABLE_AUTO_GRAMMAR_BUILD) and sets debug = 0 in the dev profile, which Struo applies only when a profile names it. Both tools use the Rust toolchain mise selects for this repository."
setup() {
  printf '\n[env]\nHELIX_DISABLE_AUTO_GRAMMAR_BUILD = "1"\n' >>.cargo/config.toml
  printf '\n[profile.dev]\ndebug = 0\n' >>Cargo.toml
}
