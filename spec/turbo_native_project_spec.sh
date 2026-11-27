#shellcheck shell=bash
# End-to-end specs for the graph Struo derives from a Turborepo workspace
# without a manifest of its own. Turbo's package graph and dry run provide the
# task graph that Struo schedules, caches, and runs without `turbo run`.

Describe 'turborepo native project'
  BeforeEach 'setup_workspace'
  AfterEach 'cleanup_workspace'

  copy_starter() {
    cp -R "$REPO_ROOT/crates/struo-frontend/prelude/examples/turbo-workspace-native-project/." "$WORKSPACE/"
  }

  # ShellSpec reads `Skip` only at the start of a line, so a guard such as
  # `cmd || Skip` runs `Skip` as a missing command. `Skip if` takes these
  # conditions instead.
  node_unavailable() {
    ! command -v node >/dev/null 2>&1
  }

  npm_unavailable() {
    ! command -v npm >/dev/null 2>&1
  }

  # Runs in a subshell, as `Skip if` conditions do; the installed packages
  # stay in the workspace.
  npm_install_fails() {
    ! (cd "$WORKSPACE" && npm install --no-audit --no-fund --loglevel=error >/dev/null 2>&1)
  }

  target_ids() {
    struo --format json query targets | jq -r '.[] | "\(.kind) \(.id)"' | sort
  }

  It 'derives Turborepo tasks with no struo.toml present'
    Skip if 'node and npm are required' npm_unavailable
    copy_starter
    rm "$WORKSPACE/struo.toml" "$WORKSPACE/graph.json"
    Skip if 'npm install failed; skipping (offline?)' npm_install_fails

    When call target_ids
    The status should be success
    The stdout should equal "turbo_task _example_greeting__build
turbo_task hello__build
turbo_test hello__test
turbo_workspace turbo"
    The path "$WORKSPACE/struo.toml" should not be exist
  End

  cache_and_invalidation() {
    printf '%s ' "$(struo --format json build hello__build | jq -r .cache)" || return
    rm -rf "$WORKSPACE/apps/hello/dist" "$WORKSPACE/packages/greeting/dist"
    printf '%s ' "$(struo --format json build hello__build | jq -r .cache)" || return
    test ! -e "$WORKSPACE/packages/greeting/dist/index.js" || return
    sed -i.bak "s/Hello, /Hi, /" "$WORKSPACE/packages/greeting/build.js" && rm "$WORKSPACE/packages/greeting/build.js.bak"
    printf '%s ' "$(struo --format json build hello__build | jq -r .cache)" || return
    cat "$WORKSPACE/apps/hello/dist/message.txt"
  }

  It 'restores outputs from cache and rebuilds consumers when an upstream package changes'
    Skip if 'node is required' node_unavailable
    copy_starter

    When call cache_and_invalidation
    The status should be success
    The stdout should equal 'miss hit miss Hi, Turborepo!'
  End

  It 'fails the suite when its upstream build output changes the result'
    Skip if 'node is required' node_unavailable
    copy_starter
    sed -i.bak "s/Hello, /Hi, /" "$WORKSPACE/packages/greeting/build.js"

    When call struo test hello__test
    The status should be failure
    The stderr should include 'hello__test'
    The stderr should include 'AssertionError'
  End
End

# A pnpm monorepo that keeps the task graph patterns of public Turborepo
# projects; see fixtures/turbo_monorepo/README.md.
Describe 'turborepo pnpm monorepo'
  BeforeEach 'setup_workspace'
  AfterEach 'cleanup_workspace'

  install_monorepo() {
    command -v node >/dev/null 2>&1 || return 1
    cp -R "$REPO_ROOT/fixtures/turbo_monorepo/." "$WORKSPACE/"
    # Reuse the caller's package caches; the spec isolates HOME.
    npm_config_cache="${SPEC_ORIGINAL_HOME:-$HOME}/.npm"
    export npm_config_cache
    if command -v pnpm >/dev/null 2>&1; then
      set -- pnpm
    else
      set -- npx -y pnpm@10.19.0
    fi
    (cd "$WORKSPACE" && "$@" install --frozen-lockfile --store-dir "${SPEC_ORIGINAL_HOME:-$HOME}/.local/share/pnpm/store" >/dev/null 2>&1)
  }

  # Runs in a subshell, as `Skip if` conditions do; the copied fixture and
  # its installed packages stay in the workspace.
  monorepo_unavailable() {
    ! install_monorepo
  }

  # The Turborepo tasks that ran instead of coming from cache while building
  # each given target, sorted and space-separated.
  executed_tasks() {
    for target in "$@"; do
      struo -v build "$target" 2>&1 | sed -n 's/.*Run Turborepo task · \([^ ]*\) · .* · executed.*/\1/p'
    done | LC_ALL=C sort -u | tr '\n' ' '
  }

  target_graph() {
    struo --format json query targets | jq -r '.[] | [.kind, .id] + (.deps // []) | join(" ")' | sort
  }

  It 'derives the task graph turbo resolves, bridging tasks that have no script'
    Skip if 'node and pnpm are required (offline?)' monorepo_unavailable

    When call target_graph
    The status should be success
    The stdout should equal "turbo_task _acme_core__build
turbo_task _acme_core__lint
turbo_task _acme_ui__lint _acme_core__build
turbo_task _acme_web__build _acme_core__build
turbo_task _acme_web__lint _acme_core__build
turbo_task root__lint
turbo_test _acme_core__test
turbo_test _acme_web__test _acme_core__build
turbo_workspace turbo _acme_core__build _acme_web__build"
  End

  restore_outputs() {
    struo --format json build _acme_web__build | jq -r .cache || return
    rm -rf "$WORKSPACE/apps/web/build" "$WORKSPACE/packages/core/dist" "$WORKSPACE/packages/core/.cache"
    struo --format json build _acme_web__build | jq -r .cache || return
    test ! -e "$WORKSPACE/packages/core/.cache/build-info.json" || return
    struo --format json build _acme_core__build >/dev/null || return
    cat "$WORKSPACE/apps/web/build/index.html" "$WORKSPACE/packages/core/.cache/build-info.json"
    test ! -e "$WORKSPACE/apps/web/build/cache" && echo 'cache directory not restored'
  }

  It 'builds through workspace bins and source-only packages and restores outputs except negated ones'
    Skip if 'node and pnpm are required (offline?)' monorepo_unavailable

    When call restore_outputs
    The status should be success
    The stdout should equal 'miss
hit
<h1>Hello, Turborepo!</h1>
<!-- development -->
{"files":["index.js"]}
cache directory not restored'
  End

  shared_configuration_change() {
    executed_tasks turbo root__lint _acme_ui__lint >/dev/null
    printf '{ "banner": "/* acme v2 */" }\n' > "$WORKSPACE/tooling/config/compiler.json"
    executed_tasks turbo root__lint _acme_ui__lint
    head -1 "$WORKSPACE/packages/core/dist/index.js"
  }

  # Root tasks receive the whole workspace, so `//#lint` reruns too; the
  # unrelated package lint tasks stay cached.
  It 'reruns only the tasks downstream of a changed configuration package'
    Skip if 'node and pnpm are required (offline?)' monorepo_unavailable

    When call shared_configuration_change
    The status should be success
    The stdout should equal '//#lint @acme/core#build @acme/ui#lint @acme/web#build /* acme v2 */'
  End

  environment_changes() {
    struo build _acme_web__build >/dev/null 2>&1 || return
    APP_MODE=production executed_tasks _acme_web__build
    tail -1 "$WORKSPACE/apps/web/build/index.html"
    APP_MODE=production executed_tasks _acme_web__build
    printf '| '
    APP_MODE=production ACME_REGION=eu executed_tasks _acme_web__build
  }

  It 'passes declared task and global environment variables and keys the cache on them'
    Skip if 'node and pnpm are required (offline?)' monorepo_unavailable

    When call environment_changes
    The status should be success
    The stdout should equal '@acme/web#build <!-- production -->
| @acme/core#build @acme/web#build '
  End

  path_changes() {
    struo build _acme_web__build >/dev/null 2>&1 || return
    mkdir -p "$WORKSPACE/../extra-bin" "$WORKSPACE/../other-home"
    HOME="$WORKSPACE/../other-home" PATH="$WORKSPACE/../extra-bin:$PATH" executed_tasks _acme_web__build
    printf 'none'
  }

  It "keeps the caller's PATH and HOME out of task cache keys"
    Skip if 'node and pnpm are required (offline?)' monorepo_unavailable

    When call path_changes
    The status should be success
    The stdout should equal 'none'
  End

  It 'runs every package test suite'
    Skip if 'node and pnpm are required (offline?)' monorepo_unavailable

    When call struo test
    The status should be success
    The stdout should include '_acme_core__test: 1 passed, 0 failed'
    The stdout should include '_acme_web__test: 1 passed, 0 failed'
  End

  lint_with_finding() {
    printf '\n// FIXME\n' >> "$WORKSPACE/apps/web/src/page.js"
    struo build _acme_web__lint
  }

  It 'fails lint tasks, including root tasks, on findings'
    Skip if 'node and pnpm are required (offline?)' monorepo_unavailable

    When call lint_with_finding
    The status should be failure
    The stderr should include 'src/page.js: FIXME'
  End
End
