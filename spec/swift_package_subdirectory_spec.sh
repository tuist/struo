#shellcheck shell=bash

Describe 'Swift package in a workspace subdirectory'
  BeforeEach 'setup_workspace'
  AfterEach 'cleanup_workspace'

  linux_host_unavailable() { [ "$(uname -s)" != Linux ]; }
  macos_toolchain_unavailable() { [ "$(uname -s)" != Darwin ] || ! xcrun --find swift >/dev/null 2>&1; }

  write_hello_package() {
    package="$WORKSPACE/packages/hello"
    mkdir -p "$package/Sources/Hello" "$package/Sources/HelloKit" "$package/Tests/HelloTests"
    cat > "$package/Package.swift" <<'SWIFT'
// swift-tools-version: 5.10
import PackageDescription

let package = Package(
    name: "Hello",
    platforms: [.macOS(.v14)],
    products: [.executable(name: "Hello", targets: ["Hello"])],
    targets: [
        .executableTarget(name: "Hello", dependencies: ["HelloKit"], path: "Sources/Hello"),
        .target(name: "HelloKit", path: "Sources/HelloKit"),
        .testTarget(name: "HelloTests", dependencies: ["HelloKit"], path: "Tests/HelloTests"),
    ]
)
SWIFT
    printf 'import HelloKit\nprint(greeting)\n' > "$package/Sources/Hello/main.swift"
    printf 'public let greeting = "hello"\n' > "$package/Sources/HelloKit/HelloKit.swift"
    cat > "$package/Tests/HelloTests/HelloTests.swift" <<'SWIFT'
import XCTest
@testable import HelloKit

final class HelloTests: XCTestCase {
    func testGreeting() { XCTAssertEqual(greeting, "hello") }
}
SWIFT
    cat > "$SPEC_ROOT/dump-package.json" <<'JSON'
{"name":"Hello","toolsVersion":{"_version":"5.10.0"},"platforms":[{"platformName":"macos","version":"14.0","options":[]}],"dependencies":[],
"products":[{"name":"Hello","targets":["Hello"],"type":{"executable":null},"settings":[]}],
"targets":[
{"name":"Hello","type":"executable","path":"Sources/Hello","dependencies":[{"byName":["HelloKit",null]}],"exclude":[],"resources":[],"settings":[]},
{"name":"HelloKit","type":"regular","path":"Sources/HelloKit","dependencies":[],"exclude":[],"resources":[],"settings":[]},
{"name":"HelloTests","type":"test","path":"Tests/HelloTests","dependencies":[{"byName":["HelloKit",null]}],"exclude":[],"resources":[],"settings":[]}]}
JSON
  }

  write_sqlite_package() {
    package="$WORKSPACE/sqlite"
    mkdir -p "$package/Sources/SQLiteKitSQLite3" "$package/Sources/SQLiteKit"
    cat > "$package/Package.swift" <<'SWIFT'
// swift-tools-version: 5.10
import PackageDescription

let package = Package(
    name: "SQLiteKit",
    products: [.library(name: "SQLiteKit", targets: ["SQLiteKit"])],
    targets: [
        .systemLibrary(name: "SQLiteKitSQLite3", providers: [.apt(["libsqlite3-dev"])]),
        .target(name: "SQLiteKit", dependencies: ["SQLiteKitSQLite3"]),
    ]
)
SWIFT
    printf 'module SQLiteKitSQLite3 [system] {\n  header "shim.h"\n  link "sqlite3"\n  export *\n}\n' > "$package/Sources/SQLiteKitSQLite3/module.modulemap"
    printf '#include <sqlite3.h>\n' > "$package/Sources/SQLiteKitSQLite3/shim.h"
    printf 'import SQLiteKitSQLite3\n' > "$package/Sources/SQLiteKit/SQLiteKit.swift"
    cat > "$SPEC_ROOT/dump-package.json" <<'JSON'
{"name":"SQLiteKit","toolsVersion":{"_version":"5.10.0"},"platforms":[],"dependencies":[],
"products":[{"name":"SQLiteKit","targets":["SQLiteKit"],"type":{"library":["automatic"]},"settings":[]}],
"targets":[
{"name":"SQLiteKitSQLite3","type":"system","dependencies":[],"exclude":[],"resources":[],"settings":[],"providers":[{"apt":[["libsqlite3-dev"]]}]},
{"name":"SQLiteKit","type":"regular","dependencies":[{"byName":["SQLiteKitSQLite3",null]}],"exclude":[],"resources":[],"settings":[]}]}
JSON
  }

  # A Linux host with a registered Darwin SDK, reduced to what graph loading
  # asks of it: compiler discovery, SDK configuration, and the manifest dump
  # the real `swift package dump-package` would print for the Package.swift
  # the package writer created.
  install_stub_darwin_toolchain() {
    toolchain="$SPEC_ROOT/toolchain"
    sdk="$toolchain/sdk/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk"
    resources="$toolchain/sdk/Developer/Toolchains/XcodeDefault.xctoolchain/usr/lib/swift"
    mkdir -p "$toolchain/bin" "$toolchain/lib/swift" "$sdk" "$resources" "$toolchain/sdk/toolset/bin"
    cat > "$toolchain/bin/swift" <<SH
#!/bin/sh
case "\$1 \$2" in
  '-print-target-info '*) printf '{"paths":{"runtimeResourcePath":"%s"}}\n' '$toolchain/lib/swift' ;;
  '--version '*) echo 'Stub Swift version 6.3' ;;
  'sdk list') echo darwin ;;
  'sdk configure') printf 'sdkRootPath: %s\nswiftResourcesPath: %s\n' '$sdk' '$resources' ;;
  'package '*) cat '$SPEC_ROOT/dump-package.json' ;;
  *) echo "unexpected swift invocation: \$*" >&2; exit 1 ;;
esac
SH
    chmod +x "$toolchain/bin/swift"
    cp "$toolchain/bin/swift" "$toolchain/bin/swiftc"
    cp "$toolchain/bin/swift" "$toolchain/bin/swift-frontend"
    PATH="$toolchain/bin:$PATH"
  }

  parent_graph() {
    write_hello_package
    install_stub_darwin_toolchain
    STRUO_CHANGE_TRACKER=0 struo --format json query 'MATCH (t:Target) RETURN t.id, t.kind, t.srcs' > "$SPEC_ROOT/query.json" || return
    jq -c '[.rows[] | select(.[0] | startswith("packages/hello/SwiftPackage_Hello_")) | select(.[0] | endswith("_MacroHost") | not)] | sort' "$SPEC_ROOT/query.json"
  }

  It 'lowers the package targets under the subdirectory when loaded from the parent'
    Skip if 'stubs the Linux Darwin SDK toolchain' linux_host_unavailable
    When call parent_graph
    The status should be success
    The output should equal '[["packages/hello/SwiftPackage_Hello_Hello","apple_application",["Sources/Hello/main.swift"]],["packages/hello/SwiftPackage_Hello_HelloKit","apple_library",["Sources/HelloKit/HelloKit.swift"]],["packages/hello/SwiftPackage_Hello_HelloTests","apple_test_bundle",["Tests/HelloTests/HelloTests.swift"]]]'
  End

  sqlite_graph() {
    write_sqlite_package
    install_stub_darwin_toolchain
    STRUO_CHANGE_TRACKER=0 struo --format json query 'MATCH (t:Target) RETURN t.id, t.kind, t.srcs, t.deps' > "$SPEC_ROOT/query.json" || return
    jq -c '[.rows[] | select(.[0] | startswith("sqlite/SwiftPackage_SQLiteKit_")) | select(.[0] | endswith("_MacroHost") | not)] | sort' "$SPEC_ROOT/query.json"
  }

  It 'resolves a system library module map under the subdirectory when loaded from the parent'
    Skip if 'stubs the Linux Darwin SDK toolchain' linux_host_unavailable
    When call sqlite_graph
    The status should be success
    The output should equal '[["sqlite/SwiftPackage_SQLiteKit_SQLiteKit","apple_library",["Sources/SQLiteKit/SQLiteKit.swift"],["sqlite/SwiftPackage_SQLiteKit_SQLiteKitSQLite3"]],["sqlite/SwiftPackage_SQLiteKit_SQLiteKitSQLite3","apple_system_module",["Sources/SQLiteKitSQLite3/module.modulemap","Sources/SQLiteKitSQLite3/shim.h"],[]]]'
  End

  root_resources_graph() {
    mkdir -p "$WORKSPACE/Sources/Assets/Resources/Declared.xcassets" "$WORKSPACE/Sources/Assets/Copied"
    printf '{}\n' > "$WORKSPACE/Sources/Assets/Resources/Declared.xcassets/Contents.json"
    printf 'hello\n' > "$WORKSPACE/Sources/Assets/Resources/message.txt"
    printf 'raw\n' > "$WORKSPACE/Sources/Assets/Copied/raw.txt"
    printf 'public let assets = 1\n' > "$WORKSPACE/Sources/Assets/Assets.swift"
    printf '// swift-tools-version: 5.9\n' > "$WORKSPACE/Package.swift"
    cat > "$SPEC_ROOT/dump-package.json" <<'JSON'
{"name":"Root","toolsVersion":{"_version":"5.9.0"},"platforms":[{"platformName":"macos","version":"13.0","options":[]}],"dependencies":[],
"products":[{"name":"Assets","targets":["Assets"],"type":{"library":["automatic"]},"settings":[]}],
"targets":[{"name":"Assets","type":"regular","dependencies":[],"exclude":[],"resources":[{"path":"Resources","rule":{"process":{}}},{"path":"Copied/raw.txt","rule":{"copy":{}}}],"settings":[]}]}
JSON
    install_stub_darwin_toolchain
    STRUO_CHANGE_TRACKER=0 struo --format json query 'MATCH (t:Target) RETURN t.id, t.attrs.resources' > "$SPEC_ROOT/query.json" || return
    jq -c '.rows[] | select(.[0] == "SwiftPackage_Root_Assets") | .[1]' "$SPEC_ROOT/query.json"
  }

  It 'keeps resource paths workspace-relative for a package at the workspace root'
    Skip if 'stubs the Linux Darwin SDK toolchain' linux_host_unavailable
    When call root_resources_graph
    The status should be success
    The output should equal '["Sources/Assets/Resources/Declared.xcassets","Sources/Assets/Resources/message.txt","Sources/Assets/Copied/raw.txt"]'
  End

  It 'builds and tests the package from the parent directory'
    Skip if 'requires Xcode' macos_toolchain_unavailable
    write_hello_package
    build_and_test() {
      STRUO_CHANGE_TRACKER=0 struo --memory-limit 2GiB build > "$SPEC_ROOT/build.out" 2>&1 || { cat "$SPEC_ROOT/build.out"; return 1; }
      [ -d "$WORKSPACE/.struo/out/packages/hello/SwiftPackage_Hello_HelloKit" ] || [ -n "$(find "$WORKSPACE/.struo/out" -name 'HelloKit.swiftmodule' -print -quit)" ] || { cat "$SPEC_ROOT/build.out"; return 1; }
      STRUO_CHANGE_TRACKER=0 struo --memory-limit 2GiB test 2>&1
    }
    When call build_and_test
    The status should be success
    The output should include 'SwiftPackage_Hello_HelloTests: 1 passed, 0 failed'
  End
End
