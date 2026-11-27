#shellcheck shell=bash

Describe 'Xcode recommended deployment target'
  BeforeEach 'setup_workspace'
  AfterEach 'cleanup_workspace'

  xcode_unavailable() {
    [ "$(uname -s)" != Darwin ] || ! xcrun --find swiftc >/dev/null 2>&1
  }

  recommended_minimum_os() {
    cp -R "$REPO_ROOT/fixtures/xcode_wrapper_resources/." "$WORKSPACE/"
    perl -pi -e 's/MACOSX_DEPLOYMENT_TARGET = 13.0;/MACOSX_DEPLOYMENT_TARGET = "\$(RECOMMENDED_MACOSX_DEPLOYMENT_TARGET)";/' "$WORKSPACE/ThemeApp.xcodeproj/project.pbxproj"
    sdk="$(xcrun --sdk macosx --show-sdk-path)"
    expected="$(plutil -extract SupportedTargets.macosx.RecommendedDeploymentTarget raw -o - "$sdk/SDKSettings.plist")"
    (close_shellspec_fds; STRUO_CHANGE_TRACKER=0 "$STRUO_BIN" -C "$WORKSPACE" query target ThemeApp) > "$WORKSPACE/target.txt" 2> "$WORKSPACE/query.err" || { cat "$WORKSPACE/query.err"; return 1; }
    actual="$(sed -n 's/^  minimum_os = String("\(.*\)")$/\1/p' "$WORKSPACE/target.txt")"
    [ "$actual" = "$expected" ] || { echo "minimum_os $actual, SDK recommends $expected"; return 1; }
  }

  It 'resolves RECOMMENDED_MACOSX_DEPLOYMENT_TARGET from the selected SDK'
    Skip if 'requires Xcode' xcode_unavailable
    When call recommended_minimum_os
    The status should be success
    The output should equal ''
  End
End
