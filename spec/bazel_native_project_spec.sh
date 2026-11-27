#shellcheck shell=bash
# End-to-end specs for the graph Struo derives from a Bazel workspace.

Describe 'bazel native project'
  BeforeEach 'setup_workspace'
  AfterEach 'cleanup_workspace'

  setup_bazel_workspace() {
    cp -R "$REPO_ROOT/fixtures/bazel_rules_rust/." "$WORKSPACE/"
    chmod +x "$WORKSPACE/tools/bazel" "$WORKSPACE/tools/bazelisk"
  }

  target_ids() {
    env PATH="$WORKSPACE/tools:$PATH" "$STRUO_BIN" -C "$WORKSPACE" --format json query targets |
      jq -r '.[] | "\(.kind) \(.id)"' |
      sort
  }

  It 'lets an owning Bazel root suppress nested ecosystem examples'
    setup_bazel_workspace

    When call target_ids
    The status should be success
    The stdout should include 'bazel_workspace bazel'
    The stdout should include 'bazel_target bz_support'
    The stdout should include 'bazel_test bz_support_test'
    The stdout should not include 'cargo_workspace'
    The stdout should not include 'swift_package_workspace'
    The path "$WORKSPACE/struo.toml" should not be exist
  End

  It 'builds and tests the complete workspace without a Struo manifest'
    setup_bazel_workspace

    When call env PATH="$WORKSPACE/tools:$PATH" "$STRUO_BIN" -C "$WORKSPACE" test --quiet
    The status should be success
    The stdout should include 'test batches'
    The contents of file "$WORKSPACE/.struo/out/bz_support_test/test/test_results.json" should include '"status":"passed"'
    The path "$WORKSPACE/struo.toml" should not be exist
  End

  It 'builds the complete workspace through the detected root'
    setup_bazel_workspace

    When call env PATH="$WORKSPACE/tools:$PATH" "$STRUO_BIN" -C "$WORKSPACE" build --quiet
    The status should be success
    The stdout should include 'struo: build bazel (bazel_workspace)'
    The path "$WORKSPACE/struo.toml" should not be exist
  End

  It 'recognizes a WORKSPACE.bazel-only repository'
    setup_bazel_workspace
    mv "$WORKSPACE/MODULE.bazel" "$WORKSPACE/WORKSPACE.bazel"

    When call env PATH="$WORKSPACE/tools:$PATH" "$STRUO_BIN" -C "$WORKSPACE" --format json query targets
    The status should be success
    The stdout should include '"id":"bazel"'
    The stdout should include '"id":"bz_support"'
    The stdout should include '"id":"bz_support_test"'
    The path "$WORKSPACE/struo.toml" should not be exist
  End

  It 'uses the native test strategy when TestRunner argv cannot run on its own'
    setup_bazel_workspace
    cat > "$WORKSPACE/tools/bazelisk" <<'SH'
#!/bin/sh
case "$1" in
  --version) printf 'bazel 8.4.2\n' ;;
  query) printf 'sh_test rule //:support_test\n' ;;
  aquery) printf '%s\n' '{"actions":[{"mnemonic":"TestRunner","arguments":["/bin/sh","-c","exit 42"]}]}' ;;
  info) printf '%s\n' "${TMPDIR:-/tmp}/struo-bazel-output-base" ;;
  fetch|test) exit 0 ;;
  *) exit 2 ;;
esac
SH

    When call env PATH="$WORKSPACE/tools:$PATH" "$STRUO_BIN" -C "$WORKSPACE" --format json test bz_support_test
    The status should be success
    The stderr should be blank
    The stdout should include '"status":"completed"'
    The stdout should include '"cache":"bypass"'
  End

  owned_cache_roundtrip() {
    printf alpha > "$WORKSPACE/value.txt"
    cat > "$WORKSPACE/tools/copy" <<SH
#!/bin/sh
printf x >> "$WORKSPACE/executions"
/bin/cat "\$1" >> "\$2"
SH
    chmod +x "$WORKSPACE/tools/copy"
    cat > "$WORKSPACE/struo.toml" <<TOML
[[target]]
name = "bazel"
kind = "bazel_workspace"
srcs = ["MODULE.bazel"]
[target.attrs]
toolchain_inputs = ["$WORKSPACE/tools/copy", "/bin/sh", "/bin/cat"]
TOML
    cat > "$WORKSPACE/tools/bazelisk" <<SH
#!/bin/sh
case "\$1" in
  --version) printf 'bazel 9\n' ;;
  query) printf 'cc_library rule //:support\n' ;;
  info) printf '%s\n' "$WORKSPACE/native" ;;
  cquery) printf 'bazel-out/alias.txt\n' ;;
  aquery) cat <<'JSON'
{"pathFragments":[{"id":1,"label":"value.txt"},{"id":2,"label":"bazel-out/value.txt"},{"id":3,"label":"bazel-out/alias.txt"},{"id":4,"label":"bazel-out/unused"}],"artifacts":[{"id":1,"pathFragmentId":1},{"id":2,"pathFragmentId":2},{"id":3,"pathFragmentId":3},{"id":4,"pathFragmentId":4}],"depSetOfFiles":[{"id":1,"directArtifactIds":[1]},{"id":2,"directArtifactIds":[2]}],"actions":[{"mnemonic":"Copy","arguments":["$WORKSPACE/tools/copy","value.txt","bazel-out/value.txt"],"inputDepSetIds":[1],"outputIds":[2]},{"mnemonic":"SolibSymlink","inputDepSetIds":[2],"outputIds":[3]},{"mnemonic":"FileWrite","outputIds":[4]}]}
JSON
    ;;
  *) exit 42 ;;
esac
SH
    local output="$WORKSPACE/.struo/bazel-actions/bazel-out/alias.txt"
    mkdir -p "$WORKSPACE/.struo/bazel-actions/bazel-out"
    printf unrelated > "$WORKSPACE/.struo/bazel-actions/bazel-out/private"
    chmod 000 "$WORKSPACE/.struo/bazel-actions/bazel-out/private"
    env PATH="$WORKSPACE/tools:$PATH" "$STRUO_BIN" -C "$WORKSPACE" build bz_support --quiet > "$WORKSPACE/run.log" || return
    [ "$(cat "$output")" = alpha ] || return 1
    [ "$(cat "$WORKSPACE/executions")" = x ] || return 1
    [ ! -e "$WORKSPACE/.struo/bazel-actions/bazel-out/unused" ] || return 1
    rm -rf "$WORKSPACE/.struo/bazel-actions"
    env PATH="$WORKSPACE/tools:$PATH" "$STRUO_BIN" -C "$WORKSPACE" build bz_support --quiet > "$WORKSPACE/run.log" || return
    [ "$(cat "$output")" = alpha ] || return 1
    [ "$(cat "$WORKSPACE/executions")" = x ] || return 1
    printf beta > "$WORKSPACE/value.txt"
    env PATH="$WORKSPACE/tools:$PATH" "$STRUO_BIN" -C "$WORKSPACE" build bz_support --quiet > "$WORKSPACE/run.log" || return
    [ "$(cat "$output")" = beta ] || return 1
    [ "$(cat "$WORKSPACE/executions")" = xx ] || return 1
    printf 'restored without execution; invalidated changed input\n'
  }

  shared_producer_build() {
    cat > "$WORKSPACE/tools/produce" <<SH
#!/bin/sh
printf x >> "$WORKSPACE/executions"
sleep 1
printf shared > "\$1"
SH
    cat > "$WORKSPACE/tools/consume" <<'SH'
#!/bin/sh
/bin/cat "$1" > "$2"
SH
    chmod +x "$WORKSPACE/tools/produce" "$WORKSPACE/tools/consume"
    cat > "$WORKSPACE/tools/bazelisk" <<SH
#!/bin/sh
case "\$1" in
  --version) printf 'bazel 9\n' ;;
  query) case "\$2" in *manual*) ;; *) printf 'genrule rule //:a\ngenrule rule //:b\n' ;; esac ;;
  info) printf '%s\n' "$WORKSPACE/native" ;;
  cquery) printf 'bazel-out/%s.txt\n' "\${2#//:}" ;;
  aquery)
    name="\${2#deps(//:}"
    name="\${name%)}"
    cat <<JSON
{"pathFragments":[{"id":1,"label":"bazel-out/shared.txt"},{"id":2,"label":"bazel-out/\$name.txt"}],"artifacts":[{"id":1,"pathFragmentId":1},{"id":2,"pathFragmentId":2}],"depSetOfFiles":[{"id":1,"directArtifactIds":[1]}],"actions":[{"mnemonic":"Genrule","arguments":["$WORKSPACE/tools/consume","bazel-out/shared.txt","bazel-out/\$name.txt"],"inputDepSetIds":[1],"outputIds":[2]},{"mnemonic":"Genrule","arguments":["$WORKSPACE/tools/produce","bazel-out/shared.txt"],"outputIds":[1]}]}
JSON
    ;;
  *) exit 42 ;;
esac
SH
    env PATH="$WORKSPACE/tools:$PATH" "$STRUO_BIN" -C "$WORKSPACE" build --quiet > "$WORKSPACE/run.log" 2>&1 || { cat "$WORKSPACE/run.log"; return 1; }
    local out="$WORKSPACE/.struo/bazel-actions/bazel-out"
    [ "$(cat "$out/a.txt")" = shared ] && [ "$(cat "$out/b.txt")" = shared ] || return 1
    printf 'producer ran %s time(s)\n' "$(wc -c < "$WORKSPACE/executions" | tr -d ' ')"
  }

  It 'runs an action shared by several targets once per build'
    setup_bazel_workspace

    When call shared_producer_build
    The status should be success
    The stdout should equal 'producer ran 1 time(s)'
  End

  manual_targets_build() {
    cat > "$WORKSPACE/tools/bazelisk" <<SH
#!/bin/sh
case "\$1" in
  --version) printf 'bazel 9\n' ;;
  query)
    case "\$2" in
      *manual*) printf 'genrule rule //:subject\nsh_test rule //:manual_test\n' ;;
      *) printf 'genrule rule //:subject\nsh_test rule //:manual_test\ngenrule rule //:kept\nsh_test rule //:kept_test\n' ;;
    esac ;;
  info) printf '%s\n' "$WORKSPACE/native" ;;
  cquery) ;;
  aquery)
    case "\$2" in
      *kept*) printf '{}\n' ;;
      *) printf 'ERROR: analysis of a manual target\n' >&2; exit 1 ;;
    esac ;;
  *) exit 42 ;;
esac
SH
    env PATH="$WORKSPACE/tools:$PATH" "$STRUO_BIN" -C "$WORKSPACE" build --quiet > "$WORKSPACE/run.log" 2>&1 || { cat "$WORKSPACE/run.log"; return 1; }
    env PATH="$WORKSPACE/tools:$PATH" "$STRUO_BIN" -C "$WORKSPACE" --format json query target bazel |
      jq -c '[.attrs._default_test_roots, .deps]'
    env PATH="$WORKSPACE/tools:$PATH" "$STRUO_BIN" -C "$WORKSPACE" --format json query targets |
      jq -r '.[].id' | sort | tr '\n' ' '
  }

  It 'builds and tests the rules a Bazel wildcard selects, skipping manual tags'
    setup_bazel_workspace

    When call manual_targets_build
    The status should be success
    The line 1 of stdout should equal '[["bz_kept_test"],["bz_kept","bz_kept_test"]]'
    The line 2 of stdout should equal 'bazel bz_kept bz_kept_test bz_manual_test bz_subject '
  End

  executable_symlink_build() {
    cat > "$WORKSPACE/tools/write" <<'SH'
#!/bin/sh
printf '#!/bin/sh\necho ran\n' > "$1"
SH
    chmod +x "$WORKSPACE/tools/write"
    cat > "$WORKSPACE/tools/bazelisk" <<SH
#!/bin/sh
case "\$1" in
  --version) printf 'bazel 9\n' ;;
  query) case "\$2" in *manual*) ;; *) printf 'genrule rule //:out\n' ;; esac ;;
  info) printf '%s\n' "$WORKSPACE/native" ;;
  cquery) printf 'bazel-out/out.txt\n' ;;
  aquery) cat <<'JSON'
{"pathFragments":[{"id":1,"label":"bazel-out/gen.sh"},{"id":2,"label":"bazel-out/tool"},{"id":3,"label":"bazel-out/out.txt"}],"artifacts":[{"id":1,"pathFragmentId":1},{"id":2,"pathFragmentId":2},{"id":3,"pathFragmentId":3}],"depSetOfFiles":[{"id":1,"directArtifactIds":[1]},{"id":2,"directArtifactIds":[2]}],"actions":[{"mnemonic":"Genrule","arguments":["$WORKSPACE/tools/write","bazel-out/gen.sh"],"outputIds":[1]},{"mnemonic":"ExecutableSymlink","inputDepSetIds":[1],"outputIds":[2]},{"mnemonic":"Genrule","arguments":["/bin/sh","-c","bazel-out/tool > bazel-out/out.txt"],"inputDepSetIds":[2],"outputIds":[3]}]}
JSON
    ;;
  *) exit 42 ;;
esac
SH
    env PATH="$WORKSPACE/tools:$PATH" "$STRUO_BIN" -C "$WORKSPACE" build bz_out --quiet > "$WORKSPACE/run.log" 2>&1 || { cat "$WORKSPACE/run.log"; return 1; }
    cat "$WORKSPACE/.struo/bazel-actions/bazel-out/out.txt"
    rm -rf "$WORKSPACE/.struo/bazel-actions"
    env PATH="$WORKSPACE/tools:$PATH" "$STRUO_BIN" -C "$WORKSPACE" build bz_out --quiet > "$WORKSPACE/run.log" 2>&1 || { cat "$WORKSPACE/run.log"; return 1; }
    cat "$WORKSPACE/.struo/bazel-actions/bazel-out/out.txt"
  }

  It 'makes an executable symlink of a generated file executable like Bazel'
    setup_bazel_workspace

    When call executable_symlink_build
    The status should be success
    The line 1 of stdout should equal 'ran'
    The line 2 of stdout should equal 'ran'
  End

  executable_write_build() {
    cat > "$WORKSPACE/tools/bazelisk" <<SH
#!/bin/sh
case "\$1" in
  --version) printf 'bazel 9\n' ;;
  query) case "\$2" in *manual*) ;; *) printf 'genrule rule //:out\n' ;; esac ;;
  info) printf '%s\n' "$WORKSPACE/native" ;;
  cquery) printf 'bazel-out/out.txt\n' ;;
  aquery) cat <<'JSON'
{"pathFragments":[{"id":1,"label":"bazel-out/tool"},{"id":2,"label":"bazel-out/out.txt"}],"artifacts":[{"id":1,"pathFragmentId":1},{"id":2,"pathFragmentId":2}],"depSetOfFiles":[{"id":1,"directArtifactIds":[1]}],"actions":[{"mnemonic":"FileWrite","outputIds":[1],"fileContents":"#!/bin/sh\\necho written\\n","isExecutable":true},{"mnemonic":"Genrule","arguments":["/bin/sh","-c","bazel-out/tool > bazel-out/out.txt"],"inputDepSetIds":[1],"outputIds":[2]}]}
JSON
    ;;
  *) exit 42 ;;
esac
SH
    env PATH="$WORKSPACE/tools:$PATH" "$STRUO_BIN" -C "$WORKSPACE" build bz_out --quiet > "$WORKSPACE/run.log" 2>&1 || { cat "$WORKSPACE/run.log"; return 1; }
    cat "$WORKSPACE/.struo/bazel-actions/bazel-out/out.txt"
  }

  It 'writes an executable file write with its executable mode'
    setup_bazel_workspace

    When call executable_write_build
    The status should be success
    The stdout should equal 'written'
  End

  external_copy_digest() {
    local root="$1"
    mkdir -p "$root/tools" "$root/native/external/repo"
    cp "$REPO_ROOT/fixtures/bazel_rules_rust/MODULE.bazel" "$root/"
    printf 'printf ok > "$1"\n' > "$root/native/external/repo/tool.sh"
    cat > "$root/tools/bazelisk" <<SH
#!/bin/sh
case "\$1" in
  --version) printf 'bazel 9\n' ;;
  query) case "\$2" in *manual*) ;; *) printf 'genrule rule //:out\n' ;; esac ;;
  info) printf '%s\n' "$root/native" ;;
  cquery) printf 'bazel-out/out.txt\n' ;;
  aquery) cat <<'JSON'
{"pathFragments":[{"id":1,"label":"external/repo/tool.sh"},{"id":2,"label":"bazel-out/out.txt"}],"artifacts":[{"id":1,"pathFragmentId":1},{"id":2,"pathFragmentId":2}],"depSetOfFiles":[{"id":1,"directArtifactIds":[1]}],"actions":[{"mnemonic":"Genrule","arguments":["/bin/sh","external/repo/tool.sh","bazel-out/out.txt"],"inputDepSetIds":[1],"outputIds":[2]}]}
JSON
    ;;
  *) exit 42 ;;
esac
SH
    chmod +x "$root/tools/bazelisk"
    env PATH="$root/tools:$PATH" XDG_CACHE_HOME="$root/xdg" "$STRUO_BIN" -C "$root" build bz_out --quiet > "$root/run.log" 2>&1 || { cat "$root/run.log"; return 1; }
    env PATH="$root/tools:$PATH" XDG_CACHE_HOME="$root/xdg" "$STRUO_BIN" -C "$root" --format json query evidence |
      jq -r '.[] | select(.kind == "action_result" and ((.outputs // {}) | keys | any(endswith("external/repo/tool.sh")))) | .action_digest' | sort -u
  }

  external_copies_across_checkouts() {
    local first second
    first="$(external_copy_digest "$WORKSPACE/one")" || return 1
    second="$(external_copy_digest "$WORKSPACE/two/nested/deeper")" || return 1
    [ -n "$first" ] || { echo 'no external copy recorded'; return 1; }
    [ "$first" = "$second" ] && echo 'same key' || echo "different keys: $first vs $second"
  }

  It 'keys external repository files by content, not by the checkout-specific output base'
    When call external_copies_across_checkouts
    The status should be success
    The stdout should equal 'same key'
  End

  It 'restores owned outputs from cache and invalidates changed sources'
    setup_bazel_workspace

    When call owned_cache_roundtrip
    The status should be success
    The stdout should equal 'restored without execution; invalidated changed input'
    The stderr should be blank
  End

  owned_test_runfiles() {
    mkdir -p "$WORKSPACE/hidden"
    printf payload > "$WORKSPACE/hidden/source.txt"
    cat > "$WORKSPACE/launcher.sh" <<'SH'
#!/bin/sh
test_binary="$1"
shift
exec "$TEST_SRCDIR/_main/$test_binary" "$@"
SH
    cat > "$WORKSPACE/test.sh" <<SH
#!/bin/sh
[ "\$#" = 1 ] && [ "\$1" = expected ] && [ "\$DECLARED" = runtime ] &&
[ "\$TEST_TOTAL_SHARDS" = 2 ] &&
[ "\$(cat "\$TEST_SRCDIR/_main/data/renamed.txt")" = payload ] &&
[ -f "\$TEST_SRCDIR/_main/empty" ] || exit 1
printf '%s\n' "\$TEST_SHARD_INDEX" >> "$WORKSPACE/shards"
SH
    chmod +x "$WORKSPACE/launcher.sh" "$WORKSPACE/test.sh"
    cat > "$WORKSPACE/struo.toml" <<'TOML'
[[target]]
name = "bazel"
kind = "bazel_workspace"
srcs = ["MODULE.bazel"]
[target.attrs.test_env]
DECLARED = "runtime"
TOML
    # Bazel reports paths under the physical directory it runs in, which on
    # macOS is /private/var/... for a /var/... temporary workspace.
    local physical
    physical="$(cd "$WORKSPACE" && pwd -P)"
    cat > "$WORKSPACE/tools/bazelisk" <<SH
#!/bin/sh
case "\$1" in
  --version) printf 'bazel 9\n' ;;
  query)
    case "\$*" in
      *streamed_jsonproto*) printf '%s\n' '{"rule":{"attribute":[{"name":"shard_count","intValue":2},{"name":"args","stringListValue":["expected"]}]}}' ;;
      *) printf 'sh_test rule //:support_test\n' ;;
    esac ;;
  info) printf '%s\n' "$physical/native" ;;
  cquery) case "\$*" in *RunEnvironmentInfo*) printf '{}\n' ;; *) printf 'bazel-out/test.runfiles\n' ;; esac ;;
  aquery) cat <<JSON
{"pathFragments":[{"id":1,"label":"launcher.sh"},{"id":2,"label":"test.sh"},{"id":3,"label":"hidden/source.txt"},{"id":4,"label":"bazel-out/test.runfiles_manifest"},{"id":5,"label":"bazel-out/test.runfiles/MANIFEST"},{"id":6,"label":"bazel-out/test.runfiles"},{"id":7,"label":"bazel-out/testlogs/shard_1/test.log"},{"id":8,"label":"bazel-out/testlogs/shard_2/test.log"}],
"artifacts":[{"id":1,"pathFragmentId":1},{"id":2,"pathFragmentId":2},{"id":3,"pathFragmentId":3},{"id":4,"pathFragmentId":4},{"id":5,"pathFragmentId":5},{"id":6,"pathFragmentId":6},{"id":7,"pathFragmentId":7},{"id":8,"pathFragmentId":8}],
"depSetOfFiles":[{"id":1,"directArtifactIds":[4]},{"id":2,"directArtifactIds":[5,2,3]},{"id":3,"directArtifactIds":[1,6]}],
"actions":[
{"mnemonic":"SourceSymlinkManifest","outputIds":[4],"fileContents":"_main/bin/test.sh $physical/test.sh\\n_main/data/renamed.txt $physical/hidden/source.txt\\n_main/empty \\n"},
{"mnemonic":"SymlinkTree","inputDepSetIds":[1],"outputIds":[5]},
{"mnemonic":"RunfilesTree","inputDepSetIds":[2],"outputIds":[6]},
{"mnemonic":"TestRunner","arguments":["./launcher.sh","bin/test.sh","expected"],"inputDepSetIds":[3],"outputIds":[7]},
{"mnemonic":"TestRunner","arguments":["./launcher.sh","bin/test.sh","expected"],"inputDepSetIds":[3],"outputIds":[8]}]}
JSON
    ;;
  *) exit 42 ;;
esac
SH
    env PATH="$WORKSPACE/tools:$PATH" "$STRUO_BIN" -C "$WORKSPACE" test bz_support_test --quiet > "$WORKSPACE/run.log" || return
    [ "$(sort "$WORKSPACE/shards" | tr '\n' ',')" = '0,1,' ] || return 1
    local tree="$WORKSPACE/.struo/bazel-actions/bazel-out/test.runfiles"
    printf stale > "$tree/stale"
    env PATH="$WORKSPACE/tools:$PATH" "$STRUO_BIN" -C "$WORKSPACE" build bz_support_test --quiet > "$WORKSPACE/run.log" || return
    [ ! -e "$tree/stale" ] || return 1
    rm "$WORKSPACE/shards"
    rm -rf "$WORKSPACE/.struo/bazel-actions"
    env PATH="$WORKSPACE/tools:$PATH" "$STRUO_BIN" -C "$WORKSPACE" test bz_support_test --quiet > "$WORKSPACE/run.log" || return
    [ "$(sort "$WORKSPACE/shards" | tr '\n' ',')" = '0,1,' ] || return 1
    printf 'ran both shards with mapped runfiles after restore\n'
  }

  bazel8_test_runfiles() {
    mkdir -p "$WORKSPACE/hidden"
    printf payload > "$WORKSPACE/hidden/source.txt"
    cat > "$WORKSPACE/launcher.sh" <<'SH'
#!/bin/sh
exec "$TEST_SRCDIR/_main/$1"
SH
    cat > "$WORKSPACE/test.sh" <<SH
#!/bin/sh
[ "\$(cat "\$TEST_SRCDIR/_main/data/renamed.txt")" = payload ] || exit 1
[ "\$MESSAGE" = 'Hello, foo' ] || exit 1
[ "\$(cat "\$TEST_SRCDIR/_main/data/gen.txt")" = generated ] || exit 1
printf ran >> "$WORKSPACE/ran"
SH
    chmod +x "$WORKSPACE/launcher.sh" "$WORKSPACE/test.sh"
    local physical
    physical="$(cd "$WORKSPACE" && pwd -P)"
    cat > "$WORKSPACE/tools/bazelisk" <<SH
#!/bin/sh
case "\$1" in
  --version) printf 'bazel 8\n' ;;
  query)
    case "\$*" in
      *streamed_jsonproto*) printf '%s\n' '{"rule":{"attribute":[{"name":"env","stringDictValue":[{"key":"MESSAGE","value":"Hello, \$(NAME)"}]}]}}' ;;
      *manual*) ;;
      *) printf 'sh_test rule //:support_test\n' ;;
    esac ;;
  info) printf '%s\n' "$physical/native" ;;
  cquery)
    case "\$*" in
      *RunEnvironmentInfo*) printf '%s\n' '{"environment":{"MESSAGE":"Hello, foo"},"inherited":[]}' ;;
      *) printf 'bazel-out/test.sh\n' ;;
    esac ;;
  aquery) cat <<JSON
{"pathFragments":[{"id":1,"label":"launcher.sh"},{"id":2,"label":"test.sh"},{"id":3,"label":"hidden/source.txt"},{"id":4,"label":"bazel-out/test.sh.runfiles_manifest"},{"id":5,"label":"bazel-out/test.sh.runfiles/MANIFEST"},{"id":6,"label":"bazel-out/_middlemen/test.sh-runfiles"},{"id":7,"label":"bazel-out/testlogs/test.log"},{"id":8,"label":"bazel-out/test.sh"},{"id":9,"label":"bazel-out/gen.txt"},{"id":10,"label":"bazel-out/pre.txt"}],
"artifacts":[{"id":1,"pathFragmentId":1},{"id":2,"pathFragmentId":2},{"id":3,"pathFragmentId":3},{"id":4,"pathFragmentId":4},{"id":5,"pathFragmentId":5},{"id":6,"pathFragmentId":6},{"id":7,"pathFragmentId":7},{"id":8,"pathFragmentId":8},{"id":9,"pathFragmentId":9},{"id":10,"pathFragmentId":10}],
"depSetOfFiles":[{"id":1,"directArtifactIds":[4]},{"id":2,"directArtifactIds":[8,5,3,9]},{"id":3,"directArtifactIds":[1,6]},{"id":4,"directArtifactIds":[2]},{"id":5,"directArtifactIds":[10]}],
"actions":[
{"mnemonic":"ExecutableSymlink","inputDepSetIds":[4],"outputIds":[8]},
{"mnemonic":"SourceSymlinkManifest","outputIds":[4],"fileContents":"_main/bin/test.sh $physical/test.sh\\n_main/data/renamed.txt $physical/hidden/source.txt\\n_main/data/gen.txt $physical/native/bazel-out/gen.txt\\n"},
{"mnemonic":"SymlinkTree","inputDepSetIds":[1],"outputIds":[5]},
{"mnemonic":"Middleman","inputDepSetIds":[2],"outputIds":[6]},
{"mnemonic":"TestRunner","arguments":["./launcher.sh","bin/test.sh"],"inputDepSetIds":[3],"outputIds":[7]},
{"mnemonic":"Genrule","arguments":["/bin/sh","-c","printf generated > bazel-out/pre.txt"],"outputIds":[10]},
{"mnemonic":"Genrule","arguments":["/bin/cp","bazel-out/pre.txt","bazel-out/gen.txt"],"inputDepSetIds":[5],"outputIds":[9]}]}
JSON
    ;;
  *) exit 42 ;;
esac
SH
    env PATH="$WORKSPACE/tools:$PATH" "$STRUO_BIN" -C "$WORKSPACE" test bz_support_test --quiet > "$WORKSPACE/run.log" 2>&1 || { cat "$WORKSPACE/run.log"; return 1; }
    cat "$WORKSPACE/ran"
  }

  It 'runs an owned Bazel 8 test with runfiles behind a middleman and its analyzed environment'
    setup_bazel_workspace

    When call bazel8_test_runfiles
    The status should be success
    The stdout should equal 'ran'
  End

  It 'runs owned test shards with manifest-mapped runfiles and explicit environment'
    setup_bazel_workspace

    When call owned_test_runfiles
    The status should be success
    The stdout should equal 'ran both shards with mapped runfiles after restore'
    The stderr should be blank
  End
End
