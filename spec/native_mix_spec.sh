#shellcheck shell=bash

Describe 'configuration-free native Mix commands'
  BeforeEach 'setup_workspace'
  AfterEach 'cleanup_workspace'

  mix_tools_unavailable() {
    ! command -v mix >/dev/null 2>&1 || ! command -v elixir >/dev/null 2>&1
  }

  prepare_native_workspace() {
    mkdir -p "$WORKSPACE/.git" "$WORKSPACE/app/lib" "$WORKSPACE/app/test/support" "$WORKSPACE/helper/lib" "$WORKSPACE/app/native"
    printf 'invalid Swift package\n' > "$WORKSPACE/app/native/Package.swift"
    printf 'invalid Swift package\n' > "$WORKSPACE/Package.swift"
    cat > "$WORKSPACE/helper/mix.exs" <<'EX'
defmodule Helper.MixProject do
  use Mix.Project
  def project, do: [app: :helper, version: "0.1.0"]
end
EX
    cat > "$WORKSPACE/helper/lib/helper.ex" <<'EX'
defmodule Helper do
  def value, do: :ok
end
EX
    cat > "$WORKSPACE/app/mix.exs" <<'EX'
defmodule Mix.Tasks.Compile.CheckOnce do
  use Mix.Task.Compiler
  def run(_) do
    marker = Path.join(Mix.Project.app_path(), "priv/compiled")
    if File.exists?(marker), do: raise("project compiler ran twice")
    File.mkdir_p!(Path.dirname(marker))
    File.write!(marker, "compiled")
    {:ok, []}
  end
end

defmodule Mix.Tasks.CheckSetup do
  use Mix.Task
  def run(_) do
    Mix.Task.run("compile")
    unless TestSupport.value() == :test, do: raise("test support was not compiled")
    File.write!("setup-runs.txt", "setup\n", [:append])
  end
end

defmodule App.MixProject do
  use Mix.Project
  def project do
    [app: :app, version: "0.1.0", deps: [{:helper, path: "../helper"}],
     compilers: [:check_once] ++ Mix.compilers(),
     elixirc_paths: if(Mix.env() == :test, do: ["lib", "test/support"], else: ["lib"]),
     aliases: [test: ["check_setup", "test"]]]
  end
end
EX
    printf '1.0.0' > "$WORKSPACE/app/VERSION"
    cat > "$WORKSPACE/app/lib/app.ex" <<'EX'
defmodule App do
  @external_resource "VERSION"
  @version File.read!("VERSION")
  def version, do: @version
  def source_root, do: Path.expand("..", __DIR__)
  def value, do: Helper.value()
end
EX
    cat > "$WORKSPACE/app/lib/dynamic_content.ex" <<'EX'
defmodule DynamicContent do
  def __mix_recompile__?, do: false
end
EX
    cat > "$WORKSPACE/app/test/support/test_support.ex" <<'EX'
defmodule TestSupport do
  def value, do: :test
end
EX
    printf 'ExUnit.start()\n' > "$WORKSPACE/app/test/test_helper.exs"
    cat > "$WORKSPACE/app/test/app_test.exs" <<'EX'
defmodule AppTest do
  use ExUnit.Case, async: true
  test "compiled application and environment-specific support" do
    assert function_exported?(DynamicContent, :__mix_recompile__?, 0)
    assert App.value() == :ok
    assert App.version() == File.read!("VERSION")
    assert App.source_root() == File.cwd!()
    assert TestSupport.value() == :test
  end
  test "native test environment and host tools" do
    assert System.get_env("STRUO_NATIVE_TEST_VALUE") == "present"
    assert System.find_executable("native-test-helper")
  end
end
EX
    mkdir -p "$SPEC_ROOT/host-tools"
    printf '#!/bin/sh\nexit 0\n' > "$SPEC_ROOT/host-tools/native-test-helper"
    chmod +x "$SPEC_ROOT/host-tools/native-test-helper"
  }

  native_struo() {
    (cd "$WORKSPACE/app" && PATH="$SPEC_ROOT/host-tools:$PATH" STRUO_NATIVE_TEST_VALUE=present "$STRUO_BIN" --format json "$@")
  }

  exercise_dependency_free_tests() {
    mkdir -p "$WORKSPACE/app"
    cp -R "$REPO_ROOT/crates/struo-frontend/prelude/examples/mix-workspace-native-project/." "$WORKSPACE/app/"
    rm "$WORKSPACE/app/struo.toml"
    native_struo test > "$SPEC_ROOT/test.json" 2> "$SPEC_ROOT/test.err" || { cat "$SPEC_ROOT/test.err"; return 1; }
    python3 - "$WORKSPACE/app/.struo/out/mix_tests/test/test_results.json" <<'PY'
import json, sys
result = json.load(open(sys.argv[1]))
assert result["status"] == "passed", result
assert result["summary"]["total"] == 1, result
assert result["summary"]["passed"] == 1, result
PY
  }

  exercise_native_commands() {
    native_struo build > "$SPEC_ROOT/build.json" 2> "$SPEC_ROOT/build.err" || { cat "$SPEC_ROOT/build.err"; return 1; }
    rm -rf "$WORKSPACE/.struo/out"
    native_struo build > "$SPEC_ROOT/restored.json" 2> "$SPEC_ROOT/restored.err" || { cat "$SPEC_ROOT/restored.err"; return 1; }
    grep -q '"cache":"hit"' "$SPEC_ROOT/restored.json" || return
    test -f "$WORKSPACE/.struo/out/app/mix_application_dev/mix/dev/lib/app/priv/compiled" || return
    MIX_ENV=test native_struo build > "$SPEC_ROOT/test-build.json" 2> "$SPEC_ROOT/test-build.err" || { cat "$SPEC_ROOT/test-build.err"; return 1; }
    test -f "$WORKSPACE/.struo/out/app/mix_application_test/mix/test/lib/app/ebin/Elixir.TestSupport.beam" || return
    rm -rf "$WORKSPACE/.struo/tmp"
    native_struo test > "$SPEC_ROOT/test.json" 2> "$SPEC_ROOT/test.err" || { cat "$SPEC_ROOT/test.json" "$SPEC_ROOT/test.err" "$WORKSPACE/.struo/out/app/mix_tests/test/elixir-test.log"; return 1; }
    rm -rf "$WORKSPACE/.struo/tmp"
    native_struo test > "$SPEC_ROOT/retest.json" 2> "$SPEC_ROOT/retest.err" || { cat "$SPEC_ROOT/retest.json" "$SPEC_ROOT/retest.err" "$WORKSPACE/.struo/out/app/mix_tests/test/elixir-test.log"; return 1; }
    [ "$(wc -l < "$WORKSPACE/app/setup-runs.txt" | tr -d ' ')" = 2 ] || return
    test ! -f "$WORKSPACE/struo.toml" || return
    test ! -f "$WORKSPACE/app/struo.toml" || return
    python3 - "$WORKSPACE/.struo/out/app/mix_tests/test/test_results.json" <<'PY'
import json, sys
result = json.load(open(sys.argv[1]))
assert result["status"] == "passed", result
assert result["summary"]["total"] == 2, result
assert result["summary"]["passed"] == 2, result
assert result["summary"]["failed"] == 0, result
assert result["artifacts"]["logs"] == [".struo/out/app/mix_tests/test/elixir-test.log"], result
PY
  }

  exercise_failing_test() {
    exercise_native_commands || return
    cat > "$WORKSPACE/app/test/failing_test.exs" <<'EX'
defmodule FailingTest do
  use ExUnit.Case, async: true
  test "a real test failure" do
    assert false
  end
end
EX
    native_struo test > "$SPEC_ROOT/failed.json" 2> "$SPEC_ROOT/failed.err"
    [ "$?" = 1 ] || return 1
    python3 - "$WORKSPACE/.struo/out/app/mix_tests/test/test_results.json" <<'PY'
import json, sys
result = json.load(open(sys.argv[1]))
assert result["status"] == "failed", result
assert result["summary"]["total"] == 3, result
assert result["summary"]["failed"] == 1, result
assert result["summary"]["passed"] == 2, result
PY
  }

  exercise_resource_invalidation() {
    exercise_native_commands || return
    printf '2.0.0' > "$WORKSPACE/app/VERSION"
    native_struo test > "$SPEC_ROOT/changed-resource.json" 2> "$SPEC_ROOT/changed-resource.err" || { cat "$SPEC_ROOT/changed-resource.err" "$WORKSPACE/.struo/out/app/mix_tests/test/elixir-test.log"; return 1; }
    native_struo build > "$SPEC_ROOT/changed-build.json" 2> "$SPEC_ROOT/changed-build.err" || return
    native_struo build > "$SPEC_ROOT/unchanged-build.json" 2> "$SPEC_ROOT/unchanged-build.err" || return
    grep -q '"cache":"hit"' "$SPEC_ROOT/unchanged-build.json"
  }

  exercise_dependency_grammars() {
    mkdir -p "$WORKSPACE/helper/src"
    printf 'Nonterminals value.\nTerminals int.\nRootsymbol value.\nvalue -> int : %s.\n' "'\$1'" > "$WORKSPACE/helper/src/helper_parser.yrl"
    printf 'Definitions.\nD = [0-9]\nRules.\n{D}+ : {token, {int, TokenLine, list_to_integer(TokenChars)}}.\nErlang code.\n' > "$WORKSPACE/helper/src/helper_lexer.xrl"
    native_struo build mix_application_dev > "$SPEC_ROOT/grammar-cold.json" 2> "$SPEC_ROOT/grammar-cold.err" || { cat "$SPEC_ROOT/grammar-cold.err"; return 1; }
    # Mix wrote the generated parser into the dependency's own sources.
    test -f "$WORKSPACE/helper/src/helper_parser.erl" || return
    rm -rf "$WORKSPACE/.struo"
    native_struo build mix_application_dev > "$SPEC_ROOT/grammar-warm.json" 2> "$SPEC_ROOT/grammar-warm.err" || { cat "$SPEC_ROOT/grammar-warm.err"; return 1; }
    grep -q '"cache":"hit"' "$SPEC_ROOT/grammar-warm.json"
  }

  exercise_structured_counts() {
    cat > "$WORKSPACE/app/test/noise_test.exs" <<'EX'
defmodule NoiseTest do
  use ExUnit.Case
  test "summary-looking output is not a result" do
    IO.puts("7 tests, 3 failures")
    IO.puts("Result: 99 passed, 5 skipped")
  end
  @tag :skip
  test "skipped", do: :ok
end
EX
    native_struo test > "$SPEC_ROOT/noise.json" 2> "$SPEC_ROOT/noise.err" || { cat "$SPEC_ROOT/noise.err" "$WORKSPACE/.struo/out/app/mix_tests/test/elixir-test.log"; return 1; }
    python3 - "$WORKSPACE/.struo/out/app/mix_tests/test/test_results.json" <<'PY'
import json, sys
result = json.load(open(sys.argv[1]))
assert result["status"] == "passed", result
assert result["summary"] == {"total": 4, "passed": 3, "failed": 0, "skipped": 1, "flaky": 0}, result
PY
    [ "$?" = 0 ] || return
    cat > "$WORKSPACE/app/test/invalid_test.exs" <<'EX'
defmodule InvalidTest do
  use ExUnit.Case
  setup_all do
    raise "failed setup invalidates the test"
  end
  test "invalid", do: :ok
end
EX
    native_struo test > "$SPEC_ROOT/invalid.json" 2> "$SPEC_ROOT/invalid.err"
    [ "$?" = 1 ] || return 1
    python3 - "$WORKSPACE/.struo/out/app/mix_tests/test/test_results.json" <<'PY'
import json, sys
result = json.load(open(sys.argv[1]))
assert result["status"] == "failed", result
assert result["summary"] == {"total": 5, "passed": 3, "failed": 1, "skipped": 1, "flaky": 0}, result
PY
  }

  exercise_source_location() {
    native_struo build > "$SPEC_ROOT/location-first.json" 2> "$SPEC_ROOT/location-first.err" || return
    destination="$SPEC_ROOT/other-workspace"
    mkdir -p "$destination/.git"
    cp -R "$WORKSPACE/app" "$WORKSPACE/helper" "$destination/"
    WORKSPACE="$destination"
    native_struo build > "$SPEC_ROOT/location-second.json" 2> "$SPEC_ROOT/location-second.err" || return
    (cd "$WORKSPACE/app" && elixir -pa "$WORKSPACE/.struo/out/app/mix_application_dev/mix/dev/lib/app/ebin" -e 'unless App.source_root() == File.cwd!(), do: raise("cached bytecode retained a different source directory")') || return
    # Path dependencies record their absolute source paths in bytecode too.
    (cd "$WORKSPACE/helper" && elixir -pa "$WORKSPACE/.struo/out/app/mix-dev-helper/mix/prod/lib/helper/ebin" -e 'unless Path.dirname(to_string(Helper.module_info(:compile)[:source])) == Path.join(File.cwd!(), "lib"), do: raise("cached path dependency bytecode retained a different source directory")')
  }

  prepare_staged_tests() {
    prepare_native_workspace
    mkdir -p "$WORKSPACE/app/_build/test/lib/app" "$WORKSPACE/app/test/fixtures"
    printf 'developer output' > "$WORKSPACE/app/_build/test/lib/app/developer-sentinel"
    printf 'not declared' > "$WORKSPACE/app/undeclared-only"
    printf 'first' > "$WORKSPACE/app/test/fixtures/value"
    printf '[workspace]\ninclude = ["app/struo.toml"]\n' > "$WORKSPACE/struo.toml"
    cat >> "$WORKSPACE/app/mix.exs" <<'EX'
defmodule Mix.Tasks.PrivateSetup do
  use Mix.Task
  def run([argument]), do: File.write!("private-setup-marker", argument)
end
EX
    cat > "$WORKSPACE/app/struo.toml" <<'TOML'
[[target]]
name = "dependencies"
kind = "mix_dependencies"
srcs = ["mix.exs"]
[target.attrs]
mix_env = "test"
_mix_path_only = true

[[target]]
name = "application"
kind = "mix_project"
srcs = ["lib/**/*.ex", "test/support/**/*.ex"]
deps = ["./dependencies"]
[target.attrs]
app_name = "app"
mix_env = "test"
data = ["VERSION"]

[[target]]
name = "staged_tests"
kind = "elixir_test"
deps = ["./application"]
srcs = ["test/staged_test.exs"]
[target.attrs]
staged_workspace = true
cacheable = false
data = ["test/fixtures/value"]
setup_tasks = [["private_setup", "private argument with spaces"]]
TOML
    cat > "$WORKSPACE/app/test/staged_test.exs" <<'EX'
defmodule StagedTest do
  use ExUnit.Case
  test "private native layout, declared resources and fresh trees" do
    app = Path.expand("_build/test/lib/app")
    assert Application.app_dir(:app) == app
    assert Helper.value() == :ok
    assert TestSupport.value() == :test
    assert File.read!(Path.join(app, "priv/compiled")) == "compiled"
    assert File.regular?(Path.join(app, ".mix/compile.elixir"))
    assert App.source_root() != File.cwd!()
    assert File.read!("private-setup-marker") == "private argument with spaces"
    refute File.exists?("undeclared-only")
    refute File.exists?(Path.join(app, "developer-sentinel"))
    refute File.exists?(Path.join(app, "previous-test-write"))
    refute File.exists?("previous-cwd-write")
    home = System.user_home!()
    refute File.exists?(Path.join(home, "previous-home-write"))
    assert System.get_env("STRUO_NATIVE_TEST_VALUE") == "present"
    assert System.find_executable("native-test-helper")
    assert File.read!("../helper/mix.exs") =~ "Helper.MixProject"
    File.write!(Path.join(app, "previous-test-write"), "private")
    File.write!("previous-cwd-write", "private")
    File.write!(Path.join(home, "previous-home-write"), "private")
    IO.puts("fixture=" <> File.read!("test/fixtures/value"))
  end
end
EX
  }

  exercise_staged_tests() {
    native_struo -C "$WORKSPACE" test app/staged_tests > "$SPEC_ROOT/staged-first.json" 2> "$SPEC_ROOT/staged-first.err" || { cat "$SPEC_ROOT/staged-first.err"; return 1; }
    grep -q 'fixture=first' "$WORKSPACE/.struo/out/app/staged_tests/test/elixir-test.log" || return
    native_struo -C "$WORKSPACE" test app/staged_tests > "$SPEC_ROOT/staged-repeat.json" 2> "$SPEC_ROOT/staged-repeat.err" || { cat "$SPEC_ROOT/staged-repeat.err"; return 1; }
    printf 'second' > "$WORKSPACE/app/test/fixtures/value"
    native_struo -C "$WORKSPACE" test app/staged_tests > "$SPEC_ROOT/staged-changed.json" 2> "$SPEC_ROOT/staged-changed.err" || return
    grep -q 'fixture=second' "$WORKSPACE/.struo/out/app/staged_tests/test/elixir-test.log" || return
    cat >> "$WORKSPACE/app/test/staged_test.exs" <<'EX'
defmodule StagedFailureTest do
  use ExUnit.Case
  test "intentional staged failure", do: flunk("intentional staged failure")
end
EX
    native_struo -C "$WORKSPACE" test app/staged_tests > "$SPEC_ROOT/staged-failed.json" 2> "$SPEC_ROOT/staged-failed.err"
    [ "$?" = 2 ] || return 1
    grep -q 'intentional staged failure' "$WORKSPACE/.struo/out/app/staged_tests/test/elixir-test.log" || return
    python3 - "$WORKSPACE/.struo/out/app/staged_tests/test/test_results.json" <<'PY'
import json, sys
result = json.load(open(sys.argv[1]))
assert result["status"] == "failed", result
assert result["summary"]["total"] == 2, result
assert result["summary"]["failed"] == 1, result
assert result["summary"]["passed"] == 1, result
PY
    [ "$?" = 0 ] || return
    [ "$(cat "$WORKSPACE/app/_build/test/lib/app/developer-sentinel")" = 'developer output' ] || return
    [ ! -e "$WORKSPACE/app/previous-cwd-write" ] || return
    [ ! -e "$WORKSPACE/app/private-setup-marker" ] || return
    [ ! -e "$WORKSPACE/.struo/out/app/application/mix/test/lib/app/previous-test-write" ] || return
    [ ! -e "$HOME/previous-home-write" ] || return
    ln -s "$WORKSPACE/app/VERSION" "$WORKSPACE/app/test/fixtures/value-link"
    printf '\n' > "$WORKSPACE/app/test/staged_test.exs"
    sed 's|data = \["test/fixtures/value"\]|data = ["test/fixtures/*"]|' "$WORKSPACE/app/struo.toml" > "$SPEC_ROOT/staged.toml"
    mv "$SPEC_ROOT/staged.toml" "$WORKSPACE/app/struo.toml"
    native_struo -C "$WORKSPACE" test app/staged_tests > "$SPEC_ROOT/staged-link.json" 2> "$SPEC_ROOT/staged-link.err"
    [ "$?" != 0 ] || return 1
    grep -q 'staged_workspace does not support symbolic links' "$SPEC_ROOT/staged-link.err"
  }

  It 'stages declared Mix test inputs in a fresh private native layout without developer writes'
    Skip if 'Mix toolchain unavailable on this host' mix_tools_unavailable
    prepare_staged_tests
    When call exercise_staged_tests
    The status should be success
  End

  It 'partitions native bytecode containing source locations across checkouts'
    Skip if 'Mix toolchain unavailable on this host' mix_tools_unavailable
    prepare_native_workspace
    When call exercise_source_location
    The status should be success
  End

  It 'selects tests without shared development and test dependencies'
    Skip if 'Mix toolchain unavailable on this host' mix_tools_unavailable
    When call exercise_dependency_free_tests
    The status should be success
  End

  It 'invalidates compilation when a project-owned external resource changes'
    Skip if 'Mix toolchain unavailable on this host' mix_tools_unavailable
    prepare_native_workspace
    When call exercise_resource_invalidation
    The status should be success
  End

  It 'keeps sources Mix generates from dependency grammars out of the cache key'
    Skip if 'Mix toolchain unavailable on this host' mix_tools_unavailable
    prepare_native_workspace
    When call exercise_dependency_grammars
    The status should be success
  End

  It 'counts native results from ExUnit events, including skipped and invalid tests'
    Skip if 'Mix toolchain unavailable on this host' mix_tools_unavailable
    prepare_native_workspace
    When call exercise_structured_counts
    The status should be success
  End

  It 'builds and tests the current package with sibling dependencies, aliases, and native environment'
    Skip if 'Mix toolchain unavailable on this host' mix_tools_unavailable
    prepare_native_workspace
    When call exercise_native_commands
    The status should be success
  End

  exercise_environment_specific_dependency_graphs() {
    mkdir -p "$WORKSPACE/.git" "$WORKSPACE/app/lib" "$WORKSPACE/first/lib" "$WORKSPACE/second/lib"
    cat > "$WORKSPACE/app/mix.exs" <<'EX'
defmodule App.MixProject do
  use Mix.Project
  @env Mix.env()
  def project, do: [app: :app, version: "0.1.0", deps: deps()]
  defp deps do
    first = [{:first, path: "../first", env: if(@env == :test, do: :test, else: :prod)}]
    if @env == :test, do: first ++ [{:second, path: "../second"}], else: first
  end
end
EX
    cat > "$WORKSPACE/first/mix.exs" <<'EX'
defmodule First.MixProject do
  use Mix.Project
  @env Mix.env()
  def project, do: [app: :first, version: "0.1.0", deps: if(@env == :test, do: [{:second, path: "../second"}], else: [])]
end
EX
    cat > "$WORKSPACE/second/mix.exs" <<'EX'
defmodule Second.MixProject do
  use Mix.Project
  def project, do: [app: :second, version: "0.1.0"]
end
EX
    native_struo -C "$WORKSPACE" query 'MATCH (t:Target)-[:DEPENDS_ON]->(d:Target) WHERE t.id CONTAINS "/mix-" RETURN t.id, d.id' > "$SPEC_ROOT/edges.json" 2> "$SPEC_ROOT/edges.err" || { cat "$SPEC_ROOT/edges.err"; return 1; }
    grep -q '"rows":\[\["app/mix-test-first","app/mix-test-second"\]\]' "$SPEC_ROOT/edges.json" || { cat "$SPEC_ROOT/edges.json"; return 1; }
    native_struo -C "$WORKSPACE" query targets > "$SPEC_ROOT/targets.json" 2>> "$SPEC_ROOT/edges.err" || { cat "$SPEC_ROOT/edges.err"; return 1; }
    grep -q '"id":"app/mix-dev-first"' "$SPEC_ROOT/targets.json" || { cat "$SPEC_ROOT/targets.json"; return 1; }
    grep -q '"id":"app/mix-test-second"' "$SPEC_ROOT/targets.json" || { cat "$SPEC_ROOT/targets.json"; return 1; }
    ! grep -q '"id":"app/mix-dev-second"' "$SPEC_ROOT/targets.json" || { cat "$SPEC_ROOT/targets.json"; return 1; }
  }

  application_digest() {
    native_struo build mix_application_dev > "$SPEC_ROOT/digest.json" 2> "$SPEC_ROOT/digest.err" || { cat "$SPEC_ROOT/digest.err"; return 1; }
    "$SPEC_PYTHON3" -c 'import json, sys; print(json.load(open(sys.argv[1]))["action_digest"])' "$SPEC_ROOT/digest.json"
  }

  exercise_untracked_project_files() {
    mkdir -p "$WORKSPACE/.git" "$WORKSPACE/app"
    cp -R "$REPO_ROOT/crates/struo-frontend/prelude/examples/mix-workspace-native-project/." "$WORKSPACE/app/"
    rm "$WORKSPACE/app/struo.toml"
    printf '1.0.0' > "$WORKSPACE/app/VERSION"
    before="$(application_digest)" || return
    # ExUnit's tmp_dir tag writes under tmp/, and hidden trees such as CI
    # configuration are not Mix inputs.
    mkdir -p "$WORKSPACE/app/tmp/ExampleTest" "$WORKSPACE/app/.github/workflows"
    printf 'scratch' > "$WORKSPACE/app/tmp/ExampleTest/output"
    printf '[]\n' > "$WORKSPACE/app/tmp/ExampleTest/.formatter.exs"
    printf 'on: push\n' > "$WORKSPACE/app/.github/workflows/ci.yml"
    [ "$(application_digest)" = "$before" ] || { echo "tmp/ or .github/ changed the compile key" >&2; return 1; }
    printf '2.0.0' > "$WORKSPACE/app/VERSION"
    versioned="$(application_digest)" || return
    [ "$versioned" != "$before" ] || { echo "a project file did not change the compile key" >&2; return 1; }
    printf '[inputs: ["mix.exs"]]\n' > "$WORKSPACE/app/.formatter.exs"
    [ "$(application_digest)" != "$versioned" ] || { echo ".formatter.exs did not change the compile key" >&2; return 1; }
  }

  exercise_hidden_formatter_inputs() {
    mkdir -p "$WORKSPACE/.git" "$WORKSPACE/app"
    cp -R "$REPO_ROOT/crates/struo-frontend/prelude/examples/mix-workspace-native-project/." "$WORKSPACE/app/"
    rm "$WORKSPACE/app/struo.toml"
    # `mix format` expands its inputs with dotfiles, so it checks `.iex.exs`.
    printf '[inputs: ["*.{ex,exs}", "{lib,test}/**/*.{ex,exs}"]]\n' > "$WORKSPACE/app/.formatter.exs"
    printf 'IO.puts(:ready)\n' > "$WORKSPACE/app/.iex.exs"
    native_struo run mix_lint > "$SPEC_ROOT/lint.json" 2> "$SPEC_ROOT/lint.err" || { cat "$SPEC_ROOT/lint.err" >&2; return 1; }
    printf 'IO.puts( :ready )\n' > "$WORKSPACE/app/.iex.exs"
    if native_struo run mix_lint > "$SPEC_ROOT/relint.json" 2> "$SPEC_ROOT/relint.err"; then
      echo "a misformatted .iex.exs passed lint" >&2
      return 1
    fi
  }

  It 'lints the hidden Elixir files mix format checks'
    Skip if 'Mix toolchain unavailable on this host' mix_tools_unavailable
    When call exercise_hidden_formatter_inputs
    The status should be success
  End

  It 'keeps test scratch files and hidden trees out of the compile key'
    Skip if 'Mix toolchain unavailable on this host' mix_tools_unavailable
    When call exercise_untracked_project_files
    The status should be success
  End

  # A stand-in for mise: its shims run the real tool only with the user's own
  # HOME, as mise needs its configuration, and `mise which` names the binary.
  install_version_manager_shims() {
    elixir_bin="$(elixir -e 'IO.write(Path.expand("../../bin", :code.lib_dir(:elixir)))')" || return
    erlang_bin="$(elixir -e 'IO.write(Path.join(:code.root_dir(), "bin"))')" || return
    mkdir -p "$SPEC_ROOT/mise/bin" "$SPEC_ROOT/mise/shims"
    cat > "$SPEC_ROOT/mise/bin/mise" <<EOF
#!/bin/sh
name=\${0##*/}
case "\$name" in mise) name=\$2 ;; esac
case "\$name" in erl|escript) real="$erlang_bin/\$name" ;; *) real="$elixir_bin/\$name" ;; esac
if [ "\${0##*/}" = mise ]; then
  [ "\$1" = which ] && printf '%s\n' "\$real"
  exit
fi
[ "\$HOME" = "$HOME" ] || { echo "mise ERROR No version is set for shim: \$name" >&2; exit 1; }
exec "\$real" "\$@"
EOF
    chmod +x "$SPEC_ROOT/mise/bin/mise"
    for tool in elixir elixirc mix erl escript iex; do
      ln -s ../bin/mise "$SPEC_ROOT/mise/shims/$tool"
    done
  }

  exercise_version_manager_shims() {
    mkdir -p "$WORKSPACE/.git" "$WORKSPACE/app"
    cp -R "$REPO_ROOT/crates/struo-frontend/prelude/examples/mix-workspace-native-project/." "$WORKSPACE/app/"
    rm "$WORKSPACE/app/struo.toml"
    install_version_manager_shims || return
    PATH="$SPEC_ROOT/mise/shims:/usr/bin:/bin"
    native_struo build > "$SPEC_ROOT/build.json" 2> "$SPEC_ROOT/build.err" || { cat "$SPEC_ROOT/build.json" "$SPEC_ROOT/build.err" >&2; return 1; }
    native_struo test > "$SPEC_ROOT/test.json" 2> "$SPEC_ROOT/test.err" || { cat "$SPEC_ROOT/test.err" >&2; return 1; }
    # Lint runs `mix` itself inside an action.
    native_struo run mix_lint > "$SPEC_ROOT/lint.json" 2> "$SPEC_ROOT/lint.err" || { cat "$SPEC_ROOT/lint.err" >&2; return 1; }
  }

  It 'builds and tests through version-manager shims'
    Skip if 'Mix toolchain unavailable on this host' mix_tools_unavailable
    When call exercise_version_manager_shims
    The status should be success
  End

  exercise_child_code_paths() {
    mkdir -p "$WORKSPACE/.git" "$WORKSPACE/app"
    cp -R "$REPO_ROOT/crates/struo-frontend/prelude/examples/mix-workspace-native-project/." "$WORKSPACE/app/"
    rm "$WORKSPACE/app/struo.toml"
    # Like `mix test`, a node a test starts must not see the project's code,
    # or a library that loads its modules into that node skips the load.
    cat > "$WORKSPACE/app/test/child_node_test.exs" <<'EX'
defmodule ChildNodeTest do
  use ExUnit.Case

  test "a spawned VM does not inherit the project's code paths" do
    {loaded, 0} = System.cmd("elixir", ["-e", "IO.write(Code.ensure_loaded?(StruoNativeProjectExample))"])
    assert loaded == "false"
  end
end
EX
    native_struo test > "$SPEC_ROOT/test.json" 2> "$SPEC_ROOT/test.err" || { cat "$WORKSPACE/.struo/out/app/mix_tests/test/elixir-test.log" >&2; return 1; }
  }

  # The layout `mix new --umbrella` generates: children share the root's
  # lockfile, dependency sources, build directory, and configuration.
  prepare_umbrella() {
    umbrella="$WORKSPACE/app"
    example="$REPO_ROOT/crates/struo-frontend/prelude/examples/elixir-library-with-mix-dependency"
    mkdir -p "$WORKSPACE/.git" "$umbrella/config" "$umbrella/deps" "$umbrella/apps/greeter/lib" "$umbrella/apps/greeter/test" "$umbrella/apps/helper/lib"
    cp "$example/mix.lock" "$umbrella/"
    cp -R "$example/deps/locked_greeting" "$umbrella/deps/"
    printf 'import Config\n' > "$umbrella/config/config.exs"
    cat > "$umbrella/mix.exs" <<'EX'
defmodule Umbrella.MixProject do
  use Mix.Project
  def project, do: [apps_path: "apps", version: "0.1.0", deps: []]
end
EX
    for child in greeter helper; do
      deps='[]'
      module=Helper
      [ "$child" = greeter ] && module=Greeter
      [ "$child" = greeter ] && deps='[{:locked_greeting, "0.1.0"}, {:helper, in_umbrella: true}]'
      # Mix leaves a dependency for another target out of the host build.
      [ "$child" = helper ] && deps='[{:locked_greeting, "0.1.0", targets: :rpi}]'
      cat > "$umbrella/apps/$child/mix.exs" <<EX
defmodule ${module}.MixProject do
  use Mix.Project
  def project do
    [app: :$child, version: "0.1.0", build_path: "../../_build", config_path: "../../config/config.exs",
     deps_path: "../../deps", lockfile: "../../mix.lock", deps: $deps]
  end
end
EX
    done
    printf 'defmodule Helper do\n  def name, do: "Umbrella"\nend\n' > "$umbrella/apps/helper/lib/helper.ex"
    printf 'defmodule Helper.Late do\n  def value, do: :loaded\nend\n' > "$umbrella/apps/helper/lib/late.ex"
    printf 'defmodule Greeter do\n  def greet, do: LockedGreeting.message(Helper.name())\nend\n' > "$umbrella/apps/greeter/lib/greeter.ex"
    printf 'ExUnit.start()\n' > "$umbrella/apps/greeter/test/test_helper.exs"
    cat > "$umbrella/apps/greeter/test/greeter_test.exs" <<'EX'
defmodule GreeterTest do
  use ExUnit.Case
  test "greets through the locked package and the sibling app" do
    assert Greeter.greet() == "Hello, Umbrella!"
  end

  # Like Mix, dependency code and the run's own files stay reachable after a
  # test changes directory for good.
  test "loads a dependency module after changing directory" do
    File.cd!(System.tmp_dir!())
    assert Helper.Late.value() == :loaded
  end
end
EX
  }

  exercise_umbrella() {
    prepare_umbrella
    native_struo build > "$SPEC_ROOT/build.json" 2> "$SPEC_ROOT/build.err" || { cat "$SPEC_ROOT/build.err" >&2; return 1; }
    native_struo test > "$SPEC_ROOT/test.json" 2> "$SPEC_ROOT/test.err" || { cat "$SPEC_ROOT/test.err" "$WORKSPACE/.struo/out/app/apps/greeter/mix_tests/test/elixir-test.log" >&2; return 1; }
    "$SPEC_PYTHON3" - "$WORKSPACE/.struo/out/app/apps/greeter/mix_tests/test/test_results.json" <<'PY'
import json, sys
result = json.load(open(sys.argv[1]))
assert result["status"] == "passed" and result["summary"]["passed"] == 2, result
PY
    # Mix builds an in-umbrella sibling in the parent's environment.
    [ -d "$WORKSPACE/.struo/out/app/apps/helper/mix_application_test" ] || { echo "the sibling was not built for the test environment" >&2; return 1; }
    (cd "$WORKSPACE/app/apps/greeter" && "$STRUO_BIN" --format json test > "$SPEC_ROOT/child-test.json" 2> "$SPEC_ROOT/child-test.err") || { cat "$SPEC_ROOT/child-test.err" >&2; return 1; }
    (cd "$WORKSPACE/app/apps/helper" && "$STRUO_BIN" --format json query test-plan > /dev/null 2> "$SPEC_ROOT/child-plan.err") || { cat "$SPEC_ROOT/child-plan.err" >&2; return 1; }
    native_struo query test-plan > /dev/null 2> "$SPEC_ROOT/plan.err" || { cat "$SPEC_ROOT/plan.err" >&2; return 1; }
    native_struo query 'MATCH (a:Target {id: "apps/helper/mix_application_dev"})-[:DEPENDS_ON]->(d:Target) RETURN d.id' > "$SPEC_ROOT/helper-deps.json" 2>&1
    ! grep -q locked "$SPEC_ROOT/helper-deps.json" || { echo "a dependency for another target reached the host build" >&2; return 1; }
    for path in config/config.exs mix.lock; do
      selected="$(native_struo query affected-tests --changed-path "$path" 2> "$SPEC_ROOT/affected.err" | "$SPEC_PYTHON3" -c 'import json, sys; print(" ".join(test["id"] for test in json.load(sys.stdin)))')"
      [ "$selected" = apps/greeter/mix_tests ] || { echo "a change to the umbrella's $path selected '$selected'" >&2; return 1; }
    done
  }

  exercise_lockless_umbrella() {
    prepare_umbrella
    rm -rf "$WORKSPACE/app/mix.lock" "$WORKSPACE/app/deps" "$WORKSPACE/app/apps/helper/lib/late.ex"
    printf 'defmodule Greeter do\n  def greet, do: "Hello, #{Helper.name()}!"\nend\n' > "$WORKSPACE/app/apps/greeter/lib/greeter.ex"
    sed -i.bak -e 's/{:locked_greeting, "0.1.0"}, //' -e 's/\[{:locked_greeting, "0.1.0", targets: :rpi}\]/[]/' "$WORKSPACE/app/apps/greeter/mix.exs" "$WORKSPACE/app/apps/helper/mix.exs"
    "$SPEC_PYTHON3" - "$WORKSPACE/app/apps/greeter/test/greeter_test.exs" <<'PY'
import pathlib, re, sys
path = pathlib.Path(sys.argv[1])
path.write_text(re.sub(r"\n  # Like Mix.*?\n  end\n", "\n", path.read_text(), flags=re.S))
PY
    native_struo test > "$SPEC_ROOT/test.json" 2> "$SPEC_ROOT/test.err" || { cat "$SPEC_ROOT/test.err" >&2; return 1; }
  }

  It 'builds an umbrella whose children depend only on each other without a lockfile'
    Skip if 'Mix toolchain unavailable on this host' mix_tools_unavailable
    When call exercise_lockless_umbrella
    The status should be success
  End

  It 'builds and tests an umbrella whose children share the root lockfile'
    Skip if 'Mix toolchain unavailable on this host' mix_tools_unavailable
    When call exercise_umbrella
    The status should be success
  End

  It 'keeps the test build off the code path of processes a test spawns'
    Skip if 'Mix toolchain unavailable on this host' mix_tools_unavailable
    When call exercise_child_code_paths
    The status should be success
  End

  test_binding() {
    native_struo query test-plan > "$SPEC_ROOT/plan.json" 2> "$SPEC_ROOT/plan.err" || { cat "$SPEC_ROOT/plan.err"; return 1; }
    "$SPEC_PYTHON3" -c 'import json, sys; print(json.load(open(sys.argv[1]))["bindings"][0]["fingerprint"])' "$SPEC_ROOT/plan.json"
  }

  selected_tests() {
    native_struo query affected-tests --changed-path "$1" 2> "$SPEC_ROOT/affected.err" | "$SPEC_PYTHON3" -c 'import json, sys; print(" ".join(test["id"] for test in json.load(sys.stdin)))'
  }

  exercise_dependency_build_directories() {
    mkdir -p "$WORKSPACE/.git" "$WORKSPACE/app/deps"
    example="$REPO_ROOT/crates/struo-frontend/prelude/examples"
    cp -R "$example/mix-workspace-native-project/." "$WORKSPACE/app/"
    rm "$WORKSPACE/app/struo.toml"
    cp "$example/elixir-library-with-mix-dependency/mix.lock" "$WORKSPACE/app/"
    cp -R "$example/elixir-library-with-mix-dependency/deps/locked_greeting" "$WORKSPACE/app/deps/"
    "$SPEC_PYTHON3" - "$WORKSPACE/app/mix.exs" <<'PY'
import pathlib, sys
path = pathlib.Path(sys.argv[1])
path.write_text(path.read_text().replace('elixir: "~> 1.15"', 'elixir: "~> 1.15",\n      deps: [{:locked_greeting, "0.1.0"}]'))
PY
    before="$(test_binding)" || return
    # Mix and Rebar leave their build directories inside a fetched package.
    mkdir -p "$WORKSPACE/app/deps/locked_greeting/_build/prod"
    printf 'build output' > "$WORKSPACE/app/deps/locked_greeting/_build/prod/output"
    [ "$(test_binding)" = "$before" ] || { echo "a dependency build directory changed the test binding" >&2; return 1; }
    [ -z "$(selected_tests deps/locked_greeting/_build/prod/output)" ] || { echo "a dependency build directory selected tests" >&2; return 1; }
    [ "$(selected_tests deps/locked_greeting/lib/locked_greeting.ex)" = mix_tests ] || { echo "a dependency source did not select tests" >&2; return 1; }
  }

  It 'keeps dependency build directories out of test bindings and selection'
    Skip if 'Mix toolchain unavailable on this host' mix_tools_unavailable
    export MIX_HOME="$SPEC_ORIGINAL_HOME/.mix"
    When call exercise_dependency_build_directories
    The status should be success
  End

  It 'resolves each Mix environment with the dependencies that environment selects'
    Skip if 'Mix toolchain unavailable on this host' mix_tools_unavailable
    When call exercise_environment_specific_dependency_graphs
    The status should be success
  End

  It 'reports actual failing tests instead of a successful empty invocation'
    Skip if 'Mix toolchain unavailable on this host' mix_tools_unavailable
    prepare_native_workspace
    When call exercise_failing_test
    The status should be success
  End
End
