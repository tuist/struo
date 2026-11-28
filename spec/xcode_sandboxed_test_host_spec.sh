#shellcheck shell=bash

Describe 'Xcode sandboxed macOS test host'
  BeforeEach 'setup_workspace'
  AfterEach 'cleanup_workspace'

  xcode_unavailable() {
    [ "$(uname -s)" != Darwin ] || ! xcrun --find xcodebuild >/dev/null 2>&1
  }

  test_sandboxed_host() {
    cp -R "$REPO_ROOT/fixtures/xcode_sandboxed_test_host/." "$WORKSPACE/"
    (close_shellspec_fds; STRUO_CHANGE_TRACKER=0 "$STRUO_BIN" -C "$WORKSPACE" test HostTests --format json) > "$WORKSPACE/test.json" 2> "$WORKSPACE/test.err" || { tail -20 "$WORKSPACE/test.err"; return 1; }
    entitlements=$(codesign -d --entitlements - --xml "$WORKSPACE/.struo/out/Host/Host App.app" 2>/dev/null) || return 1
    if printf '%s\n' "$entitlements" | grep -q testmanagerd; then
      echo 'built app gained testing entitlements'
    else
      echo 'built app entitlements unchanged'
    fi
  }

  It 'imports a differently named sandboxed host without changing its built entitlements'
    Skip if 'requires Xcode' xcode_unavailable
    When call test_sandboxed_host
    The status should be success
    The output should equal 'built app entitlements unchanged'
  End
End
