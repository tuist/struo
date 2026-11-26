#shellcheck shell=bash

Describe 'real-world native graph shapes'
  BeforeEach 'setup_workspace'
  AfterEach 'cleanup_workspace'

  native_graph_toolchain_unavailable() {
    [ "$(uname -s)" != Darwin ] || ! xcrun --find swiftc >/dev/null 2>&1
  }

  graph_struo() {
    STRUO_CHANGE_TRACKER=0 RUST_LOG=off struo --memory-limit 2GiB "$@" --format json
  }

  graph_snapshot() {
    graph_struo query 'MATCH (t:Target) RETURN t.id, t.kind, t.attrs, t.deps, t.srcs' > "$WORKSPACE/query.json" || return
    jq '.rows | map({key: .[0], value: {kind: .[1], attrs: .[2], deps: .[3], srcs: .[4]}}) | from_entries' "$WORKSPACE/query.json" > "$WORKSPACE/graph.json" || return
    jq -e 'keys as $ids | all(.[]; all(.deps[]; . as $dep | $ids | index($dep) != null))' "$WORKSPACE/graph.json" >/dev/null
  }

  graph_build() {
    graph_struo build "$1" > "$WORKSPACE/build.json" 2> "$WORKSPACE/build.err" || {
      cat "$WORKSPACE/build.err"
      return 1
    }
    if grep -E 'Internal Error:|DecodingError\.dataCorrupted|Corrupted JSON' "$WORKSPACE/build.err"; then
      return 1
    fi
  }

  graph_validate() {
    graph_struo query validate-workspace > "$WORKSPACE/validation.json" || return
    jq -e '.valid and (.diagnostics | length == 0)' "$WORKSPACE/validation.json" >/dev/null || {
      cat "$WORKSPACE/validation.json"
      return 1
    }
  }

  graph_check_action_hits() {
    graph_struo query evidence > "$WORKSPACE/evidence-warm.json" || return
    jq -e --slurpfile cold "$WORKSPACE/evidence-cold.json" '
      ($cold[0] | map(.id)) as $old |
      [.[] | select(.id as $id | $old | index($id) == null)] as $new |
      ($new | length) == ($cold[0] | length) and
      ($new | all(.cache == "hit")) and
      ([$new[] | {action_digest, input_digest, outputs}] | sort_by(.action_digest)) ==
      ([$cold[0][] | {action_digest, input_digest, outputs}] | sort_by(.action_digest))
    ' "$WORKSPACE/evidence-warm.json" >/dev/null
  }

  exercise_warm_package_graph() {
    local STRUO_ACTUAL_SWIFT STRUO_MANIFEST_PROBES actual_swift
    cp -R "$REPO_ROOT/crates/struo-frontend/prelude/examples/xcode-generated-source-e2e/." "$WORKSPACE/"
    mv "$WORKSPACE/Packages/Shared/Sources" "$WORKSPACE/Packages/Shared/Source"
    mkdir -p "$SPEC_ROOT/tools"
    actual_swift="$(xcrun --find swift)" || return
    cat > "$SPEC_ROOT/tools/swift" <<'SH'
#!/bin/sh
case " $* " in
  *' dump-package '*) printf 'dump\n' >> "$STRUO_MANIFEST_PROBES" ;;
  *' describe '*) printf 'describe\n' >> "$STRUO_MANIFEST_PROBES" ;;
esac
exec "$STRUO_ACTUAL_SWIFT" "$@"
SH
    cat > "$SPEC_ROOT/tools/xcrun" <<'SH'
#!/bin/sh
if [ "$#" = 2 ] && [ "$1" = --find ] && [ "$2" = swift ]; then
  printf '%s/swift\n' "${0%/*}"
else
  exec /usr/bin/xcrun "$@"
fi
SH
    chmod +x "$SPEC_ROOT/tools/swift" "$SPEC_ROOT/tools/xcrun"
    export STRUO_ACTUAL_SWIFT="$actual_swift" STRUO_MANIFEST_PROBES="$SPEC_ROOT/manifest-probes"
    PATH="$SPEC_ROOT/tools:$PATH" graph_snapshot || return
    cp "$WORKSPACE/graph.json" "$SPEC_ROOT/cold-graph.json"
    [ -s "$STRUO_MANIFEST_PROBES" ] || return 1
    [ "$(grep -c '^describe$' "$STRUO_MANIFEST_PROBES")" -eq 1 ] || return 1
    jq -e 'any(.[]; .attrs.module_name == "Shared" and .srcs == ["Packages/Shared/Source/Shared/Shared.swift"])' "$WORKSPACE/graph.json" >/dev/null || return
    cp "$STRUO_MANIFEST_PROBES" "$SPEC_ROOT/cold-probes"
    PATH="$SPEC_ROOT/tools:$PATH" graph_snapshot || return
    cmp -s "$WORKSPACE/graph.json" "$SPEC_ROOT/cold-graph.json" || return
    cmp -s "$STRUO_MANIFEST_PROBES" "$SPEC_ROOT/cold-probes" || {
      diff -u "$SPEC_ROOT/cold-probes" "$STRUO_MANIFEST_PROBES"
      return 1
    }

    mkdir -p "$WORKSPACE/Packages/Shared/Source/Added"
    printf 'public let added = 1\n' > "$WORKSPACE/Packages/Shared/Source/Added/Added.swift"
    perl -pi -e 's/targets: \[.target\(name: "Shared"\)\]/targets: [.target(name: "Shared"), .target(name: "Added")]/' "$WORKSPACE/Packages/Shared/Package.swift"
    PATH="$SPEC_ROOT/tools:$PATH" graph_snapshot || return
    jq -e 'to_entries | any(.key | endswith("_Added"))' "$WORKSPACE/graph.json" >/dev/null || return
    [ "$(grep -c '^dump$' "$STRUO_MANIFEST_PROBES")" -eq 2 ] || return 1
    [ "$(grep -c '^describe$' "$STRUO_MANIFEST_PROBES")" -eq 2 ]
  }

  exercise_package_path_alias() {
    cp -R "$REPO_ROOT/crates/struo-frontend/prelude/examples/xcode-generated-source-e2e/." "$WORKSPACE/"
    [ -d "$WORKSPACE/Packages/sHaReD" ] || ln -s Shared "$WORKSPACE/Packages/sHaReD"
    mkdir -p "$WORKSPACE/Packages/Consumer/Sources/Consumer"
    cat > "$WORKSPACE/Packages/Consumer/Package.swift" <<'SWIFT'
// swift-tools-version: 5.9
import PackageDescription
let package = Package(
    name: "Consumer",
    products: [.library(name: "Consumer", targets: ["Consumer"])],
    dependencies: [.package(path: "../sHaReD")],
    targets: [.target(name: "Consumer", dependencies: [.product(name: "Shared", package: "shared")])]
)
SWIFT
    printf 'import Shared\npublic let consumed = Shared.prefix\n' > "$WORKSPACE/Packages/Consumer/Sources/Consumer/Consumer.swift"
    perl -pi -e '
      s/packageReferences = \(SHARED_PACKAGE\)/packageReferences = (SHARED_PACKAGE, CONSUMER_PACKAGE)/;
      s/packageProductDependencies = \(SHARED_PRODUCT\)/packageProductDependencies = (SHARED_PRODUCT, CONSUMER_PRODUCT)/;
      s/SHARED_PACKAGE = \{/CONSUMER_PACKAGE = {isa = XCLocalSwiftPackageReference; relativePath = Packages\/Consumer; }; CONSUMER_PRODUCT = {isa = XCSwiftPackageProductDependency; package = CONSUMER_PACKAGE; productName = Consumer; }; SHARED_PACKAGE = {/;
    ' "$WORKSPACE/Generated.xcodeproj/project.pbxproj"
    graph_snapshot || return
    jq -e '
      [to_entries[] | select(.key | endswith("_MacroHost") | not) | .value] |
      (map(select(.attrs.module_name == "Shared")) | length) == 1 and
      (map(select(.attrs.module_name == "Consumer")) | length) == 1
    ' "$WORKSPACE/graph.json" >/dev/null
  }

  exercise_artifact_bundle_binary_target() {
    local bundle triple="$(uname -m)-apple-macosx"
    bundle="$WORKSPACE/CHegel.artifactbundle"
    mkdir -p "$bundle/include" "$bundle/$triple" "$bundle/x86_64-unknown-linux-gnu" "$WORKSPACE/Sources/Probe"
    printf 'int hegel(void) { return 41; }\n' > "$SPEC_ROOT/hegel.c"
    xcrun clang -c "$SPEC_ROOT/hegel.c" -o "$SPEC_ROOT/hegel.o" || return
    xcrun libtool -static -o "$bundle/$triple/libhegel_c.a" "$SPEC_ROOT/hegel.o" || return
    printf 'not an archive\n' > "$bundle/x86_64-unknown-linux-gnu/libhegel_c.a"
    printf 'int hegel(void);\n' > "$bundle/include/hegel.h"
    printf 'module CHegel {\n  header "hegel.h"\n  export *\n}\n' > "$bundle/include/module.modulemap"
    cat > "$bundle/info.json" <<JSON
{"schemaVersion": "1.0", "artifacts": {"CHegel": {"type": "staticLibrary", "version": "0.41.0", "variants": [
  {"path": "x86_64-unknown-linux-gnu/libhegel_c.a", "supportedTriples": ["x86_64-unknown-linux-gnu"], "staticLibraryMetadata": {"headerPaths": ["include"], "moduleMapPath": "include/module.modulemap"}},
  {"path": "$triple/libhegel_c.a", "supportedTriples": ["$triple"], "staticLibraryMetadata": {"headerPaths": ["include"], "moduleMapPath": "include/module.modulemap"}}
]}}}
JSON
    cat > "$WORKSPACE/Package.swift" <<'SWIFT'
// swift-tools-version: 5.9
import PackageDescription
let package = Package(
    name: "ArtifactBundles",
    products: [.executable(name: "Probe", targets: ["Probe"])],
    targets: [
        .binaryTarget(name: "CHegel", path: "CHegel.artifactbundle"),
        .executableTarget(name: "Probe", dependencies: ["CHegel"]),
    ]
)
SWIFT
    printf 'import CHegel\nprint(hegel() + 1)\n' > "$WORKSPACE/Sources/Probe/main.swift"
    graph_build SwiftPackage_ArtifactBundles_Probe || return
    [ "$("$WORKSPACE/.struo/out/SwiftPackage_ArtifactBundles_Probe/Probe.app/Contents/MacOS/Probe")" = 42 ]
  }

  exercise_sibling_path_package() {
    mkdir -p "$WORKSPACE/PackageA/Sources/PackageA" "$WORKSPACE/PackageA/Sources/PackageAMacros" "$WORKSPACE/PackageB/Sources/PackageBSupport" "$WORKSPACE/PackageB/Sources/PackageBMacroSupport"
    cat > "$WORKSPACE/PackageA/Package.swift" <<'SWIFT'
// swift-tools-version: 5.9
import CompilerPluginSupport
import PackageDescription
let package = Package(
    name: "PackageA",
    products: [.library(name: "PackageA", targets: ["PackageA"])],
    dependencies: [.package(path: "../PackageB")],
    targets: [
        .target(name: "PackageA", dependencies: [.product(name: "PackageBSupport", package: "PackageB")]),
        .macro(name: "PackageAMacros", dependencies: [.product(name: "PackageBMacroSupport", package: "PackageB")]),
    ]
)
SWIFT
    cat > "$WORKSPACE/PackageB/Package.swift" <<'SWIFT'
// swift-tools-version: 5.9
import PackageDescription
let package = Package(
    name: "PackageB",
    products: [
        .library(name: "PackageBSupport", targets: ["PackageBSupport"]),
        .library(name: "PackageBMacroSupport", targets: ["PackageBMacroSupport"]),
    ],
    targets: [.target(name: "PackageBSupport"), .target(name: "PackageBMacroSupport")]
)
SWIFT
    printf 'import PackageBSupport\n' > "$WORKSPACE/PackageA/Sources/PackageA/PackageA.swift"
    printf 'import PackageBMacroSupport\n' > "$WORKSPACE/PackageA/Sources/PackageAMacros/Macros.swift"
    printf 'public let support = 1\n' > "$WORKSPACE/PackageB/Sources/PackageBSupport/Support.swift"
    printf 'public let macroSupport = 1\n' > "$WORKSPACE/PackageB/Sources/PackageBMacroSupport/MacroSupport.swift"

    if STRUO_CHANGE_TRACKER=0 RUST_LOG=off "$STRUO_BIN" -C "$WORKSPACE/PackageA" --format json query targets > /dev/null 2> "$WORKSPACE/outside.err"; then
      echo 'a sibling package outside the workspace loaded as an incomplete graph'
      return 1
    fi
    grep -q 'swift_package_path_outside_workspace' "$WORKSPACE/outside.err" || { cat "$WORKSPACE/outside.err"; return 1; }
    grep -qF "struo -C $(cd "$WORKSPACE" && pwd -P)" "$WORKSPACE/outside.err" || { cat "$WORKSPACE/outside.err"; return 1; }

    graph_snapshot || return
    graph_validate || return
    jq -e '
      (.["PackageA/SwiftPackage_PackageA_PackageA"].deps | index("PackageB/SwiftPackage_PackageB_PackageBSupport")) and
      (.["PackageA/SwiftPackage_PackageA_PackageAMacros"].deps | index("PackageB/SwiftPackage_PackageB_PackageBMacroSupport_MacroHost")) and
      (keys | all(ascii_downcase | startswith("packagea/swiftpackage_packageb") | not))
    ' "$WORKSPACE/graph.json" >/dev/null || { jq 'with_entries(select(.key | startswith("PackageA/"))) | map_values(.deps)' "$WORKSPACE/graph.json"; return 1; }
    graph_build PackageA/SwiftPackage_PackageA_PackageA
  }

  exercise_vapor_graph() {
    cp "$REPO_ROOT/fixtures/native_graphs/vapor/Package.swift" "$WORKSPACE/"
    cp -R "$REPO_ROOT/fixtures/native_graphs/vapor/Sources" "$REPO_ROOT/fixtures/native_graphs/vapor/Tests" "$REPO_ROOT/fixtures/native_graphs/vapor/Dependencies" "$WORKSPACE/"
    graph_snapshot || return
    jq -e -f "$REPO_ROOT/spec/support/native_graphs/vapor.jq" "$WORKSPACE/graph.json" >/dev/null || return
    [ ! -e "$WORKSPACE/Package.resolved" ] || return 1
    graph_validate || return

    graph_build SwiftPackage_VaporGraph_Development || return
    graph_struo query evidence > "$WORKSPACE/evidence-cold.json" || return
    [ "$("$WORKSPACE/.struo/out/SwiftPackage_VaporGraph_Development/Development.app/Contents/MacOS/Development")" = 16 ] || return 1
    [ ! -e "$WORKSPACE/.struo/out/SwiftPackage_VaporGraph_VaporTests" ] || return 1
    [ ! -e "$WORKSPACE/.struo/out/SwiftPackage_VaporGraph_XCTVapor" ] || return 1
    [ ! -e "$WORKSPACE/.struo/out/SwiftPackage_swift-nio_NIOTransportServices" ] || return 1
    graph_build SwiftPackage_VaporGraph_Development || return
    jq -e '.cache == "hit"' "$WORKSPACE/build.json" >/dev/null || return
    graph_check_action_hits || return

    mv "$WORKSPACE/.struo/out" "$WORKSPACE/saved-outputs"
    graph_build SwiftPackage_VaporGraph_Development || return
    jq -e '.cache == "hit"' "$WORKSPACE/build.json" >/dev/null || return
    [ "$("$WORKSPACE/.struo/out/SwiftPackage_VaporGraph_Development/Development.app/Contents/MacOS/Development")" = 16 ] || return 1

    perl -pi -e 's/GRAPH_VALUE 7/GRAPH_VALUE 9/' "$WORKSPACE/Sources/CVaporBcrypt/include/CVaporBcrypt.h"
    graph_build SwiftPackage_VaporGraph_Development || return
    jq -e '.cache == "miss"' "$WORKSPACE/build.json" >/dev/null || return
    [ "$("$WORKSPACE/.struo/out/SwiftPackage_VaporGraph_Development/Development.app/Contents/MacOS/Development")" = 18 ] || return 1

    perl -pi -e 's/logValue = 2/logValue = 3/' "$WORKSPACE/Dependencies/swift-log/Sources/Logging/Logging.swift"
    graph_build SwiftPackage_VaporGraph_Development || return
    jq -e '.cache == "miss"' "$WORKSPACE/build.json" >/dev/null || return
    [ "$("$WORKSPACE/.struo/out/SwiftPackage_VaporGraph_Development/Development.app/Contents/MacOS/Development")" = 21 ] || return 1
  }

  exercise_netnewswire_graph() {
    cp -R "$REPO_ROOT/fixtures/native_graphs/netnewswire/NetNewsWire.xcodeproj" "$REPO_ROOT/fixtures/native_graphs/netnewswire/Config" "$REPO_ROOT/fixtures/native_graphs/netnewswire/Sources" "$REPO_ROOT/fixtures/native_graphs/netnewswire/Extensions" "$REPO_ROOT/fixtures/native_graphs/netnewswire/Tests" "$REPO_ROOT/fixtures/native_graphs/netnewswire/Modules" "$WORKSPACE/"
    cp -R "$REPO_ROOT/fixtures/native_graphs/netnewswire/Vendor" "$WORKSPACE/"
    sh "$WORKSPACE/Vendor/build.sh" || return
    graph_snapshot || return
    jq -e -f "$REPO_ROOT/spec/support/native_graphs/netnewswire.jq" "$WORKSPACE/graph.json" >/dev/null || return
    graph_validate || return

    graph_build NetNewsWire || return
    graph_struo query evidence > "$WORKSPACE/evidence-cold.json" || return
    [ "$("$WORKSPACE/.struo/out/NetNewsWire/NetNewsWire.app/Contents/MacOS/NetNewsWire")" = 11 ] || return 1
    [ ! -e "$WORKSPACE/.struo/out/NetNewsWire-iOS" ] || return 1
    [ ! -e "$WORKSPACE/.struo/out/XcodePackage_xcode_ios_Account_Account" ] || return 1
    [ ! -e "$WORKSPACE/.struo/out/NetNewsWireTests" ] || return 1
    graph_build NetNewsWire || return
    jq -e '.cache == "hit"' "$WORKSPACE/build.json" >/dev/null || return
    graph_check_action_hits || return

    perl -pi -e 's/CORE_VALUE 5/CORE_VALUE 8/' "$WORKSPACE/Modules/RSCore/Sources/RSCoreObjC/include/RSCoreObjC.h"
    graph_build NetNewsWire || return
    jq -e '.cache == "miss"' "$WORKSPACE/build.json" >/dev/null || return
    [ "$("$WORKSPACE/.struo/out/NetNewsWire/NetNewsWire.app/Contents/MacOS/NetNewsWire")" = 17 ] || return 1

    perl -pi -e 's/GRAPH_MAC/GRAPH_CHANGED/' "$WORKSPACE/Config/macOS.xcconfig"
    graph_snapshot || return
    jq -e '.NetNewsWire.attrs.swift_flags | index("GRAPH_CHANGED") != null and index("GRAPH_MAC") == null' "$WORKSPACE/graph.json" >/dev/null || return
    perl -pi -e 's/fileRef = MAIN_FILE/fileRef = UNUSED_FILE/' "$WORKSPACE/NetNewsWire.xcodeproj/project.pbxproj"
    graph_snapshot || return
    jq -e '.NetNewsWire.srcs == ["Sources/NotAMember.swift"] and ."NetNewsWire-iOS".srcs == ["Sources/NotAMember.swift"]' "$WORKSPACE/graph.json" >/dev/null || return
    graph_validate
  }

  exercise_xcode_build_tools() {
    cp -R "$REPO_ROOT/fixtures/xcode_workspace_compatibility/." "$WORKSPACE/"
    origin="$SPEC_ROOT/GeneratorSupport"
    mv "$WORKSPACE/App/Packages/GeneratorSupport" "$origin"
    git -C "$origin" init -q || return
    git -C "$origin" config user.name Struo || return
    git -C "$origin" config user.email struo@example.invalid || return
    git -C "$origin" add . || return
    git -C "$origin" commit -qm pinned || return
    revision="$(git -C "$origin" rev-parse HEAD)"
    git -C "$origin" tag 1.0.0 || return
    perl -pi -e 's/Value = 40/Value = 90/' "$origin/Sources/GeneratorSupport/Value.swift"
    git -C "$origin" commit -qam newer || return
    git -C "$origin" tag 1.1.0 || return
    sed "s|path: \"../GeneratorSupport\"|url: \"file://$origin\", from: \"1.0.0\"|" "$WORKSPACE/App/Packages/Generated/Package.swift" > "$WORKSPACE/manifest.tmp"
    mv "$WORKSPACE/manifest.tmp" "$WORKSPACE/App/Packages/Generated/Package.swift"
    mkdir -p "$WORKSPACE/App/Fetch.xcworkspace/xcshareddata/swiftpm"
    jq -n --arg revision "$revision" --arg url "file://$origin" '{version: 2, pins: [{identity: "generatorsupport", kind: "remoteSourceControl", location: $url, state: {revision: $revision, version: "1.0.0"}}]}' > "$WORKSPACE/App/Fetch.xcworkspace/xcshareddata/swiftpm/Package.resolved"
    graph_snapshot 2> "$WORKSPACE/query.err" || return
    jq -e '[.[] | select(.kind == "swift_package_build_tools") | .attrs.build_role] | sort == ["destination", "destination", "tools", "tools"]' "$WORKSPACE/graph.json" >/dev/null || return
    [ ! -d "$WORKSPACE/.struo/out" ] || return 1
    [ "$(git -C "$WORKSPACE/.struo/xcode-packages/generatorsupport" rev-parse HEAD)" = "$revision" ] || return 1
    graph_validate 2> "$WORKSPACE/validation.err" || return

    STRUO_CHANGE_TRACKER=0 struo --memory-limit 2GiB xcodebuild -- -workspace App/Fetch.xcworkspace -scheme Fetch -configuration Development -quiet build > "$WORKSPACE/build.out" 2> "$WORKSPACE/build.err" || {
      cat "$WORKSPACE/build.out" "$WORKSPACE/build.err"
      return 1
    }
    binary="$WORKSPACE/.struo/out/App/Fetch/Fetch.app/Contents/MacOS/Fetch"
    [ "$("$binary")" = '42:payload' ] || return 1
    [ "$(find "$WORKSPACE/.struo/out" -name lint.txt -exec cat {} \;)" = validated ] || return 1
    graph_build App/Fetch || return
    find "$WORKSPACE/.struo/out" -name lint.txt -delete
    graph_build App/Fetch || return
    [ "$(find "$WORKSPACE/.struo/out" -name lint.txt -exec cat {} \;)" = validated ] || return 1
    graph_struo test App/FetchTests > "$WORKSPACE/test.json" 2> "$WORKSPACE/test.err" || {
      cat "$WORKSPACE/test.err"
      return 1
    }
    jq -e '.summary.total == 1 and .summary.passed == 1' "$WORKSPACE/.struo/out/App/FetchTests/test/test_results.json" >/dev/null || return

    host_probe="$(jq -r 'to_entries[] | select(.value.kind == "swift_macro") | .key' "$WORKSPACE/graph.json")"
    graph_build "$host_probe" || return
    "$WORKSPACE/.struo/out/$host_probe/HostProbe-tool" || return
    macro_test="$(jq -r 'to_entries[] | select(.value.kind == "apple_test_bundle" and .value.attrs.product_name == "HostProbeTests") | .key' "$WORKSPACE/graph.json")"
    graph_struo test "$macro_test" > "$WORKSPACE/macro-test.json" 2> "$WORKSPACE/macro-test.err" || { cat "$WORKSPACE/macro-test.err"; return 1; }
    jq -e '.summary.total == 1 and .summary.passed == 1' "$WORKSPACE/.struo/out/$macro_test/test/test_results.json" >/dev/null || return
    STRUO_GENERATED_CXX=1 graph_build App/Fetch || return
    [ "$("$binary")" = '42:payload' ] || return 1
    if STRUO_GENERATED_WARNING=1 graph_struo build App/Fetch > "$WORKSPACE/generated-warning.json" 2> "$WORKSPACE/generated-warning.err"; then
      return 1
    fi
    grep -q 'error:.*Generated warning must be fatal' "$WORKSPACE/generated-warning.err" || return
    STRUO_HEADER_VALUE=8 graph_build App/Fetch || return
    [ "$("$binary")" = '43:payload' ] || return 1
    graph_build App/Fetch || return
    [ "$("$binary")" = '42:payload' ] || return 1

    jq -r '.[] | select(.kind == "swift_package_build_tools" and .attrs.target_name == "Generated" and .attrs.build_role == "destination") | .attrs |
      "\n[[target]]\nname = \"NoTraits\"\nkind = \"swift_package_build_tools\"\n[target.attrs]\npackage_path = \(.package_path | @json)\ntarget_name = \"Generated\"\ndeclares_traits = true\n[target.attrs.dependencies]\n" +
      (.dependencies | to_entries | map("\(.key | @json) = \(.value | @json)") | join("\n"))' "$WORKSPACE/graph.json" >> "$WORKSPACE/struo.toml"
    graph_build NoTraits || return
    grep -q 'buildToolValue = 140' "$(find "$WORKSPACE/.struo/out/NoTraits" -path '*/Generated/destination/*/Build.swift')" || return

    sed '/.macro(name: "HostProbe"/d; /.testTarget(name: "HostProbeTests"/d' "$WORKSPACE/App/Packages/Generated/Package.swift" > "$WORKSPACE/manifest.tmp"
    mv "$WORKSPACE/manifest.tmp" "$WORKSPACE/App/Packages/Generated/Package.swift"
    host_library="$(jq -r 'to_entries[] | select(.value.kind == "apple_library" and .value.attrs.module_name == "Generated" and (.key | endswith("_MacroHost"))) | .key' "$WORKSPACE/graph.json")"
    graph_build "$host_library" || return
    [ -s "$WORKSPACE/.struo/out/$host_library/Generated.a" ] || return 1

    printf '\n#warning "Nested warning must be fatal"\n' >> "$WORKSPACE/App/Packages/Generated/Sources/CGenerated/Nested/Value.c"
    if graph_struo build App/Fetch > "$WORKSPACE/warning.json" 2> "$WORKSPACE/warning.err"; then
      return 1
    fi
    grep -q 'error:.*Nested warning must be fatal' "$WORKSPACE/warning.err" || return
    sed '/#warning/d' "$WORKSPACE/App/Packages/Generated/Sources/CGenerated/Nested/Value.c" > "$WORKSPACE/source.tmp"
    mv "$WORKSPACE/source.tmp" "$WORKSPACE/App/Packages/Generated/Sources/CGenerated/Nested/Value.c"

    FETCH_TUIST_BUILD=1 graph_build App/Fetch || return
    [ "$("$binary")" = '43:payload' ] || return 1
    perl -pi -e 's/fixtureGeneratorValue \+ traitAdjustment/fixtureGeneratorValue + traitAdjustment + 1/' "$WORKSPACE/App/Packages/Generated/Sources/Generator/main.swift"
    graph_build App/Fetch || return
    [ "$("$binary")" = '43:payload' ] || return 1
    perl -pi -e 's/prebuildValue = 2/prebuildValue = 3/' "$WORKSPACE/App/Packages/Generated/Tools/prebuild.swift"
    graph_build App/Fetch || return
    [ "$("$binary")" = '44:payload' ] || return 1
    perl -pi -e 's/Development/InternalBeta/' "$WORKSPACE/App/struo.toml"
    graph_build App/Fetch || return
    [ "$("$binary")" = '44:payload' ] || return 1

    rm -rf "$WORKSPACE/.struo/out"
    graph_build App/Fetch || return
    [ "$("$binary")" = '44:payload' ] || return 1
    if STRUO_PLUGIN_FAIL=1 graph_struo build App/Fetch > "$WORKSPACE/failure.json" 2> "$WORKSPACE/failure.err"; then
      return 1
    fi
    grep -q 'Requested lint failure' "$WORKSPACE/failure.err"
  }

  It 'reuses real Swift manifest discovery on a warm graph and reloads changed package membership'
    Skip if 'requires Xcode' native_graph_toolchain_unavailable
    When call exercise_warm_package_graph
    The status should be success
    The output should equal ''
  End

  exercise_toolchain_linker_settings() {
    developer="$SPEC_ROOT/Xcode.app/Contents/Developer"
    mkdir -p "$SPEC_ROOT/tools" "$WORKSPACE/App.xcodeproj" "$WORKSPACE/Sources"
    cat > "$SPEC_ROOT/tools/xcrun" <<SH
#!/bin/sh
case " \$* " in
  *' --show-sdk-path '*) printf '%s\n' '$developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk' ;;
  *' --find swiftc '*) printf '%s\n' '$SPEC_ROOT/swift-6.3-RELEASE.xctoolchain/usr/bin/swiftc' ;;
esac
SH
    chmod +x "$SPEC_ROOT/tools/xcrun"
    printf 'print("app")\n' > "$WORKSPACE/Sources/main.swift"
    cat > "$WORKSPACE/App.xcodeproj/project.xcproj" <<'JSON'
{
  "default-configuration": "Debug",
  "configurations": ["Debug"],
  "files": [{"path": "Sources/main.swift", "target-membership": ["App/compile-sources"]}],
  "targets": [{
    "name": "App",
    "product-type": "application",
    "build-phases": ["compile-sources"],
    "build-settings": {
      "PRODUCT_BUNDLE_IDENTIFIER": "dev.struo.toolchain-settings",
      "PRODUCT_NAME": "App",
      "OTHER_LDFLAGS": ["-L$(DT_TOOLCHAIN_DIR)/usr/lib/swift/$(PLATFORM_NAME)", "-L$(TOOLCHAIN_DIR)/usr/lib", "-F$(PLATFORM_DIR)/Developer/Library/Frameworks", "-L$(DEVELOPER_DIR)/usr/lib"]
    }
  }],
  "build-settings": {"SDKROOT": "macosx", "MACOSX_DEPLOYMENT_TARGET": "14.0"}
}
JSON
    DT_TOOLCHAIN_DIR='' PATH="$SPEC_ROOT/tools:$PATH" graph_snapshot || return
    jq -e --arg developer "$developer" --arg toolchain "$SPEC_ROOT/swift-6.3-RELEASE.xctoolchain" '
      .App.attrs.linkopts == [
        "-L\($developer)/Toolchains/XcodeDefault.xctoolchain/usr/lib/swift/macosx",
        "-L\($toolchain)/usr/lib",
        "-F\($developer)/Platforms/MacOSX.platform/Developer/Library/Frameworks",
        "-L\($developer)/usr/lib"
      ]
    ' "$WORKSPACE/graph.json" >/dev/null || { jq '.App.attrs.linkopts' "$WORKSPACE/graph.json"; return 1; }
  }

  It 'expands Xcode toolchain and platform directories in linker flags'
    When call exercise_toolchain_linker_settings
    The status should be success
    The output should equal ''
  End

  It 'accepts local package spelling aliases without duplicating a native package target'
    Skip if 'requires Xcode' native_graph_toolchain_unavailable
    When call exercise_package_path_alias
    The status should be success
    The output should equal ''
  End

  It 'reports sibling path packages outside the workspace and follows them from a common parent'
    Skip if 'requires Xcode' native_graph_toolchain_unavailable
    When call exercise_sibling_path_package
    The status should be success
    The output should equal ''
  End

  It 'builds and tests an Xcode workspace with source-built tools, prebuild discovery, resources, and output-free commands'
    Skip if 'requires Xcode' native_graph_toolchain_unavailable
    When call exercise_xcode_build_tools
    The status should be success
    The output should equal ''
  End

  It 'links a Swift package binary target backed by a static library artifact bundle'
    Skip if 'requires Xcode' native_graph_toolchain_unavailable
    When call exercise_artifact_bundle_binary_target
    The status should be success
    The output should equal ''
  End

  It 'parses a Vapor-shaped package graph and executes only the requested dependency closure'
    Skip if 'requires an Apple Swift compiler' native_graph_toolchain_unavailable
    When call exercise_vapor_graph
    The status should be success
    The output should equal ''
  End

  It 'separates NetNewsWire-shaped destinations and tracks native membership, settings, and local dependencies'
    Skip if 'requires Xcode' native_graph_toolchain_unavailable
    When call exercise_netnewswire_graph
    The status should be success
    The output should equal ''
  End
End
