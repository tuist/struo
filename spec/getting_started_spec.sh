#shellcheck shell=bash

verify_documented_site_package() (
  cd "$WORKSPACE" || return
  mkdir -p scripts site/css
  awk '
    /^#!\/usr\/bin\/env -S struo exec -- bash$/ { copying = 1 }
    copying && /^```/ { exit }
    copying { print }
  ' "$REPO_ROOT/web/priv/docs/guide/getting-started.md" > scripts/package-site.sh
  [ -s scripts/package-site.sh ] || return 1
  chmod +x scripts/package-site.sh
  PATH="$(dirname "$STRUO_BIN"):$PATH"
  export PATH

  printf '<h1>Welcome</h1>\n' > site/index.html
  printf 'h1 { color: tomato; }\n' > site/css/main.css
  ./scripts/package-site.sh > first.txt 2>&1 || return
  grep -q 'cache miss' first.txt || return
  ./scripts/package-site.sh > second.txt 2>&1 || return
  grep -q 'cache hit' second.txt || return

  rm -r build
  ./scripts/package-site.sh > restored.txt 2>&1 || return
  grep -q 'cache hit' restored.txt || return
  [ "$(tar -xOzf build/site.tar.gz ./index.html)" = '<h1>Welcome</h1>' ] || return 1

  printf '<h1>Welcome back</h1>\n' > site/index.html
  ./scripts/package-site.sh > changed.txt 2>&1 || return
  grep -q 'cache miss' changed.txt || return
  [ "$(tar -xOzf build/site.tar.gz ./index.html)" = '<h1>Welcome back</h1>' ] || return 1
  printf 'documented site package reuses, restores, and invalidates correctly\n'
)

Describe 'getting started documentation'
  BeforeEach 'setup_workspace'
  AfterEach 'cleanup_workspace'

  It 'runs the published script through its Struo shebang'
    When call verify_documented_site_package
    The status should be success
    The stdout should equal 'documented site package reuses, restores, and invalidates correctly'
  End
End
