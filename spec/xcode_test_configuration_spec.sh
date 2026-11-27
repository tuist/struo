#shellcheck shell=bash

Describe 'Xcode scheme test configuration'
  BeforeEach 'setup_workspace'
  AfterEach 'cleanup_workspace'

  xcode_unavailable() {
    [ "$(uname -s)" != Darwin ] || ! xcrun --find xcodebuild >/dev/null 2>&1
  }

  # The scheme tests with the Test configuration, which defines TEST for the
  # app and its framework, while the app itself still builds as Debug.
  test_with_scheme_configuration() {
    cp -R "$REPO_ROOT/fixtures/xcode_test_configuration/." "$WORKSPACE/"
    (
      close_shellspec_fds
      export STRUO_CHANGE_TRACKER=0
      "$STRUO_BIN" -C "$WORKSPACE" test ReaderTests --format json > "$WORKSPACE/test.json" &&
        "$STRUO_BIN" -C "$WORKSPACE" build Reader --format json > /dev/null
    ) 2> "$WORKSPACE/err" || { tail -20 "$WORKSPACE/err"; return 1; }
    strings "$WORKSPACE/.struo/out/Reader/Reader.app/Contents/MacOS/Reader" | grep -o 'reader-[a-z]*-configuration'
    strings "$WORKSPACE/.struo/out/Reader/Reader.app/Contents/Frameworks/Store.framework/Store" | grep -o 'store-[a-z]*-configuration'
  }

  It 'builds tests and their dependencies with the scheme Test configuration'
    Skip if 'requires Xcode' xcode_unavailable
    When call test_with_scheme_configuration
    The status should be success
    The line 1 of output should equal 'reader-debug-configuration'
    The line 2 of output should equal 'store-debug-configuration'
  End
End
