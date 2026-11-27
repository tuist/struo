#shellcheck shell=bash

Describe 'Mix compiler resources'
  BeforeEach 'setup_workspace'
  AfterEach 'cleanup_workspace'

  mix_tools_unavailable() {
    ! command -v mix >/dev/null 2>&1 || ! command -v elixir >/dev/null 2>&1
  }

  prepare_resource_dependency() {
    cp -R "$REPO_ROOT/crates/struo-frontend/prelude/examples/elixir-library-with-mix-dependency/." "$WORKSPACE/"
    mkdir -p "$WORKSPACE/deps/locked_greeting/priv"
    printf 'static' > "$WORKSPACE/deps/locked_greeting/priv/static.txt"
    cat > "$WORKSPACE/deps/locked_greeting/mix.exs" <<'EOF'
defmodule Mix.Tasks.Compile.Resource do
  use Mix.Task.Compiler

  def run(_args) do
    path = Path.join(Mix.Project.app_path(), "priv/native/generated.txt")
    File.mkdir_p!(Path.dirname(path))
    File.write!(path, "generated")
    manifest = Path.join(Mix.Project.app_path(), ".mix/volatile")
    File.mkdir_p!(Path.dirname(manifest))
    File.write!(manifest, Integer.to_string(System.system_time(:nanosecond)))
    {:ok, []}
  end
end

defmodule LockedGreeting.MixProject do
  use Mix.Project

  def project do
    [
      app: :locked_greeting,
      version: "0.1.0",
      elixir: "~> 1.14",
      compilers: [:resource] ++ Mix.compilers()
    ]
  end

  def application, do: []
end
EOF
  }

  build_and_restore_resources() {
    struo --format json build greeting > "$WORKSPACE/build.json" || return
    app_dir="$WORKSPACE/.struo/out/mix-locked-greeting/mix/prod/lib/locked_greeting"
    [ "$(cat "$app_dir/priv/native/generated.txt")" = generated ] || return
    if [ "${1:-}" != generated-only ]; then
      [ "$(cat "$app_dir/priv/static.txt")" = static ] || return
    fi
    rm -rf "$WORKSPACE/.struo/out"
    struo --format json build greeting > "$WORKSPACE/restored.json" || return
    [ "$(cat "$app_dir/priv/native/generated.txt")" = generated ] || return
    if [ "${1:-}" != generated-only ]; then
      [ "$(cat "$app_dir/priv/static.txt")" = static ] || return
    fi
    grep -q '"cache":"hit"' "$WORKSPACE/restored.json"
  }

  compare_independent_dependency_builds() {
    XDG_CACHE_HOME="$SPEC_ROOT/first-cache" struo --format json build greeting > "$SPEC_ROOT/first.json" || return
    manifest="$WORKSPACE/.struo/out/mix-locked-greeting/mix/prod/lib/locked_greeting/.mix/volatile"
    cp "$manifest" "$SPEC_ROOT/first-manifest" || return
    rm -rf "$WORKSPACE/.struo"
    XDG_CACHE_HOME="$SPEC_ROOT/second-cache" struo --format json build greeting > "$SPEC_ROOT/second.json" || return
    python3 - "$SPEC_ROOT/first.json" "$SPEC_ROOT/second.json" "$SPEC_ROOT/first-manifest" "$manifest" <<'PY'
import json, pathlib, sys
first, second = [json.load(open(path)) for path in sys.argv[1:3]]
assert first["action_digest"] == second["action_digest"], (first, second)
assert pathlib.Path(sys.argv[3]).read_bytes() != pathlib.Path(sys.argv[4]).read_bytes()
PY
    [ "$?" = 0 ] || return
    cat > "$WORKSPACE/deps/locked_greeting/lib/locked_greeting.ex" <<'EX'
defmodule LockedGreeting do
  def message(name), do: "Changed, #{name}!"
end
EX
    XDG_CACHE_HOME="$SPEC_ROOT/second-cache" struo --format json build greeting > "$SPEC_ROOT/changed.json" || return
    python3 - "$SPEC_ROOT/second.json" "$SPEC_ROOT/changed.json" <<'PY'
import json, sys
previous, changed = [json.load(open(path)) for path in sys.argv[1:]]
assert previous["action_digest"] != changed["action_digest"], (previous, changed)
PY
  }

  prepare_build_root_resources() {
    mkdir -p "$WORKSPACE/lib" "$WORKSPACE/helper/lib"
    cat > "$WORKSPACE/mix.exs" <<'EX'
defmodule Mix.Tasks.Compile.Resources do
  use Mix.Task.Compiler
  def run(_args) do
    destination = Path.join(Mix.Project.build_path(), "generated-assets/project/entry.js")
    File.mkdir_p!(Path.dirname(destination))
    File.write!(destination, "generated")
    counter = case File.read("compile-count") do
      {:ok, contents} -> String.to_integer(contents)
      _ -> 0
    end
    File.write!("compile-count", Integer.to_string(counter + 1))
    {:ok, []}
  end
end

defmodule Mix.Tasks.Assets.Validate do
  use Mix.Task
  def run(_args) do
    root = Mix.Project.build_path()
    unless File.read!(Path.join(root, "generated-assets/project/entry.js")) == "generated", do: raise("missing compiler resource")
    unless File.exists?(Path.join(root, "lib/helper/ebin/Elixir.Helper.beam")), do: raise("missing staged dependency")
  end
end

defmodule ResourceProject.MixProject do
  use Mix.Project
  def project, do: [app: :resource_project, version: "0.1.0", compilers: [:resources] ++ Mix.compilers(), deps: [{:helper, path: "helper"}]]
  def application, do: [extra_applications: [:logger]]
end
EX
    printf 'defmodule ResourceProject do\n  def value, do: Helper.value()\nend\n' > "$WORKSPACE/lib/resource_project.ex"
    printf 'defmodule Helper do\n  def value, do: :ok\nend\n' > "$WORKSPACE/helper/lib/helper.ex"
    cat > "$WORKSPACE/helper/mix.exs" <<'EX'
defmodule Helper.MixProject do
  use Mix.Project
  def project, do: [app: :helper, version: "0.1.0"]
end
EX
    printf '%%{}\n' > "$WORKSPACE/mix.lock"
    cat > "$WORKSPACE/struo.toml" <<'TOML'
[[target]]
name = "dependencies"
kind = "mix_dependencies"
srcs = ["mix.exs", "mix.lock"]
[target.attrs]
mix_env = "prod"

[[target]]
name = "application"
kind = "mix_project"
srcs = ["lib/**/*.ex", "mix.exs"]
deps = ["dependencies"]
[target.attrs]
app_name = "resource_project"
mix_env = "prod"

[[target]]
name = "release"
kind = "mix_release"
deps = ["application"]
[target.attrs]
pre_tasks = [["assets.validate"]]
cacheable = false
TOML
  }

  release_and_restore_build_root_resources() {
    struo build release --format json > "$SPEC_ROOT/release-first.json" || return
    rm -rf "$WORKSPACE/.struo/out" "$WORKSPACE/.struo/tmp"
    struo build release --format json > "$SPEC_ROOT/release-restored.json" || return
    [ "$(cat "$WORKSPACE/compile-count")" = 1 ]
  }

  It 'stages compiler-generated build-root resources and dependencies for uncached release tasks'
    Skip if 'Mix toolchain unavailable on this host' mix_tools_unavailable
    prepare_build_root_resources
    When call release_and_restore_build_root_resources
    The status should be success
  End

  It 'preserves generated and static priv resources across cached builds'
    Skip if 'Mix toolchain unavailable on this host' mix_tools_unavailable
    prepare_resource_dependency

    When call build_and_restore_resources
    The status should be success
  End

  It 'keeps consumer keys stable across independent builds with volatile native manifests'
    Skip if 'Mix toolchain unavailable on this host' mix_tools_unavailable
    prepare_resource_dependency

    When call compare_independent_dependency_builds
    The status should be success
  End

  rebar_unavailable() {
    [ ! -x "${MIX_REBAR3:-}" ]
  }

  prepare_rebar_hook_dependency() {
    MIX_REBAR3="$(HOME="$SPEC_ORIGINAL_HOME" elixir -e 'Mix.start(); IO.write(Mix.Rebar.local_rebar_path(:rebar3))' 2>/dev/null)"
    export MIX_REBAR3
    cp -R "$REPO_ROOT/crates/struo-frontend/prelude/examples/elixir-library-with-mix-dependency/." "$WORKSPACE/"
    package="$WORKSPACE/deps/locked_greeting"
    rm -rf "$package/mix.exs" "$package/lib"
    mkdir -p "$package/src"
    if [ "${1:-}" != generated-only ]; then
      mkdir -p "$package/priv"
      printf 'static' > "$package/priv/static.txt"
    fi
    # The native build reads the host outside the package, like a NIF
    # Makefile linking a system library.
    printf generated > "$SPEC_ROOT/native.txt"
    # Like a NIF Makefile, the hook writes into the output application.
    # Rebar runs a hook without a shell, so it runs one script.
    cat > "$package/generate.sh" <<EOF
mkdir -p "\$REBAR_BARE_COMPILER_OUTPUT_DIR/priv/native"
cat "$SPEC_ROOT/native.txt" > "\$REBAR_BARE_COMPILER_OUTPUT_DIR/priv/native/generated.txt"
EOF
    printf '{pre_hooks, [{compile, "sh generate.sh"}]}.\n' > "$package/rebar.config"
    printf '{application, locked_greeting, [{vsn, "0.1.0"}, {applications, [kernel, stdlib]}, {modules, []}, {registered, []}]}.\n' > "$package/src/locked_greeting.app.src"
    printf -- '-module(locked_greeting).\n-export([message/1]).\nmessage(Name) -> Name.\n' > "$package/src/locked_greeting.erl"
    "$SPEC_PYTHON3" - "$WORKSPACE" <<'PY'
import json, pathlib, sys
root = pathlib.Path(sys.argv[1])
lock = root / "mix.lock"
lock.write_text(lock.read_text().replace("[:mix]", "[:rebar3]"))
graph = json.loads((root / "mix-dependencies.json").read_text())
graph["lockfile"] = graph["struo_inputs"]["mix.lock"] = lock.read_text()
graph["lock"]["locked_greeting"][4] = ["rebar3"]
for dependency in graph["dependencies"]:
    if dependency["app"] == "locked_greeting":
        dependency["manager"] = "rebar3"
(root / "mix-dependencies.json").write_text(json.dumps(graph))
PY
  }

  build_and_restore_rebar_hook_resources() {
    struo --format json build mix-locked-greeting > "$WORKSPACE/build.json" || return
    app_dir="$WORKSPACE/.struo/out/mix-locked-greeting/mix/prod/lib/locked_greeting"
    [ "$(cat "$app_dir/priv/native/generated.txt")" = generated ] || return
    [ "$(cat "$app_dir/priv/static.txt")" = static ] || return
    rm -rf "$WORKSPACE/.struo/out"
    struo --format json build mix-locked-greeting > "$WORKSPACE/restored.json" || return
    [ "$(cat "$app_dir/priv/native/generated.txt")" = generated ] || return
    [ "$(cat "$app_dir/priv/static.txt")" = static ] || return
    "$SPEC_PYTHON3" -c 'import json, sys; assert json.load(open(sys.argv[1]))["cache"] == "hit"' "$WORKSPACE/restored.json"
  }

  consumer_compile_digest() {
    rm -rf "$WORKSPACE/.struo" "$XDG_CACHE_HOME" "$(struo_log_dir)"
    struo --format json build greeting > "$WORKSPACE/greeting.json" || return
    "$SPEC_PYTHON3" - "$(struo_log_dir)" <<'PY'
import json, pathlib, sys
for log in pathlib.Path(sys.argv[1]).glob("*.log"):
    for line in log.read_text().splitlines():
        fields = json.loads(line).get("fields", {})
        if fields.get("identifier") == "greeting:elixir-compile" and fields.get("action_digest"):
            print(fields["action_digest"])
PY
  }

  rebuild_consumer_after_native_change() {
    before="$(consumer_compile_digest)" || return
    # A fresh build of the same package sources produces a different native
    # library, which the consumer compiles against.
    printf rebuilt > "$SPEC_ROOT/native.txt"
    after="$(consumer_compile_digest)" || return
    [ -n "$before" ] || return
    [ "$(cat "$WORKSPACE/.struo/out/mix-locked-greeting/mix/prod/lib/locked_greeting/priv/native/generated.txt")" = rebuilt ] || return
    [ "$after" != "$before" ] || { echo "a changed hook-built priv file left the consumer key unchanged" >&2; return 1; }
  }

  It 'rebuilds consumers when a Rebar compile hook writes different priv files'
    Skip if 'Mix toolchain unavailable on this host' mix_tools_unavailable
    prepare_rebar_hook_dependency generated-only
    Skip if 'Rebar 3 unavailable on this host' rebar_unavailable

    When call rebuild_consumer_after_native_change
    The status should be success
  End

  It 'keeps priv files a Rebar compile hook writes into the output application'
    Skip if 'Mix toolchain unavailable on this host' mix_tools_unavailable
    prepare_rebar_hook_dependency
    Skip if 'Rebar 3 unavailable on this host' rebar_unavailable

    When call build_and_restore_rebar_hook_resources
    The status should be success
  End

  It 'captures generated priv resources without static source resources'
    Skip if 'Mix toolchain unavailable on this host' mix_tools_unavailable
    prepare_resource_dependency
    rm -rf "$WORKSPACE/deps/locked_greeting/priv"

    When call build_and_restore_resources generated-only
    The status should be success
  End
End
