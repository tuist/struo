#shellcheck shell=bash

Describe 'Xcode projects in different checkout paths'
  BeforeEach 'setup_workspace'
  AfterEach 'cleanup_workspace'

  xcode_unavailable() {
    [ "$(uname -s)" != Darwin ] || ! xcrun --find swiftc >/dev/null 2>&1
  }

  # Each checkout starts from an empty cache in the same cache location, so
  # the environment the build inherits differs only in the checkout path.
  build_checkout() {
    root="$WORKSPACE/$1"
    rm -rf "$WORKSPACE/xdg"
    mkdir -p "$root/checkout" "$WORKSPACE/xdg"
    cp -R "$REPO_ROOT/fixtures/xcode_script_phases/." "$root/checkout/"
    (
      close_shellspec_fds
      export XDG_CACHE_HOME="$WORKSPACE/xdg/cache" XDG_STATE_HOME="$WORKSPACE/xdg/state" XDG_DATA_HOME="$WORKSPACE/xdg/data" XDG_CONFIG_HOME="$WORKSPACE/xdg/config" STRUO_CHANGE_TRACKER=0
      cd "$root/checkout" &&
        "$STRUO_BIN" --format json query graph-fingerprint | jq -r '.digest' > "$root/fingerprint" &&
        "$STRUO_BIN" --format json build Script_App > /dev/null &&
        "$STRUO_BIN" --format json query evidence |
          jq -r '[.[] | select(.kind == "action_result") | .action_digest] | unique | .[]' > "$root/actions"
    ) 2> "$root/err" || { cat "$root/err"; return 1; }
  }

  checkouts_share_keys() {
    build_checkout first || return
    build_checkout second-path || return
    cmp -s "$WORKSPACE/first/fingerprint" "$WORKSPACE/second-path/fingerprint" || { echo 'graph fingerprints differ'; return 1; }
    [ -s "$WORKSPACE/first/actions" ] || { echo 'no actions recorded'; return 1; }
    diff "$WORKSPACE/first/actions" "$WORKSPACE/second-path/actions" > /dev/null || { echo "$(diff "$WORKSPACE/first/actions" "$WORKSPACE/second-path/actions" | grep -c '^<') action keys differ"; return 1; }
  }

  It 'keys script phases and builds independently of the checkout path'
    Skip if 'requires Xcode' xcode_unavailable
    When call checkouts_share_keys
    The status should be success
    The output should equal ''
  End
End
