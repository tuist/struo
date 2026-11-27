#shellcheck shell=bash

Describe 'Xcode framework with a private umbrella header'
  BeforeEach 'setup_workspace'
  AfterEach 'cleanup_workspace'

  xcode_unavailable() {
    [ "$(uname -s)" != Darwin ] || ! xcrun --find swiftc >/dev/null 2>&1
  }

  build_kit_app() {
    cp -R "$REPO_ROOT/fixtures/xcode_private_umbrella/." "$WORKSPACE/"
    (close_shellspec_fds; STRUO_CHANGE_TRACKER=0 "$STRUO_BIN" -C "$WORKSPACE" build KitApp --format json) > "$WORKSPACE/build.json" 2> "$WORKSPACE/build.err" || { cat "$WORKSPACE/build.err"; return 1; }
    "$WORKSPACE/.struo/out/KitApp/KitApp.app/Contents/MacOS/KitApp"
  }

  It 'exposes Objective-C declarations from the private umbrella header to the framework and its importers'
    Skip if 'requires Xcode' xcode_unavailable
    When call build_kit_app
    The status should be success
    The output should equal '42'
  End
End
