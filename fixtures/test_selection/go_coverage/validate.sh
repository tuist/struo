#!/usr/bin/env bash
set -euo pipefail

: "${STRUO_BINARY:?Set STRUO_BINARY to an absolute Struo binary path}"
: "${TUIST_URL:?Set TUIST_URL to the test server URL}"
: "${TUIST_ACCOUNT:?Set TUIST_ACCOUNT to the test account}"
: "${TUIST_PROJECT:?Set TUIST_PROJECT to a disposable test project}"
: "${TUIST_TOKEN:?Set TUIST_TOKEN without printing it}"
export STRUO_CACHE_PROVIDER=local
source_dir=$(dirname "$(realpath "$0")")
workspace=$(mktemp -d)
cleanup() {
    if test -n "${STRUO_VALIDATION_OUTPUT:-}"; then
        mkdir -p "$STRUO_VALIDATION_OUTPUT"
        cp -R "$workspace/.struo/validation/." "$STRUO_VALIDATION_OUTPUT/"
    fi
    rm -rf "$workspace"
    if test -n "${fresh:-}"; then rm -rf "$fresh"; fi
}
trap cleanup EXIT
cp -R "$source_dir/." "$workspace/"
cd "$workspace"
rm validate.sh
printf '.struo/\n' > .gitignore
cat >> struo.toml <<EOF

[infrastructure.cache]
provider = "tuist"

[infrastructures.tuist]
kind = "tuist"
url = "$TUIST_URL"
account = "$TUIST_ACCOUNT"
project = "$TUIST_PROJECT"
EOF
git init -q
git config user.email coverage-fixture@example.invalid
git config user.name 'Coverage fixture'
git add .
git commit -qm 'Baseline coverage fixture'
git remote add origin https://example.invalid/coverage-selection.git
mkdir -p .struo/validation
query() { "$STRUO_BINARY" query test-plan --changed-path shared/alpha.go --format json "$@"; }
assert_targets() {
    local file=$1 expected=$2
    actual=$(jq -c '[.plan.selection.tests[].id] | sort' "$file")
    test "$actual" = "$expected" || { cat "$file"; echo "Expected $expected, got $actual"; exit 1; }
    printf '%s: %s\n' "$(basename "$file" .json)" "$actual"
}
await_targets() {
    local file=$1 expected=$2
    shift 2
    for attempt in $(seq 1 10); do
        "$@" > "$file"
        if test "$(jq -c '[.plan.selection.tests[].id] | sort' "$file")" = "$expected"; then
            assert_targets "$file" "$expected"
            return
        fi
        echo "Waiting for asynchronous coverage checkpoint ($attempt/10)"
        sleep 2
    done
    assert_targets "$file" "$expected"
}
assert_stale() {
    local plan=$1 label=$2
    printf '\n// Input changed after plan export.\n' >> beta/beta_test.go
    if "$STRUO_BINARY" test --plan "$plan" --format json > ".struo/validation/$label.json" 2> ".struo/validation/$label.stderr"; then
        echo 'Expected changed excluded target to invalidate portable plan'; exit 1
    fi
    grep -q 'stale for excluded target `BetaTests`' ".struo/validation/$label.stderr"
    git restore beta/beta_test.go
    echo "$label: rejected stale excluded target BetaTests"
}
query --coverage-selection > .struo/validation/no-evidence.json
assert_targets .struo/validation/no-evidence.json '["AlphaTests","BetaTests","NarrowTests"]'
"$STRUO_BINARY" test --all --coverage-report --jobs 2 --format json > .struo/validation/baseline.json || { cat .struo/validation/baseline.json; exit 1; }
jq -e '.coverage_reported == true' .struo/validation/baseline.json >/dev/null || { jq '{coverage_error, execution: .execution.execution.runs | map({target, success, results_error, error, stderr})}' .struo/validation/baseline.json; exit 1; }
echo 'baseline: complete isolated observations submitted'
printf '\n// Production-only change.\n' >> shared/alpha.go
git add shared/alpha.go
git commit -qm 'Change alpha source'
query > .struo/validation/default.json
assert_targets .struo/validation/default.json '["AlphaTests","BetaTests","NarrowTests"]'
await_targets .struo/validation/selected.json '["AlphaTests","NarrowTests"]' query --coverage-selection
jq -e '.plan.selection.policy.safety == "approximate"' .struo/validation/selected.json >/dev/null
assert_stale .struo/validation/selected.json stale-nonempty
fresh=$(mktemp -d)
git clone -q . "$fresh"
git -C "$fresh" remote set-url origin https://example.invalid/coverage-selection.git
"$STRUO_BINARY" --directory "$fresh" query test-plan --changed-path shared/alpha.go --coverage-selection --format json > .struo/validation/fresh.json
assert_targets .struo/validation/fresh.json '["AlphaTests","NarrowTests"]'
test ! -f "$fresh/.struo/out/BetaTests/test/test_results.json"
rm -rf "$fresh"
"$STRUO_BINARY" test --plan .struo/validation/selected.json --jobs 2 --format json > .struo/validation/executed.json
jq -e '.complete and .success' .struo/validation/executed.json >/dev/null
query --coverage-selection > .struo/validation/second.json
assert_targets .struo/validation/second.json '["AlphaTests","NarrowTests"]'
printf '\nfunc TestNew(t *testing.T) {}\n' >> beta/beta_test.go
git add beta/beta_test.go
git commit -qm 'Add test inventory'
query --coverage-selection > .struo/validation/incomplete-diff.json
assert_targets .struo/validation/incomplete-diff.json '["AlphaTests","BetaTests","NarrowTests"]'
query --coverage-selection --changed-path beta/beta_test.go > .struo/validation/new-test.json
assert_targets .struo/validation/new-test.json '["AlphaTests","BetaTests","NarrowTests"]'
git reset -q --hard HEAD~1
printf '\n# Configuration change.\n' >> struo.toml
git add struo.toml
git commit -qm 'Change graph configuration'
query --coverage-selection --changed-path struo.toml > .struo/validation/config.json
assert_targets .struo/validation/config.json '["AlphaTests","BetaTests","NarrowTests"]'
git reset -q --hard HEAD~1
cp shared/alpha.go shared/new.go
sed -i 's/Alpha/NewAlpha/g' shared/new.go
git add shared/new.go
git commit -qm 'Add source input'
query --coverage-selection --changed-path shared/new.go > .struo/validation/new-source.json
assert_targets .struo/validation/new-source.json '["AlphaTests","BetaTests","NarrowTests"]'
git reset -q --hard HEAD~1
git rm -q shared/unused.go
git commit -qm 'Delete source input'
query --coverage-selection --changed-path shared/unused.go > .struo/validation/deletion.json
assert_targets .struo/validation/deletion.json '["AlphaTests","BetaTests","NarrowTests"]'
git reset -q --hard HEAD~1
git mv shared/beta.go shared/renamed.go
git commit -qm 'Rename source input'
query --coverage-selection --changed-path shared/beta.go --changed-path shared/renamed.go > .struo/validation/rename.json
assert_targets .struo/validation/rename.json '["AlphaTests","BetaTests","NarrowTests"]'
git reset -q --hard HEAD~1
sed -i "s|url = .*|url = \"http://127.0.0.1:1\"|" struo.toml
git add struo.toml
git commit -qm 'Unavailable history provider'
query --coverage-selection --changed-path struo.toml > .struo/validation/outage.json
assert_targets .struo/validation/outage.json '["AlphaTests","BetaTests","NarrowTests"]'
git reset -q --hard HEAD~1
printf '\nfunc TestDynamic(t *testing.T) { t.Run("child", func(t *testing.T) {}) }\n' >> beta/beta_test.go
git add beta/beta_test.go
git commit -qm 'Add dynamic subtests'
"$STRUO_BINARY" test BetaTests --coverage-report --jobs 1 --format json > .struo/validation/dynamic-report.json
jq -e '.coverage_reported == false' .struo/validation/dynamic-report.json >/dev/null
query --coverage-selection --changed-path beta/beta_test.go > .struo/validation/dynamic.json
assert_targets .struo/validation/dynamic.json '["AlphaTests","BetaTests","NarrowTests"]'
git reset -q --hard HEAD~1
sed -i 's/!= 7/!= 999/' beta/beta_test.go
git add beta/beta_test.go
git commit -qm 'Introduce failing test'
if "$STRUO_BINARY" test BetaTests --coverage-report --jobs 1 --format json > .struo/validation/failed-report.json; then
    echo 'Expected failing test to fail execution'; exit 1
fi
jq -e '.coverage_reported == false' .struo/validation/failed-report.json >/dev/null
query --coverage-selection --changed-path beta/beta_test.go > .struo/validation/failed.json
assert_targets .struo/validation/failed.json '["AlphaTests","BetaTests","NarrowTests"]'
git reset -q --hard HEAD~1
sed -i 's/coverage_per_test = true/coverage_per_test = false\ncoverage = true/g' struo.toml
git add struo.toml
git commit -qm 'Use aggregate coverage only'
"$STRUO_BINARY" test BetaTests --coverage-report --jobs 1 --format json > .struo/validation/aggregate-report.json
jq -e '.coverage_reported == false' .struo/validation/aggregate-report.json >/dev/null
query --coverage-selection --changed-path struo.toml > .struo/validation/aggregate.json
assert_targets .struo/validation/aggregate.json '["AlphaTests","BetaTests","NarrowTests"]'
for control in new-test config dynamic failed aggregate; do
    jq -e '[.plan.selection.tests[].reasons[] | select(contains("changed paths do not match the baseline diff"))] | length == 0' ".struo/validation/$control.json" >/dev/null
done
git reset -q --hard HEAD~1
"$STRUO_BINARY" test --all --coverage-report --jobs 2 --format json > .struo/validation/refreshed.json
jq -e '.coverage_reported == true' .struo/validation/refreshed.json >/dev/null
printf '\n// Second production change.\n' >> shared/beta.go
git add shared/beta.go
git commit -qm 'Change beta after full refresh'
await_targets .struo/validation/after-refresh.json '["BetaTests","NarrowTests"]' "$STRUO_BINARY" query test-plan --changed-path shared/beta.go --coverage-selection --format json
git reset -q --hard HEAD~1
sed -i 's|cover_packages = \["example.com/coverage-selection/narrow"\]|cover_packages = ["example.com/coverage-selection/narrow", "example.com/coverage-selection/shared"]|' struo.toml
git add struo.toml
git commit -qm 'Instrument shared sources in every target'
"$STRUO_BINARY" test --all --coverage-report --jobs 2 --format json > .struo/validation/empty-baseline.json
jq -e '.coverage_reported == true' .struo/validation/empty-baseline.json >/dev/null
printf '\n// Instrumented but not exercised by any test.\n' >> shared/unused.go
git add shared/unused.go
git commit -qm 'Change unexercised shared source'
await_targets .struo/validation/empty.json '[]' "$STRUO_BINARY" query test-plan --changed-path shared/unused.go --coverage-selection --format json
"$STRUO_BINARY" test --plan .struo/validation/empty.json --format json > .struo/validation/empty-execution.json
jq -e '.complete and .success and (.execution.runs | length == 0)' .struo/validation/empty-execution.json >/dev/null
assert_stale .struo/validation/empty.json stale-empty
echo 'PASS: no evidence, default, selection, fresh checkout, execution, second selection, narrow instrumentation, complete-diff new test/config, incomplete diff, new source, deletion, rename, outage, dynamic subtests, failed tests, aggregate-only, full refresh, empty execution, stale nonempty/empty plans'
