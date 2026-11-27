#shellcheck shell=bash

Describe 'Xcode remote package without a committed Package.resolved'
  BeforeEach 'setup_workspace'
  AfterEach 'cleanup_workspace'

  xcode_unavailable() {
    [ "$(uname -s)" != Darwin ] || ! xcrun --find xcodebuild >/dev/null 2>&1
  }

  # SwiftPM remembers each package version's revision for its URL across runs,
  # so every run must create the same commits.
  tag_greeting() {
    printf 'public let greeting = "%s"\n' "$1" > "$origin/Sources/Greeting/Greeting.swift"
    git -C "$origin" add . &&
      GIT_AUTHOR_DATE='2026-01-01T00:00:00Z' GIT_COMMITTER_DATE='2026-01-01T00:00:00Z' git -C "$origin" commit -qm "$1" &&
      git -C "$origin" tag "$1"
  }

  prepare_origin() {
    cp -R "$REPO_ROOT/fixtures/xcode_unlocked_remote_package/." "$WORKSPACE/"
    origin="$SPEC_ROOT/Greeting"
    mv "$WORKSPACE/Greeting" "$origin"
    git -C "$origin" init -q && git -C "$origin" config user.name Struo && git -C "$origin" config user.email struo@example.invalid || return
    tag_greeting 1.0.0 && tag_greeting 1.1.0 && tag_greeting 2.0.0 || return
    # The project names an https package; serve it from the local repository.
    git config --global url."$origin".insteadOf https://struo.invalid/Greeting
  }

  build_unlocked_app() {
    prepare_origin || return
    (close_shellspec_fds; STRUO_CHANGE_TRACKER=0 "$STRUO_BIN" -C "$WORKSPACE" build App --format json) > "$WORKSPACE/build.json" 2> "$WORKSPACE/build.err" || { cat "$WORKSPACE/build.err"; return 1; }
    "$WORKSPACE/.struo/out/App/App.app/Contents/MacOS/App"
  }

  build_locked_app() {
    prepare_origin || return
    lockfile="$WORKSPACE/App.xcodeproj/project.xcworkspace/xcshareddata/swiftpm/Package.resolved"
    mkdir -p "$(dirname "$lockfile")"
    jq -n --arg revision "$(git -C "$origin" rev-parse 1.0.0)" '{version: 3, pins: [{identity: "greeting", kind: "remoteSourceControl", location: "https://struo.invalid/Greeting", state: {revision: $revision, version: "1.0.0"}}]}' > "$lockfile"
    cp "$lockfile" "$SPEC_ROOT/committed.resolved"
    (close_shellspec_fds; STRUO_CHANGE_TRACKER=0 "$STRUO_BIN" -C "$WORKSPACE" build App --format json) > "$WORKSPACE/build.json" 2> "$WORKSPACE/build.err" || { cat "$WORKSPACE/build.err"; return 1; }
    cmp -s "$lockfile" "$SPEC_ROOT/committed.resolved" || { echo 'Package.resolved was rewritten'; return 1; }
    "$WORKSPACE/.struo/out/App/App.app/Contents/MacOS/App"
  }

  It 'builds the committed Package.resolved pin without rewriting it'
    Skip if 'requires Xcode' xcode_unavailable
    When call build_locked_app
    The status should be success
    The output should equal '1.0.0'
  End

  It 'resolves the project package requirements like Xcode and builds against the resolved version'
    Skip if 'requires Xcode' xcode_unavailable
    When call build_unlocked_app
    The status should be success
    The output should equal '1.1.0'
  End
End
