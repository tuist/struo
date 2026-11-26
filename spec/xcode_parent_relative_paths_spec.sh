#shellcheck shell=bash

Describe 'Xcode project paths outside the project directory'
  BeforeEach 'setup_workspace'
  AfterEach 'cleanup_workspace'

  xcode_unavailable() {
    [ "$(uname -s)" != Darwin ] || ! xcrun --find swiftc >/dev/null 2>&1
  }

  build_sibling_paths() {
    cp -R "$REPO_ROOT/fixtures/xcode_parent_relative_paths/." "$WORKSPACE/"
    STRUO_CHANGE_TRACKER=0 struo build app/ReproApp --format json > "$WORKSPACE/build.json" 2> "$WORKSPACE/build.err" || { cat "$WORKSPACE/build.err"; return 1; }
    app="$(find "$WORKSPACE/.struo/out" -type d -name ReproApp.app | head -n 1)"
    [ -n "$app" ] || return 1
    [ "$(plutil -extract ReproInfoPlistSource raw "$app/Contents/Info.plist")" = shared ] || return 1
    "$app/Contents/MacOS/ReproApp" 2>/dev/null
  }

  It 'resolves ../ build settings and group paths against the project directory'
    Skip if 'requires Xcode' xcode_unavailable
    When call build_sibling_paths
    The status should be success
    The output should equal 'shared sources compiled'
  End
End
