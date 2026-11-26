#shellcheck shell=bash

Describe 'Xcode generated Info.plist'
  BeforeEach 'setup_workspace'
  AfterEach 'cleanup_workspace'

  watchos_toolchain_unavailable() {
    [ "$(uname -s)" != Darwin ] || ! xcrun --sdk watchsimulator --show-sdk-path >/dev/null 2>&1
  }

  plist_value() {
    plutil -extract "$2" "${3:-raw}" -o - "$1"
  }

  build_generated_info_plists() {
    cp -R "$REPO_ROOT/fixtures/xcode_generated_info_plist/." "$WORKSPACE/"
    STRUO_CHANGE_TRACKER=0 struo build --format json > "$WORKSPACE/build.json" 2> "$WORKSPACE/build.err" || { cat "$WORKSPACE/build.err"; return 1; }
    phone="$WORKSPACE/.struo/out/ReproApp/ReproApp.app/Info.plist"
    watch="$WORKSPACE/.struo/out/WatchApp/WatchApp.app/Info.plist"
    plutil -lint "$phone" "$watch" >/dev/null || return 1

    [ "$(plist_value "$phone" CFBundleDisplayName)" = 'Repro & Display' ] || return 1
    [ "$(plist_value "$phone" ReproTemplateKey)" = 'from template' ] || return 1
    [ "$(plist_value "$phone" UILaunchScreen json)" = '{"UILaunchScreen":{}}' ] || return 1
    [ "$(plist_value "$phone" CFBundleDevelopmentRegion)" = en ] || return 1
    [ "$(plist_value "$phone" UIApplicationSupportsIndirectInputEvents)" = true ] || return 1
    [ "$(plist_value "$phone" 'UISupportedInterfaceOrientations~ipad' json)" = '["UIInterfaceOrientationPortrait","UIInterfaceOrientationLandscapeLeft"]' ] || return 1
    [ "$(plist_value "$phone" LSRequiresIPhoneOS)" = true ] || return 1
    [ "$(plist_value "$phone" CFBundleExecutable)" = ReproApp ] || return 1
    [ "$(plist_value "$phone" CFBundleVersion)" = 7 ] || return 1
    [ "$(plist_value "$phone" CFBundleShortVersionString)" = 2.1 ] || return 1
    [ "$(plist_value "$phone" UIDeviceFamily json)" = '[1,2]' ] || return 1

    [ "$(plist_value "$watch" WKApplication)" = true ] || return 1
    [ "$(plist_value "$watch" WKCompanionAppBundleIdentifier)" = dev.example.repro ] || return 1
    [ "$(plist_value "$watch" UISupportedInterfaceOrientations json)" = '["UIInterfaceOrientationPortrait"]' ] || return 1
    [ "$(plist_value "$watch" UIDeviceFamily json)" = '[4]' ] || return 1
    [ "$(plist_value "$watch" 'MinimumOSVersion~ipad')" = 9.0 ] || return 1
    ! plist_value "$watch" LSRequiresIPhoneOS >/dev/null 2>&1 || return 1
    ! plist_value "$watch" CFBundleVersion >/dev/null 2>&1
  }

  It 'matches Xcode: INFOPLIST_KEY settings, template merging, versions, and platform keys'
    Skip if 'requires Xcode with the watchOS SDK' watchos_toolchain_unavailable
    When call build_generated_info_plists
    The status should be success
    The output should equal ''
  End
End
