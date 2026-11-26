#shellcheck shell=bash
# End-to-end specs for `struo cache gc`.

Describe 'struo cache gc'
  BeforeEach 'setup_workspace'
  AfterEach 'cleanup_workspace'

  It 'is a no-op on an empty workspace'
    When call struo cache gc --max-size 1GB
    The status should be success
    The stdout should include '0 entries removed'
  End

  It 'leaves the cache intact when it already fits the budget'
    struo exec -e PATH=/usr/bin:/bin -- /bin/sh -c 'printf hello' >/dev/null 2>&1
    When call struo cache gc --max-size 1GiB
    The status should be success
    The stdout should include 'reclaimed 0 bytes'
  End

  It 'reclaims the whole store under a zero budget'
    struo exec -e PATH=/usr/bin:/bin -- /bin/sh -c 'printf hello' >/dev/null 2>&1
    struo cache gc --max-size 0 >/dev/null 2>&1
    # After a full reclaim the store reports zero blobs and actions.
    When call struo cache stats
    The status should be success
    The stdout should include 'blobs:   0'
    The stdout should include 'actions: 0'
  End

  It 'previews reclaimable space with --dry-run without deleting'
    struo exec -e PATH=/usr/bin:/bin -- /bin/sh -c 'printf hello' >/dev/null 2>&1
    struo cache gc --max-size 0 --dry-run >/dev/null 2>&1
    # The store is untouched: blobs and the action are still present.
    When call struo cache stats
    The status should be success
    The stdout should not include 'blobs:   0'
  End

  trim_in_background() {
    env STRUO_CACHE_MAX_SIZE=0 "$STRUO_BIN" -C "$WORKSPACE" exec -e PATH=/usr/bin:/bin -- /bin/sh -c 'printf old' >/dev/null 2>&1
    find "$XDG_CACHE_HOME/struo/cas" -type f -exec touch -t 202001010000 {} +
    env STRUO_CACHE_MAX_SIZE=1 "$STRUO_BIN" -C "$WORKSPACE" exec -e PATH=/usr/bin:/bin -- /bin/sh -c 'printf new' >/dev/null 2>&1
    # The command returns before its detached collection finishes.
    for _ in $(seq 1 100); do
      struo cache stats | grep -q 'actions: 1 ' && break
      sleep 0.1
    done
    struo cache stats
  }

  It 'trims entries unused for longer than the grace period once the cache exceeds STRUO_CACHE_MAX_SIZE'
    When call trim_in_background
    The status should be success
    The stdout should include 'actions: 1 '
  End

  It 'defaults to [local_cache] max_size from struo.toml'
    printf '[local_cache]\nmax_size = "3GB"\n' > "$WORKSPACE/struo.toml"
    When call "$STRUO_BIN" --format json -C "$WORKSPACE" cache gc --dry-run
    The status should be success
    The stdout should include '"max_size":3000000000'
  End

  It 'lets STRUO_CACHE_MAX_SIZE override struo.toml'
    printf '[local_cache]\nmax_size = "3GB"\n' > "$WORKSPACE/struo.toml"
    When call env STRUO_CACHE_MAX_SIZE=5GB "$STRUO_BIN" --format json -C "$WORKSPACE" cache gc --dry-run
    The status should be success
    The stdout should include '"max_size":5000000000'
  End

  It 'rejects a malformed [local_cache] max_size when the manifest loads'
    printf '[local_cache]\nmax_size = "lots"\n' > "$WORKSPACE/struo.toml"
    When call struo exec -e PATH=/usr/bin:/bin -- /bin/sh -c 'printf hi'
    The status should not equal 0
    The stderr should include 'invalid size `lots`'
  End

  warn_twice_about_a_malformed_environment_cap() {
    env STRUO_CACHE_MAX_SIZE=0 "$STRUO_BIN" -C "$WORKSPACE" exec -e PATH=/usr/bin:/bin -- /bin/sh -c 'printf a' >/dev/null 2>&1
    touch -t 202001010000 "$XDG_CACHE_HOME/struo/cas/gc.lock"
    for _ in 1 2; do
      env STRUO_CACHE_MAX_SIZE=lots "$STRUO_BIN" -C "$WORKSPACE" exec -e PATH=/usr/bin:/bin -- /bin/sh -c 'printf b' 2>&1 >/dev/null |
        grep -c 'skipped automatic cache cleanup' || true
    done
  }

  It 'warns about a malformed STRUO_CACHE_MAX_SIZE once per check, without failing the command'
    When call warn_twice_about_a_malformed_environment_cap
    The status should be success
    The stdout should equal "$(printf '1\n0')"
  End

  It 'rejects a malformed size'
    When call struo cache gc --max-size not-a-size
    The status should not equal 0
    The stderr should include 'max-size'
  End

  It 'emits a structured record under --format json'
    When call "$STRUO_BIN" --format json -C "$WORKSPACE" cache gc --max-size 500MB
    The status should be success
    The stdout should include '"bytes_reclaimed":'
    The stdout should include '"removed":'
    The stdout should include '"max_size":500000000'
  End
End
