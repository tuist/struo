#shellcheck shell=bash

Describe 'Xcode project selection without a manifest'
  BeforeEach 'setup_workspace'
  AfterEach 'cleanup_workspace'

  # Graph discovery only reads native manifests, so off macOS stand-ins for
  # Xcode's tools let the check run on every host. The fixture stores its
  # projects as JSON, which the real `plutil` converts unchanged.
  install_xcode_stand_ins() {
    if [ "$(uname -s)" = Darwin ] && command -v xcrun >/dev/null 2>&1; then
      return 0
    fi
    mkdir -p "$SPEC_ROOT/bin"
    cat > "$SPEC_ROOT/bin/plutil" <<'SH'
#!/bin/sh
for path in "$@"; do :; done
exec cat "$path"
SH
    cat > "$SPEC_ROOT/bin/xcrun" <<'SH'
#!/bin/sh
case "$*" in
  *--show-sdk-path) printf '%s\n' "$SPEC_ROOT" ;;
  *--show-sdk-version) printf '15.0\n' ;;
esac
SH
    chmod +x "$SPEC_ROOT/bin/plutil" "$SPEC_ROOT/bin/xcrun"
    PATH="$SPEC_ROOT/bin:$PATH"
    export PATH
  }

  It 'resolves the root workspace instead of failing on several Xcode projects'
    cp -R "$REPO_ROOT/fixtures/xcode_workspace_selection/." "$WORKSPACE/"
    install_xcode_stand_ins

    When call struo --format json query targets
    The status should be success
    The stdout should include '"name":"App","kind":"apple_library"'
    The stdout should include '"name":"DesignSystem","kind":"apple_library"'
    The stdout should not include '"name":"Demo"'
    The stdout should not include '"name":"Playground"'
    The path "$WORKSPACE/struo.toml" should not be exist
  End

  It 'resolves a root workspace whose projects all live in subdirectories'
    cp -R "$REPO_ROOT/fixtures/xcode_workspace_selection/." "$WORKSPACE/"
    mkdir -p "$WORKSPACE/Apps" "$WORKSPACE/Tools"
    mv "$WORKSPACE/App.xcodeproj" "$WORKSPACE/App.swift" "$WORKSPACE/Apps/"
    mv "$WORKSPACE/Playground.xcodeproj" "$WORKSPACE/Playground.swift" "$WORKSPACE/Tools/"
    sed 's|group:App.xcodeproj|group:Apps/App.xcodeproj|' "$WORKSPACE/App.xcworkspace/contents.xcworkspacedata" > "$SPEC_ROOT/contents"
    mv "$SPEC_ROOT/contents" "$WORKSPACE/App.xcworkspace/contents.xcworkspacedata"
    install_xcode_stand_ins

    When call struo --format json query targets
    The status should be success
    The stdout should include '"id":"xcode","package":"","name":"xcode","kind":"xcode_workspace","deps":["App","DesignSystem"]'
    The stdout should include '"name":"App","kind":"apple_library"'
    The stdout should include '"name":"DesignSystem","kind":"apple_library"'
    The stdout should not include '"name":"Demo"'
    The stdout should not include '"name":"Playground"'
  End

  It 'reports competing workspaces on the seed and accepts the suggested manifest'
    cp -R "$REPO_ROOT/fixtures/xcode_workspace_selection/." "$WORKSPACE/"
    cp -R "$WORKSPACE/App.xcworkspace" "$WORKSPACE/Tools.xcworkspace"
    install_xcode_stand_ins
    choose_suggested_workspace() {
      ! struo --format json query targets > /dev/null 2> "$SPEC_ROOT/ambiguous.err" || return 1
      diagnostic="$(grep '^{' "$SPEC_ROOT/ambiguous.err" | jq -c '.error.diagnostics[] | select(.code == "native_entry_point_ambiguous" and .target == "xcode" and .attribute == "project")')"
      [ -n "$diagnostic" ] || { cat "$SPEC_ROOT/ambiguous.err"; return 1; }
      printf '%s' "$diagnostic" | jq -r '.repairs[] | select(contains("`Tools.xcworkspace`"))' | sed 1,2d > "$WORKSPACE/struo.toml"
      grep -q 'project = "Tools.xcworkspace"' "$WORKSPACE/struo.toml" || { cat "$WORKSPACE/struo.toml"; return 1; }
      struo --format json query targets
    }

    When call choose_suggested_workspace
    The status should be success
    The stdout should include '"name":"App","kind":"apple_library"'
    The stdout should include '"name":"DesignSystem","kind":"apple_library"'
    The stdout should not include 'native_entry_point_ambiguous'
  End
End
