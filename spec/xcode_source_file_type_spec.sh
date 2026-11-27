#shellcheck shell=bash

Describe 'Xcode source file types'
  BeforeEach 'setup_workspace'
  AfterEach 'cleanup_workspace'

  xcode_unavailable() {
    [ "$(uname -s)" != Darwin ] || ! xcrun --find swiftc >/dev/null 2>&1
  }

  build_store_app() {
    cp -R "$REPO_ROOT/fixtures/xcode_objcpp_file_type/." "$WORKSPACE/"
    vendor="$WORKSPACE/Store/Vendor"
    (close_shellspec_fds; xcrun clang++ -c -target "$(uname -m)-apple-macosx13.0" "$vendor/separator.cpp" -o "$vendor/separator.o" && xcrun libtool -static -o "$vendor/libseparator.a" "$vendor/separator.o") || return 1
    rm "$vendor/separator.o"
    (close_shellspec_fds; STRUO_CHANGE_TRACKER=0 "$STRUO_BIN" -C "$WORKSPACE" build StoreApp --format json) > "$WORKSPACE/build.json" 2> "$WORKSPACE/build.err" || { cat "$WORKSPACE/build.err"; return 1; }
    "$WORKSPACE/.struo/out/StoreApp/StoreApp.app/Contents/MacOS/StoreApp"
  }

  It 'compiles a .m file whose explicit file type is Objective-C++ as Objective-C++ and links the vendored C++ archive from its Frameworks phase'
    Skip if 'requires Xcode' xcode_unavailable
    When call build_store_app
    The status should be success
    The output should equal 'objc+cpp'
  End
End
