#shellcheck shell=bash

Describe 'Xcode reproducible links'
  BeforeEach 'setup_workspace'
  AfterEach 'cleanup_workspace'

  xcode_unavailable() {
    [ "$(uname -s)" != Darwin ] || ! xcrun --find swiftc >/dev/null 2>&1
  }

  debug_app_links_reproducibly() {
    cp -R "$REPO_ROOT/fixtures/xcode_wrapper_resources/." "$WORKSPACE/"
    perl -pi -e 's/SWIFT_VERSION = 5.0;/SWIFT_VERSION = 5.0; SWIFT_OPTIMIZATION_LEVEL = "-Onone";/' "$WORKSPACE/ThemeApp.xcodeproj/project.pbxproj"
    (close_shellspec_fds; STRUO_CHANGE_TRACKER=0 "$STRUO_BIN" -C "$WORKSPACE" --format json build ThemeApp --verify-reproducible) > "$WORKSPACE/verify.json" 2> "$WORKSPACE/verify.err" || { cat "$WORKSPACE/verify.err"; return 1; }
    grep -q '"reproducible":true' "$WORKSPACE/verify.json" || { cat "$WORKSPACE/verify.json"; return 1; }
  }

  It 'links an -Onone app to the same executable in independent cold builds'
    Skip if 'requires Xcode' xcode_unavailable
    When call debug_app_links_reproducibly
    The status should be success
    The output should equal ''
  End
End
