#shellcheck shell=bash
# End-to-end specs for Starlark analysis-backed graph actions.

Describe 'graph declared actions'
  BeforeEach 'setup_workspace'
  AfterEach 'cleanup_workspace'

  create_declared_action_workspace() {
    mkdir -p "$WORKSPACE/modules" "$WORKSPACE/tools/demo"
    cat > "$WORKSPACE/struo.toml" <<'EOF'
[modules]
paths = ["modules/*.star"]
EOF
    cat > "$WORKSPACE/modules/demo.star" <<'EOF'
def _demo_impl(ctx):
    out = declare_output(ctx["capability"] + ".txt")
    content = ctx["attr"]["message"] + ":" + ctx["capability"]
    run_action(
        argv = [host_which("sh"), "-c", "printf \"%s\" \"$1\" > \"$2\"", "sh", content, out],
        outputs = [out],
        identifier = ctx["label"]["id"] + ":" + ctx["capability"],
    )
    return {"out": out}

demo_action = target_kind(
    docs = "Demo action kind",
    attrs = [
        attr("message", "string", required = True, docs = "Message to emit"),
    ],
    providers = ["demo_output"],
    capabilities = [
        capability("build", ["default"]),
        capability("run", ["default"]),
        capability("test", ["default"]),
    ],
    impl = _demo_impl,
)
EOF
    cat > "$WORKSPACE/tools/demo/struo.toml" <<'EOF'
[[target]]
name = "Task"
kind = "demo_action"

[target.attrs]
message = "hello"
EOF
  }

  create_split_action_workspace() {
    mkdir -p "$WORKSPACE/modules" "$WORKSPACE/tools/split"
    cat > "$WORKSPACE/struo.toml" <<'EOF'
[modules]
paths = ["modules/*.star"]
EOF
    cat > "$WORKSPACE/modules/split.star" <<'EOF'
def _split_impl(ctx):
    first = declare_output("first.txt")
    second = declare_output("second.txt")
    run_action(
        argv = [host_which("sh"), "-c", "printf \"%s\" \"$1\" > \"$2\"", "sh", ctx["attr"]["message"], first],
        outputs = [first],
        identifier = ctx["label"]["id"] + ":first",
    )
    run_action(
        argv = [host_which("sh"), "-c", "printf second > \"$1\"", "sh", second],
        outputs = [second],
        identifier = ctx["label"]["id"] + ":second",
        depends_on_prior_actions = False,
    )
    return {"first": first, "second": second}

split_action = target_kind(
    docs = "Split action kind",
    attrs = [
        attr("message", "string", required = True, docs = "Message to emit"),
    ],
    providers = ["split_output"],
    capabilities = [capability("build", ["default"])],
    impl = _split_impl,
)
EOF
    cat > "$WORKSPACE/tools/split/struo.toml" <<'EOF'
[[target]]
name = "Task"
kind = "split_action"

[target.attrs]
message = "one"
EOF
  }

  It 'builds custom target kind declared actions through the graph command'
    create_declared_action_workspace

    When call struo --format json build tools/demo/Task
    The status should be success
    The stdout should include '"target":"tools/demo/Task"'
    The stdout should include '"capability":"build"'
    The stdout should include '"status":"completed"'
    The stdout should include '.struo/out/tools/demo/Task/build.txt'
    The path "$WORKSPACE/.struo/out/tools/demo/Task/build.txt" should be file
    The contents of file "$WORKSPACE/.struo/out/tools/demo/Task/build.txt" should equal 'hello:build'
  End

  It 'records evidence for graph build declared actions'
    create_declared_action_workspace
    struo --format json build tools/demo/Task >/dev/null

    When call struo --format json query evidence tools/demo/Task:build
    The status should be success
    The stdout should include '"schema":"struo.evidence.v1"'
    The stdout should include '"subject":{"kind":"target","id":"tools/demo/Task","capability":"build"}'
    The stdout should include '"status":"passed"'
    The stdout should include '"cache":"miss"'
    The stdout should include '"exit_code":0'
    The stdout should include '.struo/out/tools/demo/Task/build.txt'
    The path "$WORKSPACE/.struo/struo.sqlite" should be file
  End

  It 'runs custom target kind declared actions through the graph command'
    create_declared_action_workspace

    When call struo --format json run tools/demo/Task
    The status should be success
    The stdout should include '"target":"tools/demo/Task"'
    The stdout should include '"capability":"run"'
    The stdout should include '"status":"completed"'
    The stdout should include '.struo/out/tools/demo/Task/run.txt'
    The path "$WORKSPACE/.struo/out/tools/demo/Task/run.txt" should be file
    The contents of file "$WORKSPACE/.struo/out/tools/demo/Task/run.txt" should equal 'hello:run'
  End

  It 'tests custom target kind declared actions through the graph command'
    create_declared_action_workspace

    When call struo --format json test tools/demo/Task
    The status should be success
    The stdout should include '"target":"tools/demo/Task"'
    The stdout should include '"capability":"test"'
    The stdout should include '"status":"completed"'
    The stdout should include '.struo/out/tools/demo/Task/test.txt'
    The path "$WORKSPACE/.struo/out/tools/demo/Task/test.txt" should be file
    The contents of file "$WORKSPACE/.struo/out/tools/demo/Task/test.txt" should equal 'hello:test'
  End

  It 'verifies build artifacts independently of declared logs nested in directory outputs'
    create_declared_action_workspace
    cat > "$WORKSPACE/modules/demo.star" <<'EOF'
def _demo_impl(ctx):
    out = declare_output("bundle")
    run_action(
        argv = [host_which("sh"), "-c", "printf stable > \"$1/artifact\"; printf '%s' \"$$\"", "sh", out],
        outputs = [out],
        stdout = out + "/log.txt",
        create_dirs = [out],
    )
    second = declare_output("second.txt")
    run_action(
        argv = [host_which("sh"), "-c", "printf stable > \"$1\"", "sh", second],
        outputs = [second],
    )
    return {}

demo_action = target_kind(
    attrs = [attr("message", "string")],
    capabilities = [capability("build", ["default"])],
    impl = _demo_impl,
)
EOF
    When call struo --format json build --verify-reproducible tools/demo/Task
    The status should be success
    The stdout should include '"artifacts_match":true'
    The stdout should include '"logs_match":false'
    The stdout should include '"log":true'
    The path "$WORKSPACE/.struo/out/tools/demo/Task/bundle" should not be exist
  End

  It 'uses declared logs as the verdict when a build declares no other artifacts'
    create_declared_action_workspace
    cat > "$WORKSPACE/modules/demo.star" <<'EOF'
def _demo_impl(ctx):
    log = declare_output("log.txt")
    run_action(argv = [host_which("sh"), "-c", "printf '%s' \"$$\""], outputs = [log], stdout = log)
    return {}

demo_action = target_kind(
    attrs = [attr("message", "string")],
    capabilities = [capability("build", ["default"])],
    impl = _demo_impl,
)
EOF
    When call struo --format json build --verify-reproducible tools/demo/Task
    The status should be failure
    The stdout should include '"artifacts_match":null'
    The stdout should include '"logs_match":false'
    The stdout should include '"success":false'
  End

  exercise_cached_environment() {
    create_declared_action_workspace
    cat > "$WORKSPACE/modules/demo.star" <<'EOF'
def _demo_impl(ctx):
    out = declare_output("environment.txt")
    run_action(
        argv = [host_which("sh"), "-c", "printf '%s:%s' \"$STRUO_INHERITED_VALUE\" \"$NATIVE_SETTING\" > \"$1\"", "sh", out],
        outputs = [out],
        inherit_parent_env = True,
        sandbox = "off",
        env = {"NATIVE_SETTING": "resolved"},
    )
    return {}

demo_action = target_kind(
    attrs = [attr("message", "string")],
    capabilities = [capability("build", ["default"])],
    impl = _demo_impl,
)
EOF
    for value in first first second second; do
      STRUO_CHANGE_TRACKER=0 STRUO_INHERITED_VALUE="$value" NATIVE_SETTING=caller struo --format json build tools/demo/Task > "$SPEC_ROOT/environment.json" 2> "$SPEC_ROOT/environment.err" || { cat "$SPEC_ROOT/environment.err"; return 1; }
      [ "$(cat "$WORKSPACE/.struo/out/tools/demo/Task/environment.txt")" = "$value:resolved" ] || return 1
      jq -r '.cache' "$SPEC_ROOT/environment.json" >> "$SPEC_ROOT/cache-states"
      rm "$WORKSPACE/.struo/out/tools/demo/Task/environment.txt"
    done
    [ "$(cat "$SPEC_ROOT/cache-states")" = "$(printf 'miss\nhit\nmiss\nhit')" ] || return 1
    [ -d "$WORKSPACE/.struo/analysis" ] || return 1
    if grep -R -E '"STRUO_INHERITED_VALUE"[[:space:]]*:[[:space:]]*"(first|second)"' "$WORKSPACE/.struo/analysis"; then
      return 1
    fi
  }

  It 'caches the effective inherited environment, restores outputs, and honors explicit overrides'
    When call exercise_cached_environment
    The status should be success
    The output should equal ''
  End

  build_uncached_sandboxed_directories() {
    create_declared_action_workspace
    cat > "$WORKSPACE/modules/demo.star" <<'EOF'
def _demo_impl(ctx):
    tree = declare_output("tree")
    run_action(
        argv = [host_which("sh"), "-c", "mkdir \"$1\" && head -c 4194304 /dev/urandom > \"$1/payload\"", "sh", tree],
        outputs = [tree],
        clean_paths = [tree],
        sandbox = "copied-inputs",
        cacheable = False,
        intermediate = True,
    )
    copy = declare_output("copy")
    run_action(
        argv = [host_which("sh"), "-c", "mkdir \"$2\" && cp \"$1/payload\" \"$2/payload\"", "sh", tree, copy],
        inputs = [tree],
        outputs = [copy],
        clean_paths = [copy],
        sandbox = "copied-inputs",
        cacheable = False,
        intermediate = True,
        depends_on_prior_actions = False,
    )
    return {}

demo_action = target_kind(
    attrs = [attr("message", "string")],
    capabilities = [capability("build", ["default"])],
    impl = _demo_impl,
)
EOF
    out="$WORKSPACE/.struo/out/tools/demo/Task"
    struo --format json build tools/demo/Task | jq -r '.status' || return
    cmp "$out/tree/payload" "$out/copy/payload" || return
    du -sk "$XDG_CACHE_HOME/struo/cas" | awk '{ print ($1 < 1024 ? "small cache" : "cache holds " $1 " KiB") }'
  }

  It 'keeps intermediate outputs of uncached sandboxed actions out of the cache'
    When call build_uncached_sandboxed_directories
    The status should be success
    The line 1 of stdout should equal 'completed'
    The line 2 of stdout should equal 'small cache'
  End

  reuse_consumer_of_restorable_directory() {
    create_declared_action_workspace
    cat > "$WORKSPACE/modules/demo.star" <<'EOF'
def _demo_impl(ctx):
    tree = declare_output("tree")
    run_action(
        argv = [host_which("sh"), "-c", "mkdir \"$1\" && printf stable > \"$1/payload\"", "sh", tree],
        outputs = [tree],
        clean_paths = [tree],
        sandbox = "copied-inputs",
        cacheable = False,
        identifier = "producer",
    )
    copy = declare_output("copy.txt")
    run_action(
        argv = [host_which("sh"), "-c", "cp \"$1/payload\" \"$2\"", "sh", tree, copy],
        inputs = [tree],
        outputs = [copy],
        sandbox = "copied-inputs",
        identifier = "consumer",
    )
    return {}

demo_action = target_kind(
    attrs = [attr("message", "string")],
    capabilities = [capability("build", ["default"])],
    impl = _demo_impl,
)
EOF
    for _ in 1 2; do
      struo --format json build tools/demo/Task >/dev/null || return
    done
    latest_log="$(ls -t "$(struo_log_dir)"/*.log | head -1)"
    jq -r 'select(.fields.identifier == "consumer") | .fields.cache' "$latest_log"
  }

  It 'keeps other outputs of uncached sandboxed actions restorable, so cacheable consumers hit'
    When call reuse_consumer_of_restorable_directory
    The status should be success
    The stdout should equal 'hit'
  End

  It 'names a declared output an uncached sandboxed action did not produce'
    create_declared_action_workspace
    cat > "$WORKSPACE/modules/demo.star" <<'EOF'
def _demo_impl(ctx):
    run_action(
        argv = [host_which("sh"), "-c", "true"],
        outputs = [declare_output("missing")],
        sandbox = "copied-inputs",
        cacheable = False,
        intermediate = True,
    )
    return {}

demo_action = target_kind(
    attrs = [attr("message", "string")],
    capabilities = [capability("build", ["default"])],
    impl = _demo_impl,
)
EOF
    When call struo --format json build tools/demo/Task
    The status should be failure
    The stderr should include 'declared output `.struo/out/tools/demo/Task/missing` was not produced'
  End

  It 'keeps later graph actions cached when they skip prior action digests'
    create_split_action_workspace
    struo --format json build tools/split/Task >/dev/null
    sed -i.bak 's/message = "one"/message = "two"/' "$WORKSPACE/tools/split/struo.toml"

    When call struo --format json build tools/split/Task
    The status should be success
    The stdout should include '"target":"tools/split/Task"'
    The stdout should include '"cache":"miss"'
    The contents of file "$WORKSPACE/.struo/out/tools/split/Task/first.txt" should equal 'two'
    The contents of file "$WORKSPACE/.struo/out/tools/split/Task/second.txt" should equal 'second'
    evidence=$("$STRUO_BIN" -C "$WORKSPACE" --format json query evidence tools/split/Task:build)
    The variable evidence should include '"cache":"hit"'
  End
End
