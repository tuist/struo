#shellcheck shell=bash

Describe 'Xcode app icon setting'
  BeforeEach 'setup_workspace'
  AfterEach 'cleanup_workspace'

  app_icon_toolchain_unavailable() {
    [ "$(uname -s)" != Darwin ] || ! xcrun --sdk iphonesimulator --find actool >/dev/null 2>&1
  }

  build_app() {
    # actool starts an ibtoold daemon that outlives the build; without this
    # it inherits ShellSpec's pipes and holds the spec open until it idles out.
    (
      for fd in /dev/fd/*; do
        fd="${fd##*/}"
        case "$fd" in 0|1|2|*[!0-9]*) continue ;; esac
        eval "exec ${fd}>&-"
      done
      STRUO_CHANGE_TRACKER=0 RUST_LOG=off struo build app/ReproApp --format json > "$WORKSPACE/build.json" 2> "$WORKSPACE/build.err" < /dev/null
    ) || { cat "$WORKSPACE/build.err"; return 1; }
    (cd "$app" && find . | sort) > "$WORKSPACE/bundle-$1.txt"
  }

  build_icon_only_catalog() {
    cp -R "$REPO_ROOT/fixtures/xcode_app_icon/." "$WORKSPACE/"
    app="$WORKSPACE/.struo/out/app/ReproApp/ReproApp.app"
    build_app cold || return 1
    build_app warm || return 1
    diff "$WORKSPACE/bundle-cold.txt" "$WORKSPACE/bundle-warm.txt" || return 1
    [ -s "$app/Assets.car" ] || return 1
    plutil -extract CFBundleIcons.CFBundlePrimaryIcon.CFBundleIconName raw -o - "$app/Info.plist"
  }

  It 'compiles the icon of a nested project into the bundle and its Info.plist'
    Skip if 'requires Xcode' app_icon_toolchain_unavailable
    When call build_icon_only_catalog
    The status should be success
    The output should equal 'AppIcon'
  End
End
