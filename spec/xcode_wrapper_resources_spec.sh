#shellcheck shell=bash

Describe 'Xcode wrapper resources'
  BeforeEach 'setup_workspace'
  AfterEach 'cleanup_workspace'

  xcode_unavailable() {
    [ "$(uname -s)" != Darwin ] || ! xcrun --find swiftc >/dev/null 2>&1
  }

  build_theme_app() {
    cp -R "$REPO_ROOT/fixtures/xcode_wrapper_resources/." "$WORKSPACE/"
    (close_shellspec_fds; STRUO_CHANGE_TRACKER=0 "$STRUO_BIN" -C "$WORKSPACE" build ThemeApp --format json) > "$WORKSPACE/build.json" 2> "$WORKSPACE/build.err" || { cat "$WORKSPACE/build.err"; return 1; }
    resources="$(find "$WORKSPACE/.struo/out" -type d -path '*/ThemeApp.app/Contents/Resources' | head -n 1)"
    (cd "$resources" && find . \( -path './*.theme/*' -o -path './*.rtfd/*' \) -type f | sort)
  }

  It 'copies wrapper directories into the bundle like folder references'
    Skip if 'requires Xcode' xcode_unavailable
    When call build_theme_app
    The status should be success
    The output should equal "./Credits.rtfd/TXT.rtf
./Gray.theme/Info.plist
./Gray.theme/stylesheet.css
./Plum.theme/Info.plist
./Plum.theme/stylesheet.css
./Sepia.theme/Info.plist
./Sepia.theme/stylesheet.css"
  End
End
