#shellcheck shell=bash

Describe 'Xcode project paths outside the project directory'
  BeforeEach 'setup_workspace'
  AfterEach 'cleanup_workspace'

  xcode_unavailable() {
    [ "$(uname -s)" != Darwin ] || ! xcrun --find swiftc >/dev/null 2>&1
  }

  # actool starts an ibtoold daemon that outlives the build. Close ShellSpec's
  # private pipes first so the daemon cannot hold the example open until it idles out.
  build_repro() {
    (close_shellspec_fds; STRUO_CHANGE_TRACKER=0 "$STRUO_BIN" -C "$1" build "$2" --format json) > "$WORKSPACE/build.json" 2> "$WORKSPACE/build.err" || { cat "$WORKSPACE/build.err"; return 1; }
  }

  build_sibling_paths() {
    cp -R "$REPO_ROOT/fixtures/xcode_parent_relative_paths/." "$WORKSPACE/"
    build_repro "$WORKSPACE" app/ReproApp || return
    app="$(find "$WORKSPACE/.struo/out" -type d -name ReproApp.app | head -n 1)"
    [ -n "$app" ] || return 1
    [ "$(plutil -extract ReproInfoPlistSource raw "$app/Contents/Info.plist")" = shared ] || return 1
    [ "$(cat "$app/Contents/Resources/greeting.txt")" = 'shared resource copied' ] || return 1
    [ -s "$app/Contents/Resources/Assets.car" ] || return 1
    "$app/Contents/MacOS/ReproApp" 2>/dev/null
  }

  build_from_project_directory() {
    cp -R "$REPO_ROOT/fixtures/xcode_parent_relative_paths/." "$WORKSPACE/"
    build_repro "$WORKSPACE/app" ReproApp || return
    app="$(find "$WORKSPACE/app/.struo/out" -type d -name ReproApp.app | head -n 1)"
    [ -n "$app" ] || return 1
    [ "$(plutil -extract ReproInfoPlistSource raw "$app/Contents/Info.plist")" = shared ] || return 1
    [ "$(cat "$app/Contents/Resources/greeting.txt")" = 'shared resource copied' ] || return 1
    [ -s "$app/Contents/Resources/Assets.car" ] || return 1
    "$app/Contents/MacOS/ReproApp" 2>/dev/null
  }

  It 'builds sources, resources, asset catalogs, and Info.plist that live outside the workspace'
    Skip if 'requires Xcode' xcode_unavailable
    When call build_from_project_directory
    The status should be success
    The output should equal 'shared sources compiled'
  End

  It 'resolves ../ build settings and group paths against the project directory'
    Skip if 'requires Xcode' xcode_unavailable
    When call build_sibling_paths
    The status should be success
    The output should equal 'shared sources compiled'
  End
End
