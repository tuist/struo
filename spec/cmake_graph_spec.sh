#shellcheck shell=bash

Describe 'CTest graph target kind'
  BeforeEach 'setup_workspace'
  AfterEach 'cleanup_workspace'

  cmake_tools_unavailable() {
    ! command -v cmake >/dev/null 2>&1 || ! command -v ctest >/dev/null 2>&1 || ! command -v ninja >/dev/null 2>&1 || ! command -v cc >/dev/null 2>&1
  }

  write_ctest_workspace() {
    cp "$REPO_ROOT/crates/struo-frontend/prelude/examples/cmake-test-minimal/"* "$WORKSPACE/"
  }

  restore_ctest_tree() {
    write_ctest_workspace
    struo --format json build Tests || return
    rm -rf "$WORKSPACE/.struo/out/Tests/cmake-build"
    struo --format json build Tests || return
    struo --format json test Tests || return
    cat "$WORKSPACE/.struo/out/Tests/test/test_results.json"
  }

  It 'rebuilds a deleted native test tree and reports one passing suite'
    Skip if 'CMake native toolchain unavailable' cmake_tools_unavailable
    When call restore_ctest_tree
    The status should be success
    The stdout should include '"cache":"bypass"'
    The stdout should include '"total":1,"passed":1,"failed":0'
    The contents of file "$WORKSPACE/.struo/out/Tests/test/ctest.xml" should include 'check_sum'
  End

  fail_ctest_suite() {
    write_ctest_workspace
    printf '%s\n' 'int main(void) { return 7; }' > "$WORKSPACE/test.c"
    struo --format json test Tests > "$WORKSPACE/failure.json" 2>&1
    code=$?
    cat "$WORKSPACE/.struo/out/Tests/test/test_results.json"
    return "$code"
  }

  It 'propagates native failure while retaining normalized failed results'
    Skip if 'CMake native toolchain unavailable' cmake_tools_unavailable
    When call fail_ctest_suite
    The status should be failure
    The stdout should include '"total":1,"passed":0,"failed":1'
    The contents of file "$WORKSPACE/.struo/out/Tests/test/ctest.xml" should include '<failure'
  End

  fail_empty_ctest_suite() {
    write_ctest_workspace
    printf '%s\n' 'cmake_minimum_required(VERSION 3.21)' 'project(Empty C)' 'enable_testing()' > "$WORKSPACE/CMakeLists.txt"
    struo --format json test Tests > "$WORKSPACE/failure.json" 2>&1
    code=$?
    cat "$WORKSPACE/.struo/out/Tests/test/test_results.json"
    return "$code"
  }

  It 'fails an empty native suite rather than claiming a passing test'
    Skip if 'CMake native toolchain unavailable' cmake_tools_unavailable
    When call fail_empty_ctest_suite
    The status should be failure
    The stdout should include '"status":"failed"'
  End

  run_custom_cmake_suite() {
    mode="$1"
    write_ctest_workspace
    cat > "$WORKSPACE/CMakeLists.txt" <<'CMAKE'
cmake_minimum_required(VERSION 3.21)
project(CustomTests C)
add_executable(check_sum test.c)
add_custom_target(native-check COMMAND check_sum DEPENDS check_sum)
add_custom_target(second-check COMMAND "${CMAKE_COMMAND}" -E echo "second suite executed")
add_custom_target(slow-check COMMAND "${CMAKE_COMMAND}" -E sleep 3)
CMAKE
    targets='"native-check", "second-check"'
    case "$mode" in
      failure) printf '%s\n' 'int main(void) { return 7; }' > "$WORKSPACE/test.c" ;;
      missing) targets='"missing-check"' ;;
      timeout) targets='"slow-check"'; printf '%s\n' 'timeout_ms = 100' >> "$WORKSPACE/struo.toml" ;;
    esac
    printf 'test_targets = [%s]\nctest = "nonexistent-ctest"\n' "$targets" >> "$WORKSPACE/struo.toml"
    struo --format json test Tests > "$WORKSPACE/run.json" 2>&1
    code=$?
    cat "$WORKSPACE/.struo/out/Tests/test/test_results.json"
    return "$code"
  }

  It 'runs explicit native test targets without CTest and retains logs without invented JUnit'
    Skip if 'CMake native toolchain unavailable' cmake_tools_unavailable
    When call run_custom_cmake_suite success
    The status should be success
    The stdout should include '"total":1,"passed":1,"failed":0'
    The stdout should include '"native_results":[]'
    The contents of file "$WORKSPACE/.struo/out/Tests/test/cmake-test.log" should include 'second suite executed'
    The path "$WORKSPACE/.struo/out/Tests/test/ctest.xml" should not be exist
  End

  Describe 'custom native test failures'
    Parameters
      failure
      missing
      timeout
    End

    It "retains normalized failure for $1"
      Skip if 'CMake native toolchain unavailable' cmake_tools_unavailable
      When call run_custom_cmake_suite "$1"
      The status should be failure
      The stdout should include '"total":1,"passed":0,"failed":1'
      The contents of file "$WORKSPACE/.struo/out/Tests/test/cmake-test.log" should include 'CMake test runner failed:'
    End
  End

  preserve_configure_argument_boundaries() {
    write_ctest_workspace
    cat >> "$WORKSPACE/struo.toml" <<'TOML'
configure_args = ['-DPROBE_LIST=a;-DPROBE_SPLIT=b', '-DPROBE_PATH=C:\dir', '-DPROBE_BRACKET=[x', '-DPROBE_AFTER=y']
TOML
    struo --format json test Tests > /dev/null || return
    grep '^PROBE_' "$WORKSPACE/.struo/out/Tests/cmake-build/CMakeCache.txt" | sort
  }

  It 'passes each configure argument to CMake as one native argument'
    Skip if 'CMake native toolchain unavailable' cmake_tools_unavailable
    When call preserve_configure_argument_boundaries
    The status should be success
    The line 1 of stdout should equal 'PROBE_AFTER:UNINITIALIZED=y'
    The line 2 of stdout should equal 'PROBE_BRACKET:UNINITIALIZED=[x'
    The line 3 of stdout should equal 'PROBE_LIST:UNINITIALIZED=a;-DPROBE_SPLIT=b'
    The line 4 of stdout should equal 'PROBE_PATH:UNINITIALIZED=C:\dir'
    The lines of stdout should equal 4
  End

  reject_relocated_ctest_tree() {
    write_ctest_workspace
    struo --format json build Tests > "$WORKSPACE/original.json" || return
    mkdir "$WORKSPACE/other"
    cp "$WORKSPACE/struo.toml" "$WORKSPACE/CMakeLists.txt" "$WORKSPACE/test.c" "$WORKSPACE/other/"
    cp -R "$WORKSPACE/.struo" "$WORKSPACE/other/.struo"
    "$STRUO_BIN" -C "$WORKSPACE/other" --format json build Tests > "$WORKSPACE/relocated.json" || return
    "$SPEC_PYTHON3" - "$WORKSPACE" <<'PY'
import json, pathlib, sys
root = pathlib.Path(sys.argv[1])
original = json.loads((root / 'original.json').read_text())
relocated = json.loads((root / 'relocated.json').read_text())
assert original['action_digest'] != relocated['action_digest']
assert relocated['cache'] == 'bypass', relocated
cache = (root / 'other/.struo/out/Tests/cmake-build/CMakeCache.txt').read_text()
assert str(root / 'other') in cache
print('different workspace: distinct actual key and cache bypass')
PY
  }

  It 'does not reuse a captured native build tree at a different workspace path'
    Skip if 'CMake native toolchain unavailable' cmake_tools_unavailable
    When call reject_relocated_ctest_tree
    The status should be success
    The stdout should include 'distinct actual key and cache bypass'
  End

  reject_sandboxed_ctest() {
    write_ctest_workspace
    struo --format json build --sandbox copied-inputs Tests
  }

  It 'rejects sandbox relocation instead of caching an invalid native tree'
    Skip if 'CMake native toolchain unavailable' cmake_tools_unavailable
    When call reject_sandboxed_ctest
    The status should be failure
    The stderr should include 'requires execution at its original workspace path'
    The stderr should include 'session failed'
  End

  timeout_ctest_suite() {
    write_ctest_workspace
    printf '%s\n' 'timeout_ms = 100' >> "$WORKSPACE/struo.toml"
    printf '%s\n' 'add_test(NAME slow COMMAND "${CMAKE_COMMAND}" -E sleep 3)' >> "$WORKSPACE/CMakeLists.txt"
    struo --format json test Tests > "$WORKSPACE/failure.json" 2>&1
    code=$?
    cat "$WORKSPACE/.struo/out/Tests/test/test_results.json"
    return "$code"
  }

  It 'enforces a suite timeout and retains normalized failure results'
    Skip if 'CMake native toolchain unavailable' cmake_tools_unavailable
    When call timeout_ctest_suite
    The status should be failure
    The stdout should include '"status":"failed"'
    The contents of file "$WORKSPACE/.struo/out/Tests/test/ctest.log" should include 'Process terminated due to timeout'
  End

  consume_generated_cmake_header() {
    value="$1"
    cat > "$WORKSPACE/CMakeLists.txt" <<'CMAKE'
cmake_minimum_required(VERSION 3.20)
project(Gen C)
include(values.cmake)
configure_file(config.h.in "${CMAKE_BINARY_DIR}/include/gen/config.h")
add_library(gen STATIC gen.c)
target_include_directories(gen PUBLIC "${CMAKE_BINARY_DIR}/include")
CMAKE
    printf 'set(GEN_VALUE 42)\n' > "$WORKSPACE/values.cmake"
    printf '#define GEN_VALUE @GEN_VALUE@\n' > "$WORKSPACE/config.h.in"
    printf '#include <gen/config.h>\nint gen_value(void) { return GEN_VALUE; }\n' > "$WORKSPACE/gen.c"
    printf '#include <gen/config.h>\n_Static_assert(GEN_VALUE == 42, "stale generated header");\nint consumer(void) { return GEN_VALUE; }\n' > "$WORKSPACE/consumer.c"
    cat > "$WORKSPACE/struo.toml" <<'TOML'
[[target]]
name = "Gen"
kind = "cmake_project"
srcs = ["CMakeLists.txt", "values.cmake", "config.h.in", "gen.c"]
[target.attrs]
products = ["{static_prefix}gen{static_suffix}", "include/gen/config.h"]
generated_includes = ["include"]

[[target]]
name = "Consumer"
kind = "c_library"
srcs = ["consumer.c"]
deps = ["Gen"]
TOML
    struo --format json build --sandbox copied-inputs Consumer > "$WORKSPACE/first.json" || return
    [ "$value" = 42 ] && return
    printf 'set(GEN_VALUE %s)\n' "$value" > "$WORKSPACE/values.cmake"
    struo --format json build --sandbox copied-inputs Consumer
  }

  It 'publishes headers CMake generates to native dependents'
    Skip if 'CMake native toolchain unavailable' cmake_tools_unavailable
    When call consume_generated_cmake_header 42
    The status should be success
    The contents of file "$WORKSPACE/first.json" should include '"status":"completed"'
  End

  It 'rebuilds dependents when a generated CMake header changes'
    Skip if 'CMake native toolchain unavailable' cmake_tools_unavailable
    When call consume_generated_cmake_header 7
    The status should be failure
    The stderr should include 'stale generated header'
  End

  reuse_dependent_of_generated_root() {
    layout="$1"
    cat > "$WORKSPACE/CMakeLists.txt" <<'CMAKE'
cmake_minimum_required(VERSION 3.20)
project(Gen C)
file(WRITE "${CMAKE_BINARY_DIR}/gen/config.h" "#define GEN_VALUE 42\n")
file(WRITE "${CMAKE_BINARY_DIR}/include/gen/config.h" "#define GEN_VALUE 42\n")
string(RANDOM LENGTH 16 stamp)
file(WRITE "${CMAKE_BINARY_DIR}/stamp.txt" "${stamp}\n")
add_library(gen STATIC gen.c)
target_include_directories(gen PUBLIC "${CMAKE_BINARY_DIR}")
CMAKE
    printf '#include <gen/config.h>\nint gen_value(void) { return GEN_VALUE; }\n' > "$WORKSPACE/gen.c"
    printf '#include <gen/config.h>\nint consumer(void) { return GEN_VALUE; }\n' > "$WORKSPACE/consumer.c"
    case "$layout" in
      files) products='"./{static_prefix}gen{static_suffix}", "./gen/config.h", "stamp.txt"'; includes='"./"' ;;
      directory) products='"{static_prefix}gen{static_suffix}", "include", "stamp.txt"'; includes='"include/"' ;;
    esac
    cat > "$WORKSPACE/struo.toml" <<TOML
[[target]]
name = "Gen"
kind = "cmake_project"
srcs = ["CMakeLists.txt", "gen.c"]
[target.attrs]
products = [$products]
generated_includes = [$includes]

[[target]]
name = "Consumer"
kind = "c_library"
srcs = ["consumer.c"]
deps = ["Gen"]
TOML
    struo --format json build --sandbox copied-inputs Consumer > /dev/null || return
    struo --format json build --sandbox copied-inputs Consumer
  }

  It 'publishes only header products from a generated include directory'
    Skip if 'CMake native toolchain unavailable' cmake_tools_unavailable
    When call reuse_dependent_of_generated_root files
    The status should be success
    The stdout should include '"cache":"hit"'
  End

  It 'publishes a generated header directory product'
    Skip if 'CMake native toolchain unavailable' cmake_tools_unavailable
    When call reuse_dependent_of_generated_root directory
    The status should be success
    The stdout should include '"cache":"hit"'
  End

  reuse_versioned_shared_library() {
    printf 'cmake_minimum_required(VERSION 3.20)\nproject(Shared C)\nadd_library(shared SHARED shared.c)\nset_target_properties(shared PROPERTIES VERSION 1.2.3 SOVERSION 1)\n' > "$WORKSPACE/CMakeLists.txt"
    printf 'int shared_value(void) { return 1; }\n' > "$WORKSPACE/shared.c"
    printf '[[target]]\nname = "Shared"\nkind = "cmake_project"\nsrcs = ["CMakeLists.txt", "shared.c"]\n[target.attrs]\nproducts = ["{shared_prefix}shared{shared_suffix}"]\n' > "$WORKSPACE/struo.toml"
    struo --format json build Shared > /dev/null || return
    struo --format json build Shared > /dev/null || return
    struo --format json query evidence > "$WORKSPACE/evidence.json" || return
    "$SPEC_PYTHON3" - "$WORKSPACE" <<'PY'
import json, pathlib, sys
root = pathlib.Path(sys.argv[1])
copies = [record['cache'] for record in json.loads((root / 'evidence.json').read_text())
          if record.get('kind') == 'action_result' and record.get('cache') != 'bypass'
          and any(c['category'] == 'generated-input' and c['label'].endswith('libshared.so')
                  for c in record['input_fingerprint']['components'])]
assert sorted(copies) == ['hit', 'miss'], copies
print('product staging reused')
PY
  }

  It 'reuses the staged copy of a versioned shared library product'
    Skip if 'CMake native toolchain unavailable' cmake_tools_unavailable
    Skip if 'shared library naming is Linux-specific' [ "$(uname -s)" != Linux ]
    When call reuse_versioned_shared_library
    The status should be success
    The stdout should include 'product staging reused'
  End

  write_discovered_cmake_project() {
    cat > "$WORKSPACE/CMakeLists.txt" <<'CMAKE'
cmake_minimum_required(VERSION 3.20)
project(Discovered C)
set(GREET_VALUE 42)
configure_file(config.h.in "${CMAKE_BINARY_DIR}/greet/config.h")
add_library(greet STATIC greet.c greet.h "${CMAKE_BINARY_DIR}/greet/config.h")
target_include_directories(greet PUBLIC "${CMAKE_SOURCE_DIR}" "${CMAKE_BINARY_DIR}")
add_executable(check check.c)
target_link_libraries(check PRIVATE greet)
enable_testing()
add_test(NAME check COMMAND check)
CMAKE
    printf '#define GREET_VALUE @GREET_VALUE@\n' > "$WORKSPACE/config.h.in"
    printf 'int greet(void);\n' > "$WORKSPACE/greet.h"
    printf '#include <greet/config.h>\n#include "greet.h"\nint greet(void) { return GREET_VALUE; }\n' > "$WORKSPACE/greet.c"
    printf '#include "greet.h"\nint main(void) { return greet() == 42 ? 0 : 1; }\n' > "$WORKSPACE/check.c"
    printf 'This file is not part of the build.\n' > "$WORKSPACE/README.md"
  }

  build_discovered_cmake_project() {
    write_discovered_cmake_project
    mkdir "$WORKSPACE/consumer"
    printf '#include <greet/config.h>\n#include <greet.h>\nint consumer(void) { return greet() + GREET_VALUE; }\n' > "$WORKSPACE/consumer/consumer.c"
    printf '[[target]]\nname = "consumer"\nkind = "c_library"\nsrcs = ["consumer.c"]\ndeps = ["../greet"]\n' > "$WORKSPACE/consumer/struo.toml"
    struo --format json query targets > "$WORKSPACE/targets.json" || return
    struo --format json build consumer/consumer > /dev/null || return
    struo --format json test ctest__check > /dev/null || return
    struo --format json query affected-tests --changed-path greet.c > "$WORKSPACE/affected.json" || return
    "$SPEC_PYTHON3" - "$WORKSPACE" <<'PY'
import json, pathlib, sys
root = pathlib.Path(sys.argv[1])
kinds = {target['id']: target['kind'] for target in json.loads((root / 'targets.json').read_text())}
assert kinds.get('greet') == 'cmake_project', kinds
assert kinds.get('check') == 'cmake_project', kinds
assert kinds.get('ctest__check') == 'cmake_test', kinds
assert 'ctest' not in kinds, kinds
assert (root / '.struo/out/greet/products/libgreet.a').is_file()
assert (root / '.struo/out/greet/products/greet/config.h').is_file()
results = json.loads((root / '.struo/out/ctest__check/test/test_results.json').read_text())
assert results['status'] == 'passed', results
assert [test['id'] for test in json.loads((root / 'affected.json').read_text())] == ['ctest__check']
print('discovered, built, consumed, and tested')
PY
  }

  It 'discovers a CMake project without a Struo manifest and builds, links, and tests it'
    Skip if 'CMake native toolchain unavailable' cmake_tools_unavailable
    When call build_discovered_cmake_project
    The status should be success
    The stdout should include 'discovered, built, consumed, and tested'
  End

  discovered_cmake_identity() {
    write_discovered_cmake_project
    struo --format json query graph-fingerprint > "$WORKSPACE/first.json" || return
    mkdir -p "$SPEC_ROOT/elsewhere/nested"
    cp -R "$WORKSPACE" "$SPEC_ROOT/elsewhere/nested/workspace"
    "$STRUO_BIN" -C "$SPEC_ROOT/elsewhere/nested/workspace" --format json query graph-fingerprint > "$WORKSPACE/moved.json" || return
    struo --format json build greet > /dev/null || return
    "$STRUO_BIN" -C "$SPEC_ROOT/elsewhere/nested/workspace" --format json build greet > /dev/null || return
    cmp "$WORKSPACE/.struo/out/greet/products/libgreet.a" "$SPEC_ROOT/elsewhere/nested/workspace/.struo/out/greet/products/libgreet.a" || return
    printf 'More notes.\n' >> "$WORKSPACE/README.md"
    struo --format json query graph-fingerprint > "$WORKSPACE/unrelated.json" || return
    "$SPEC_PYTHON3" - "$WORKSPACE" <<'PY'
import json, pathlib, sys
root = pathlib.Path(sys.argv[1])
def targets(name):
    return [c for c in json.loads((root / name).read_text())['components'] if c['category'] == 'target']
first = targets('first.json')
assert {c['label'] for c in first} >= {'greet', 'check', 'ctest__check'}, first
assert first == targets('moved.json'), 'target hashes depend on the checkout path'
assert first == targets('unrelated.json'), 'an unrelated file changed target hashes'
print('stable target identity')
PY
  }

  It 'gives discovered CMake targets the same identity at any checkout path'
    Skip if 'CMake native toolchain unavailable' cmake_tools_unavailable
    When call discovered_cmake_identity
    The status should be success
    The stdout should include 'stable target identity'
  End

  debug_products_at_any_checkout_path() {
    printf 'cmake_minimum_required(VERSION 3.20)\nproject(Trace C)\nif(NOT CMAKE_BUILD_TYPE)\n  set(CMAKE_BUILD_TYPE Debug CACHE STRING "" FORCE)\nendif()\nenable_language(CXX)\nadd_library(trace STATIC trace.c late.cpp)\nadd_library(shared SHARED shared.c)\nadd_executable(app app.c)\ntarget_link_libraries(app PRIVATE trace shared)\n' > "$WORKSPACE/CMakeLists.txt"
    printf 'const char *trace(void) { return __FILE__; }\n' > "$WORKSPACE/trace.c"
    printf 'int shared(void) { return 1; }\n' > "$WORKSPACE/shared.c"
    printf 'extern "C" const char *late(void) { return __FILE__; }\n' > "$WORKSPACE/late.cpp"
    printf 'const char *trace(void);\nint shared(void);\nint main(void) { return trace()[0] == 0 || shared() != 1; }\n' > "$WORKSPACE/app.c"
    moved="$SPEC_ROOT/elsewhere/nested/workspace"
    mkdir -p "$SPEC_ROOT/elsewhere/nested"
    cp -R "$WORKSPACE" "$moved"
    products='trace/products/libtrace.a'
    [ "$(uname -s)" = Linux ] && products="$products app/products/app"
    for target in trace app; do
      struo --format json build "$target" > /dev/null || return
      "$STRUO_BIN" -C "$moved" --format json build "$target" > /dev/null || return
    done
    for product in $products; do
      cmp "$WORKSPACE/.struo/out/$product" "$moved/.struo/out/$product" || return
    done
    echo 'identical debug products'
  }

  It 'builds identical debug products at any checkout path'
    Skip if 'CMake native toolchain unavailable' cmake_tools_unavailable
    When call debug_products_at_any_checkout_path
    The status should be success
    The stdout should include 'identical debug products'
  End

  globbed_cmake_sources_stay_stable() {
    printf 'cmake_minimum_required(VERSION 3.20)\nproject(Glob C)\nfile(GLOB_RECURSE GLOB_SOURCES "${CMAKE_SOURCE_DIR}/*.c")\nadd_library(globbed STATIC ${GLOB_SOURCES})\n' > "$WORKSPACE/CMakeLists.txt"
    printf 'int globbed(void) { return 1; }\n' > "$WORKSPACE/globbed.c"
    struo --format json query graph-fingerprint > "$WORKSPACE/before.json" || return
    struo --format json build globbed > /dev/null || return
    struo --format json query graph-fingerprint > "$WORKSPACE/after.json" || return
    struo --format json query target globbed
    "$SPEC_PYTHON3" - "$WORKSPACE" <<'PY'
import json, pathlib, sys
root = pathlib.Path(sys.argv[1])
def targets(name):
    return [c for c in json.loads((root / name).read_text())['components'] if c['category'] == 'target']
assert targets('before.json') == targets('after.json'), 'building changed the discovered graph'
print('globbed sources stable')
PY
  }

  It 'keeps Struo build trees out of globbed CMake sources'
    Skip if 'CMake native toolchain unavailable' cmake_tools_unavailable
    When call globbed_cmake_sources_stay_stable
    The status should be success
    The stdout should include 'globbed sources stable'
    The stdout should not include '.struo'
  End

  failing_nested_cmake_project() {
    mkdir -p "$WORKSPACE/native" "$WORKSPACE/ext/lib"
    printf 'int native(void) { return 1; }\n' > "$WORKSPACE/native/native.c"
    printf '[[target]]\nname = "native"\nkind = "c_library"\nsrcs = ["native.c"]\n' > "$WORKSPACE/native/struo.toml"
    printf 'cmake_minimum_required(VERSION 3.20)\nproject(Broken C)\nfind_package(StruoMissingPackage REQUIRED)\n' > "$WORKSPACE/ext/lib/CMakeLists.txt"
    struo --format json build native/native > /dev/null || return
    struo --format json build ext/lib/cmake
  }

  It 'reports a CMake project that fails to configure without blocking unrelated targets'
    Skip if 'CMake native toolchain unavailable' cmake_tools_unavailable
    When call failing_nested_cmake_project
    The status should be failure
    The stderr should include 'StruoMissingPackage'
    The stderr should include 'exclude = [\"ext/lib\"]'
  End

  authored_cmake_target_claims_project() {
    write_discovered_cmake_project
    printf '[[target]]\nname = "Lib"\nkind = "cmake_project"\nsrcs = ["CMakeLists.txt", "*.c", "*.h", "*.in"]\n[target.attrs]\nproducts = ["libgreet.a"]\n' > "$WORKSPACE/struo.toml"
    struo --format json query targets
  }

  It 'leaves a CMake project to an authored target that covers it'
    Skip if 'CMake native toolchain unavailable' cmake_tools_unavailable
    When call authored_cmake_target_claims_project
    The status should be success
    The stdout should include '"id":"Lib"'
    The stdout should not include '"id":"greet"'
  End

  discovery_reconfigures_only_on_change() {
    mkdir "$WORKSPACE/src"
    cat > "$WORKSPACE/CMakeLists.txt" <<'CMAKE'
cmake_minimum_required(VERSION 3.20)
project(Reuse C)
file(APPEND "$ENV{STRUO_SPEC_CONFIGURES}" "configure\n")
file(GLOB REUSE_SOURCES "${CMAKE_SOURCE_DIR}/src/*.c")
add_library(reuse STATIC ${REUSE_SOURCES})
CMAKE
    printf 'int first(void) { return 1; }\n' > "$WORKSPACE/src/first.c"
    export STRUO_SPEC_CONFIGURES="$SPEC_ROOT/configures"
    configures() { wc -l < "$STRUO_SPEC_CONFIGURES" | tr -d ' '; }
    struo --format json query targets > /dev/null || return
    struo --format json query targets > /dev/null || return
    echo "unchanged: $(configures)"
    printf 'int first(void) { return 2; }\n' > "$WORKSPACE/src/first.c"
    struo --format json query targets > /dev/null || return
    echo "source edited: $(configures)"
    printf 'int second(void) { return 2; }\n' > "$WORKSPACE/src/second.c"
    struo --format json query target reuse > "$WORKSPACE/target.json" || return
    echo "source added: $(configures)"
    grep -q 'src/second.c' "$WORKSPACE/target.json" && echo 'new source discovered'
    printf '# comment\n' >> "$WORKSPACE/CMakeLists.txt"
    struo --format json query targets > /dev/null || return
    echo "configuration edited: $(configures)"
  }

  It 'reconfigures a discovered CMake project only when its configuration inputs change'
    Skip if 'CMake native toolchain unavailable' cmake_tools_unavailable
    When call discovery_reconfigures_only_on_change
    The status should be success
    The line 1 of stdout should equal 'unchanged: 1'
    The line 2 of stdout should equal 'source edited: 1'
    The line 3 of stdout should equal 'source added: 2'
    The line 4 of stdout should equal 'new source discovered'
    The line 5 of stdout should equal 'configuration edited: 3'
  End

  select_discovered_ctest_tests() {
    mkdir "$WORKSPACE/data"
    cat > "$WORKSPACE/CMakeLists.txt" <<'CMAKE'
cmake_minimum_required(VERSION 3.20)
project(Select C)
enable_testing()
add_library(alpha STATIC alpha.c)
add_library(beta STATIC beta.c)
add_executable(alpha_check alpha_check.c)
target_link_libraries(alpha_check PRIVATE alpha)
add_executable(beta_check beta_check.c)
target_link_libraries(beta_check PRIVATE beta)
add_test(NAME alpha COMMAND alpha_check)
add_test(NAME beta COMMAND beta_check)
add_test(NAME beta/data COMMAND beta_check "${CMAKE_SOURCE_DIR}/data/beta.txt")
CMAKE
    printf 'int alpha(void) { return 1; }\n' > "$WORKSPACE/alpha.c"
    printf 'int beta(void) { return 2; }\n' > "$WORKSPACE/beta.c"
    printf 'int alpha(void);\nint main(void) { return alpha() == 1 ? 0 : 1; }\n' > "$WORKSPACE/alpha_check.c"
    printf 'int beta(void);\nint main(void) { return beta() == 2 ? 0 : 1; }\n' > "$WORKSPACE/beta_check.c"
    printf 'beta\n' > "$WORKSPACE/data/beta.txt"
    printf 'Notes.\n' > "$WORKSPACE/README.md"
    for path in alpha.c beta.c data/beta.txt CMakeLists.txt README.md; do
      printf '%s %s\n' "$path" "$(struo --format json query affected-tests --changed-path "$path" | "$SPEC_PYTHON3" -c 'import json, sys; print(",".join(t["id"] for t in json.load(sys.stdin)))')" || return
    done
    struo --format json test > /dev/null || return
    "$SPEC_PYTHON3" - "$WORKSPACE" <<'PY'
import json, pathlib, sys
root = pathlib.Path(sys.argv[1])
for name in ['ctest__alpha', 'ctest__beta', 'ctest__beta_data']:
    results = json.loads((root / '.struo/out' / name / 'test/test_results.json').read_text())
    assert results['status'] == 'passed', results
print('each test ran from one shared build')
PY
  }

  It 'selects only the CTest tests whose executables a change reaches'
    Skip if 'CMake native toolchain unavailable' cmake_tools_unavailable
    When call select_discovered_ctest_tests
    The status should be success
    The line 1 of stdout should equal 'alpha.c ctest__alpha'
    The line 2 of stdout should equal 'beta.c ctest__beta,ctest__beta_data'
    The line 3 of stdout should equal 'data/beta.txt ctest__beta_data'
    The line 4 of stdout should equal 'CMakeLists.txt ctest__alpha,ctest__beta,ctest__beta_data'
    The line 5 of stdout should equal 'README.md ctest__alpha,ctest__beta,ctest__beta_data'
    The line 6 of stdout should equal 'each test ran from one shared build'
  End

  standalone_assembler_builds() {
    printf 'cmake_minimum_required(VERSION 3.20)\nproject(Bare C ASM)\nadd_library(bare STATIC bare.s)\n' > "$WORKSPACE/CMakeLists.txt"
    printf '.globl bare\nbare:\n  ret\n' > "$WORKSPACE/bare.s"
    printf '[[target]]\nname = "Bare"\nkind = "cmake_project"\nsrcs = ["CMakeLists.txt", "bare.s"]\n[target.attrs]\nproducts = ["libbare.a"]\nconfigure_args = ["-DCMAKE_ASM_COMPILER=%s"]\n' "$(command -v as)" > "$WORKSPACE/struo.toml"
    struo --format json build Bare > /dev/null && echo 'standalone assembler builds'
  }

  It 'keeps the native flags of a standalone assembler'
    Skip if 'CMake native toolchain unavailable' cmake_tools_unavailable
    Skip if 'GNU as unavailable' [ "$(uname -s)" != Linux ] || ! command -v as >/dev/null 2>&1
    When call standalone_assembler_builds
    The status should be success
    The stdout should include 'standalone assembler builds'
  End

  shared_tree_follows_compiler() {
    real_cc="$(command -v cc)"
    mkdir "$WORKSPACE/sdk"
    printf 'cmake_minimum_required(VERSION 3.20)\nproject(Shared C)\nenable_testing()\nadd_executable(value value.c)\nadd_test(NAME value COMMAND value "${CMAKE_SOURCE_DIR}/expected.txt")\n' > "$WORKSPACE/CMakeLists.txt"
    printf '#include <stdio.h>\nint main(int argc, char **argv) { int expected = 0; FILE *file = fopen(argv[1], "r"); return !file || fscanf(file, "%%d", &expected) != 1 || expected != BUILD_VALUE; }\n' > "$WORKSPACE/value.c"
    cat > "$WORKSPACE/struo.toml" <<TOML
[[target]]
name = "tree"
kind = "cmake_project"
srcs = ["CMakeLists.txt", "value.c"]
[target.attrs]
export_build_tree = true
configure_args = ["-DCMAKE_C_COMPILER:FILEPATH=$WORKSPACE/sdk/compiler"]

[[target]]
name = "check"
kind = "cmake_test"
srcs = ["CMakeLists.txt", "value.c"]
deps = ["tree"]
[target.attrs]
tests = ["value"]
TOML
    for value in 11 29; do
      printf '#!/bin/sh\nexec "%s" -DBUILD_VALUE=%s "$@"\n' "$real_cc" "$value" > "$WORKSPACE/sdk/compiler"
      chmod +x "$WORKSPACE/sdk/compiler"
      printf '%s\n' "$value" > "$WORKSPACE/expected.txt"
      struo --format json test check > /dev/null || return
      echo "value=$value"
    done
  }

  It 'rebuilds a shared CMake build tree when its compiler changes'
    Skip if 'CMake native toolchain unavailable' cmake_tools_unavailable
    When call shared_tree_follows_compiler
    The status should be success
    The line 1 of stdout should equal 'value=11'
    The line 2 of stdout should equal 'value=29'
  End

  group_linked_ctest_tests() {
    cat > "$WORKSPACE/CMakeLists.txt" <<'CMAKE'
cmake_minimum_required(VERSION 3.20)
project(Linked C)
enable_testing()
add_library(deep STATIC deep.c)
set_target_properties(deep PROPERTIES POSITION_INDEPENDENT_CODE ON)
add_library(middle SHARED middle.c)
target_link_libraries(middle PRIVATE deep)
add_executable(probe probe.c)
target_link_libraries(probe PRIVATE middle)
add_executable(plain plain.c)
add_test(NAME deep COMMAND probe)
add_test(NAME setup COMMAND plain)
add_test(NAME user_a COMMAND plain)
add_test(NAME user_b COMMAND plain)
set_tests_properties(setup PROPERTIES FIXTURES_SETUP shared)
set_tests_properties(user_a user_b PROPERTIES FIXTURES_REQUIRED shared)
add_test(NAME generate COMMAND plain)
add_test(NAME verify COMMAND plain)
set_tests_properties(verify PROPERTIES DEPENDS generate)
add_test(NAME off COMMAND plain)
set_tests_properties(off PROPERTIES DISABLED TRUE)
CMAKE
    printf 'int deep(void) { return 1; }\n' > "$WORKSPACE/deep.c"
    printf 'int deep(void);\nint middle(void) { return deep(); }\n' > "$WORKSPACE/middle.c"
    printf 'int middle(void);\nint main(void) { return middle() == 1 ? 0 : 1; }\n' > "$WORKSPACE/probe.c"
    printf 'int main(void) { return 0; }\n' > "$WORKSPACE/plain.c"
    struo --format json query targets | "$SPEC_PYTHON3" -c 'import json, sys; print(" ".join(sorted(t["id"] for t in json.load(sys.stdin) if t["kind"] == "cmake_test")))' || return
    printf 'deep.c %s\n' "$(struo --format json query affected-tests --changed-path deep.c | "$SPEC_PYTHON3" -c 'import json, sys; print(",".join(t["id"] for t in json.load(sys.stdin)))')" || return
    struo --format json test > /dev/null && echo 'every test passed'
  }

  It 'keeps linked CTest tests together, skips disabled ones, and follows transitive dependencies'
    Skip if 'CMake native toolchain unavailable' cmake_tools_unavailable
    When call group_linked_ctest_tests
    The status should be success
    The line 1 of stdout should equal 'ctest__deep ctest__generate ctest__setup'
    The line 2 of stdout should equal 'deep.c ctest__deep'
    The line 3 of stdout should equal 'every test passed'
  End

  select_tests_through_included_headers() {
    mkdir -p "$WORKSPACE/include/lib" "$WORKSPACE/tests"
    cat > "$WORKSPACE/CMakeLists.txt" <<'CMAKE'
cmake_minimum_required(VERSION 3.20)
project(Headers C)
enable_testing()
add_library(headers INTERFACE)
target_include_directories(headers INTERFACE "${CMAKE_SOURCE_DIR}/include")
add_library(compiled STATIC compiled.c include/lib/value.h include/lib/impl.tcc)
target_link_libraries(compiled PUBLIC headers)
add_executable(inline_check tests/inline_check.c)
target_link_libraries(inline_check PRIVATE headers)
add_executable(plain_check tests/plain_check.c)
add_test(NAME inline COMMAND inline_check)
add_test(NAME plain COMMAND plain_check)
CMAKE
    printf '#include <lib/impl.tcc>\n' > "$WORKSPACE/include/lib/value.h"
    printf 'static inline int value(void) { return 0; }\n' > "$WORKSPACE/include/lib/impl.tcc"
    printf '#include <lib/value.h>\nint main(void) { return value(); }\n' > "$WORKSPACE/tests/inline_check.c"
    printf 'int main(void) { return 0; }\n' > "$WORKSPACE/tests/plain_check.c"
    printf '#include <lib/value.h>\nint compiled(void) { return value(); }\n' > "$WORKSPACE/compiled.c"
    for header in include/lib/value.h include/lib/impl.tcc; do
      struo --format json query affected-tests --changed-path "$header" | "$SPEC_PYTHON3" -c 'import json, sys; print(",".join(t["id"] for t in json.load(sys.stdin)))' || return
    done
  }

  It 'selects a CTest test through the headers on its include path'
    Skip if 'CMake native toolchain unavailable' cmake_tools_unavailable
    When call select_tests_through_included_headers
    The status should be success
    The line 1 of stdout should equal 'ctest__inline'
    The line 2 of stdout should equal 'ctest__inline'
  End

  shared_tree_follows_outside_files() {
    outside="$SPEC_ROOT/outside"
    mkdir -p "$outside" "$WORKSPACE/linked/real"
    ln -s real "$WORKSPACE/linked/link"
    printf 'cmake_minimum_required(VERSION 3.20)\nproject(Outside C)\nenable_testing()\nadd_executable(value value.c)\ntarget_include_directories(value PRIVATE "%s")\nadd_test(NAME value COMMAND value "%s/expected.txt")\n' "$outside" "$outside" > "$WORKSPACE/CMakeLists.txt"
    printf '#include <stdio.h>\n#include <outside_value.h>\nint main(int argc, char **argv) { int expected = 0; FILE *file = fopen(argv[1], "r"); return !file || fscanf(file, "%%d", &expected) != 1 || expected != OUTSIDE_VALUE; }\n' > "$WORKSPACE/value.c"
    for value in 11 29; do
      printf '#define OUTSIDE_VALUE %s\n' "$value" > "$outside/outside_value.h"
      printf '%s\n' "$value" > "$outside/expected.txt"
      struo --format json test > /dev/null || return
      echo "value=$value"
    done
  }

  It 'rebuilds a shared CMake build tree when a file outside the project changes'
    Skip if 'CMake native toolchain unavailable' cmake_tools_unavailable
    When call shared_tree_follows_outside_files
    The status should be success
    The line 1 of stdout should equal 'value=11'
    The line 2 of stdout should equal 'value=29'
  End

  select_tests_reading_copied_data() {
    mkdir "$WORKSPACE/data"
    cat > "$WORKSPACE/CMakeLists.txt" <<'CMAKE'
cmake_minimum_required(VERSION 3.20)
project(Copied C)
enable_testing()
add_custom_target(test_data ALL COMMAND "${CMAKE_COMMAND}" -E copy_directory "${CMAKE_SOURCE_DIR}/data" "${CMAKE_BINARY_DIR}/data" SOURCES data/in.txt)
add_executable(reader reader.c)
add_executable(other other.c)
add_test(NAME reader COMMAND reader data/in.txt)
add_test(NAME other COMMAND other)
CMAKE
    printf 'in\n' > "$WORKSPACE/data/in.txt"
    printf '#include <stdio.h>\nint main(int argc, char **argv) { return fopen(argv[1], "r") ? 0 : 1; }\n' > "$WORKSPACE/reader.c"
    printf 'int main(void) { return 0; }\n' > "$WORKSPACE/other.c"
    struo --format json query affected-tests --changed-path data/in.txt | "$SPEC_PYTHON3" -c 'import json, sys; print(",".join(t["id"] for t in json.load(sys.stdin)))'
  }

  It 'selects CTest tests when data a custom target copies changes'
    Skip if 'CMake native toolchain unavailable' cmake_tools_unavailable
    When call select_tests_reading_copied_data
    The status should be success
    The output should include 'ctest__reader'
  End

  rebuild_only_reached_products() {
    printf '#!/bin/sh\nfor argument in "$@"; do case "$argument" in *.c) basename "$argument" >> "%s/compiled.log";; esac; done\nexec "$@"\n' "$SPEC_ROOT" > "$SPEC_ROOT/launcher"
    chmod +x "$SPEC_ROOT/launcher"
    printf 'cmake_minimum_required(VERSION 3.20)\nproject(Incremental C)\nadd_library(alpha STATIC alpha.c)\nadd_library(beta STATIC beta.c)\n' > "$WORKSPACE/CMakeLists.txt"
    printf 'int alpha(void) { return 1; }\n' > "$WORKSPACE/alpha.c"
    printf 'int beta(void) { return 2; }\n' > "$WORKSPACE/beta.c"
    printf '[[target]]\nname = "cmake"\nkind = "cmake_native_workspace"\nsrcs = ["CMakeLists.txt"]\n[target.attrs]\nconfigure_args = ["-DCMAKE_C_COMPILER_LAUNCHER=%s"]\n' "$SPEC_ROOT/launcher" > "$WORKSPACE/struo.toml"
    fingerprint() { struo --format json query graph-fingerprint | "$SPEC_PYTHON3" -c 'import json, sys; print(next(c["digest"] for c in json.load(sys.stdin)["components"] if c["label"] == "alpha"))'; }
    struo --format json build alpha > /dev/null && struo --format json build beta > /dev/null || return
    before="$(fingerprint)" || return
    : > "$SPEC_ROOT/compiled.log"
    struo --format json build alpha > /dev/null && struo --format json build beta > /dev/null || return
    echo "unchanged: $(sort "$SPEC_ROOT/compiled.log" | tr '\n' ' ')"
    printf 'int beta(void) { return 3; }\n' > "$WORKSPACE/beta.c"
    struo --format json build alpha > /dev/null && struo --format json build beta > /dev/null || return
    echo "beta edited: $(sort "$SPEC_ROOT/compiled.log" | tr '\n' ' ')"
    [ "$before" = "$(fingerprint)" ] && echo 'alpha identity unchanged'
  }

  It 'rebuilds only the discovered products a change reaches'
    Skip if 'CMake native toolchain unavailable' cmake_tools_unavailable
    When call rebuild_only_reached_products
    The status should be success
    The line 1 of stdout should equal 'unchanged: '
    The line 2 of stdout should equal 'beta edited: beta.c '
    The line 3 of stdout should equal 'alpha identity unchanged'
  End

  always_run_target_data_reaches_tests() {
    mkdir "$WORKSPACE/data"
    cat > "$WORKSPACE/CMakeLists.txt" <<'CMAKE'
cmake_minimum_required(VERSION 3.20)
project(Always C)
enable_testing()
add_custom_target(test_data ALL COMMAND "${CMAKE_COMMAND}" -E copy_directory "${CMAKE_SOURCE_DIR}/data" "${CMAKE_BINARY_DIR}/data")
add_executable(reader reader.c)
add_test(NAME reader COMMAND reader "${CMAKE_BINARY_DIR}/data/value.txt" "${CMAKE_SOURCE_DIR}/expected.txt")
CMAKE
    printf '#include <stdio.h>\nint main(int argc, char **argv) { int a = 0, b = 1; FILE *x = fopen(argv[1], "r"), *y = fopen(argv[2], "r"); return !x || !y || fscanf(x, "%%d", &a) != 1 || fscanf(y, "%%d", &b) != 1 || a != b; }\n' > "$WORKSPACE/reader.c"
    for value in 11 29; do
      printf '%s\n' "$value" > "$WORKSPACE/data/value.txt"
      printf '%s\n' "$value" > "$WORKSPACE/expected.txt"
      struo --format json test > /dev/null || return
      echo "value=$value"
    done
  }

  It 'rebuilds a shared CMake build tree when data an always-run target copies changes'
    Skip if 'CMake native toolchain unavailable' cmake_tools_unavailable
    When call always_run_target_data_reaches_tests
    The status should be success
    The line 1 of stdout should equal 'value=11'
    The line 2 of stdout should equal 'value=29'
  End

  configure_read_file_reaches_tests() {
    cat > "$WORKSPACE/CMakeLists.txt" <<'CMAKE'
cmake_minimum_required(VERSION 3.20)
project(ConfigureRead C)
enable_testing()
file(READ "${CMAKE_SOURCE_DIR}/version.txt" BUILD_VALUE)
string(STRIP "${BUILD_VALUE}" BUILD_VALUE)
add_executable(value value.c)
target_compile_definitions(value PRIVATE BUILD_VALUE=${BUILD_VALUE})
add_test(NAME value COMMAND value "${CMAKE_SOURCE_DIR}/expected.txt")
CMAKE
    printf '#include <stdio.h>\nint main(int argc, char **argv) { int expected = 0; FILE *file = fopen(argv[1], "r"); return !file || fscanf(file, "%%d", &expected) != 1 || expected != BUILD_VALUE; }\n' > "$WORKSPACE/value.c"
    for value in 11 29; do
      printf '%s\n' "$value" > "$WORKSPACE/version.txt"
      printf '%s\n' "$value" > "$WORKSPACE/expected.txt"
      struo --format json test > /dev/null || return
      echo "value=$value"
    done
  }

  It 'rebuilds a shared CMake build tree when a file configure reads changes'
    Skip if 'CMake native toolchain unavailable' cmake_tools_unavailable
    When call configure_read_file_reaches_tests
    The status should be success
    The line 1 of stdout should equal 'value=11'
    The line 2 of stdout should equal 'value=29'
  End

  project_iterating_its_targets_builds() {
    printf 'cmake_minimum_required(VERSION 3.20)\nproject(Iterate C)\nadd_library(lib STATIC lib.c)\nget_property(all DIRECTORY PROPERTY BUILDSYSTEM_TARGETS)\nforeach(t IN LISTS all)\n  target_compile_options(${t} PRIVATE -Wall)\nendforeach()\n' > "$WORKSPACE/CMakeLists.txt"
    printf 'int lib(void) { return 1; }\n' > "$WORKSPACE/lib.c"
    struo --format json build lib > /dev/null && echo 'iterating project builds'
  }

  It 'adds no target to a project that iterates its own targets'
    Skip if 'CMake native toolchain unavailable' cmake_tools_unavailable
    When call project_iterating_its_targets_builds
    The status should be success
    The stdout should include 'iterating project builds'
  End

  select_tests_through_sibling_headers() {
    cat > "$WORKSPACE/CMakeLists.txt" <<'CMAKE'
cmake_minimum_required(VERSION 3.20)
project(Sibling C)
enable_testing()
add_library(util STATIC util.c shared.h)
add_library(core STATIC core.c)
add_executable(t t.c)
target_link_libraries(t PRIVATE core)
add_executable(u u.c)
target_link_libraries(u PRIVATE util)
add_test(NAME t COMMAND t)
add_test(NAME u COMMAND u)
CMAKE
    printf '#define SHARED 0\n' > "$WORKSPACE/shared.h"
    printf '#include "shared.h"\nint util(void) { return SHARED; }\n' > "$WORKSPACE/util.c"
    printf '#include "shared.h"\nint core(void) { return SHARED; }\n' > "$WORKSPACE/core.c"
    printf 'int core(void);\nint main(void) { return core(); }\n' > "$WORKSPACE/t.c"
    printf 'int util(void);\nint main(void) { return util(); }\n' > "$WORKSPACE/u.c"
    struo --format json query affected-tests --changed-path shared.h | "$SPEC_PYTHON3" -c 'import json, sys; print(",".join(sorted(t["id"] for t in json.load(sys.stdin))))'
  }

  It 'selects a CTest test through a header beside a source it compiles'
    Skip if 'CMake native toolchain unavailable' cmake_tools_unavailable
    When call select_tests_through_sibling_headers
    The status should be success
    The output should equal 'ctest__t,ctest__u'
  End

  stage_discovered_soname() {
    printf 'cmake_minimum_required(VERSION 3.20)\nproject(Soname C)\nadd_library(versioned SHARED versioned.c)\nset_target_properties(versioned PROPERTIES VERSION 1.2.3 SOVERSION 1)\n' > "$WORKSPACE/CMakeLists.txt"
    printf 'int versioned(void) { return 1; }\n' > "$WORKSPACE/versioned.c"
    struo --format json build versioned > /dev/null || return
    products="$WORKSPACE/.struo/out/versioned/products"
    [ -f "$products/libversioned.so" ] && [ ! -L "$products/libversioned.so.1" ] && cmp "$products/libversioned.so" "$products/libversioned.so.1" && echo 'soname staged'
  }

  It 'stages the runtime name of a discovered versioned shared library'
    Skip if 'CMake native toolchain unavailable' cmake_tools_unavailable
    Skip if 'shared library naming is Linux-specific' [ "$(uname -s)" != Linux ]
    When call stage_discovered_soname
    The status should be success
    The stdout should include 'soname staged'
  End

  external_project_products_at_any_checkout_path() {
    mkdir "$WORKSPACE/ext"
    cat > "$WORKSPACE/CMakeLists.txt" <<'CMAKE'
cmake_minimum_required(VERSION 3.20)
project(Outer C ASM)
include(ExternalProject)
ExternalProject_Add(ext SOURCE_DIR "${CMAKE_SOURCE_DIR}/ext" CMAKE_ARGS -DCMAKE_BUILD_TYPE=Debug INSTALL_COMMAND "" BUILD_BYPRODUCTS "<BINARY_DIR>/libext.a")
add_library(outer STATIC outer.c entry.S)
cmake_language(DEFER CALL add_library late STATIC late.S)
CMAKE
    printf 'cmake_minimum_required(VERSION 3.20)\nproject(Ext C)\nadd_library(ext STATIC ext.c)\n' > "$WORKSPACE/ext/CMakeLists.txt"
    printf 'const char *ext(void) { return __FILE__; }\n' > "$WORKSPACE/ext/ext.c"
    printf 'int outer(void) { return 1; }\n' > "$WORKSPACE/outer.c"
    printf '.globl entry\nentry:\n  ret\n' > "$WORKSPACE/entry.S"
    printf '.globl late\nlate:\n  ret\n' > "$WORKSPACE/late.S"
    printf '[[target]]\nname = "Outer"\nkind = "cmake_project"\nsrcs = ["CMakeLists.txt", "*.c", "*.S", "ext/*"]\n[target.attrs]\nproducts = ["libouter.a", "liblate.a", "ext-prefix/src/ext-build/libext.a"]\nenv = { CFLAGS = "" }\n' > "$WORKSPACE/struo.toml"
    moved="$SPEC_ROOT/elsewhere/nested/workspace"
    mkdir -p "$SPEC_ROOT/elsewhere/nested"
    cp -R "$WORKSPACE" "$moved"
    struo --format json build Outer > /dev/null || return
    "$STRUO_BIN" -C "$moved" --format json build Outer > /dev/null || return
    for product in libouter.a liblate.a ext-prefix/src/ext-build/libext.a; do
      cmp "$WORKSPACE/.struo/out/Outer/products/$product" "$moved/.struo/out/Outer/products/$product" || return
    done
    echo 'identical external and assembly products'
  }

  It 'builds identical ExternalProject and assembly products at any checkout path'
    Skip if 'CMake native toolchain unavailable' cmake_tools_unavailable
    When call external_project_products_at_any_checkout_path
    The status should be success
    The stdout should include 'identical external and assembly products'
  End

  rebuild_changed_cmake_toolchain() {
    kind="$1"
    selection="$2"
    write_ctest_workspace
    real_cc="$(command -v cc)"
    mkdir "$WORKSPACE/sdk"
    cat > "$WORKSPACE/test.c" <<'C'
#include <stdio.h>
int main(void) { printf("%d\n", BUILD_VALUE); return 0; }
C
    if [ "$selection" = compiler ]; then
      configure="-DCMAKE_C_COMPILER:FILEPATH=$WORKSPACE/sdk/compiler"
    else
      configure="-DCMAKE_TOOLCHAIN_FILE:FILEPATH=$WORKSPACE/sdk/toolchain.cmake"
      printf '#!/bin/sh\nexec "%s" "$@"\n' "$real_cc" > "$WORKSPACE/sdk/compiler"
      chmod +x "$WORKSPACE/sdk/compiler"
    fi
    cat > "$WORKSPACE/struo.toml" <<TOML
[[target]]
name = "Tests"
kind = "$kind"
srcs = ["CMakeLists.txt", "test.c"]
[target.attrs]
configure_args = ["$configure"]
TOML
    if [ "$kind" = cmake_project ]; then
      printf '%s\n' 'products = ["check_sum"]' >> "$WORKSPACE/struo.toml"
      binary="$WORKSPACE/.struo/out/Tests/products/check_sum"
    else
      binary="$WORKSPACE/.struo/out/Tests/cmake-build/check_sum"
    fi
    for value in 11 29; do
      if [ "$selection" = compiler ]; then
        printf '#!/bin/sh\nexec "%s" -DBUILD_VALUE=%s "$@"\n' "$real_cc" "$value" > "$WORKSPACE/sdk/compiler"
        chmod +x "$WORKSPACE/sdk/compiler"
      else
        printf 'set(CMAKE_C_COMPILER "%s/sdk/compiler")\nset(CMAKE_C_FLAGS_INIT "-DBUILD_VALUE=%s")\n' "$WORKSPACE" "$value" > "$WORKSPACE/sdk/toolchain.cmake"
      fi
      struo --format json build Tests > "$WORKSPACE/build-$value.json" || return
      "$binary" > "$WORKSPACE/value-$value.txt" || return
    done
    first="$(cat "$WORKSPACE/value-11.txt")"
    second="$(cat "$WORKSPACE/value-29.txt")"
    printf '%s %s: %s -> %s\n' "$kind" "$selection" "$first" "$second"
    [ "$first" = 11 ] && [ "$second" = 29 ]
  }

  Describe 'selected native toolchain changes'
    Parameters
      cmake_test compiler
      cmake_test toolchain
      cmake_project compiler
      cmake_project toolchain
    End

    It "rebuilds $1 when an external $2 changes at the same path"
      Skip if 'CMake native toolchain unavailable' cmake_tools_unavailable
      When call rebuild_changed_cmake_toolchain "$1" "$2"
      The status should be success
      The stdout should include '11 -> 29'
    End
  End

End
