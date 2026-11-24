#!/usr/bin/env bash
set -euo pipefail
: "${STRUO_BINARY:?set STRUO_BINARY to an absolute binary path}"
: "${TUIST_URL:?set TUIST_URL to the disposable development server}"
: "${TUIST_ACCOUNT:?set TUIST_ACCOUNT}"
: "${TUIST_PROJECT:?set TUIST_PROJECT to a disposable project}"
: "${TUIST_TOKEN:?set TUIST_TOKEN without printing it}"
if [[ $# -ne 3 ]]; then
  echo 'Usage: validate.sh FIXTURE_DIRECTORY ZERO_HIT_SOURCE COVERED_SOURCE' >&2
  exit 2
fi
fixture=$(realpath "$1")
zero=$2
covered=$3
workspace=$(mktemp -d)
fresh=$(mktemp -d)
cleanup() {
  if [[ -n ${STRUO_VALIDATION_OUTPUT:-} ]]; then
    mkdir -p "$STRUO_VALIDATION_OUTPUT"
    cp -R "$workspace/.struo/validation/." "$STRUO_VALIDATION_OUTPUT/"
  fi
  rm -rf "$workspace" "$fresh"
}
trap cleanup EXIT
tar -C "$fixture" --exclude=.git --exclude=.struo --exclude=node_modules -cf - . | tar -C "$workspace" -xf -
cd "$workspace"
printf '.struo/\nnode_modules/\n' >> .gitignore
cat >> struo.toml <<EOF

[infrastructure.cache]
provider = "tuist"

[infrastructures.tuist]
kind = "tuist"
url = "$TUIST_URL"
account = "$TUIST_ACCOUNT"
project = "$TUIST_PROJECT"
EOF
export STRUO_CACHE_PROVIDER=local
if [[ -n ${STRUO_FIXTURE_SETUP:-} ]]; then bash -c "$STRUO_FIXTURE_SETUP"; fi
git init -q
git config user.email coverage-fixture@example.invalid
git config user.name 'Coverage fixture'
git add .
git commit -qm 'Coverage baseline fixture'
git remote add origin https://example.invalid/coverage-selection.git
mkdir -p .struo/validation
"$STRUO_BINARY" test --all --coverage-report --format json > .struo/validation/baseline.json
jq -e '.coverage_reported == true' .struo/validation/baseline.json >/dev/null || { cat .struo/validation/baseline.json; exit 1; }
printf '\n' >> "$zero"
git add "$zero"
git commit -qm 'Change zero-hit product source'
query() { "$STRUO_BINARY" query test-plan --changed-path "$zero" --format json "$@"; }
query > .struo/validation/default.json
jq -e '.plan.selection.tests | length > 0' .struo/validation/default.json >/dev/null
for attempt in $(seq 1 10); do
  query --coverage-selection > .struo/validation/selected.json
  if jq -e '.plan.selection.tests | length == 0' .struo/validation/selected.json >/dev/null; then break; fi
  echo "Waiting for asynchronous coverage checkpoint ($attempt/10)"
  sleep 2
done
jq -e '.plan.selection.tests | length == 0' .struo/validation/selected.json >/dev/null || { cat .struo/validation/selected.json; exit 1; }
jq -e '.plan.selection.policy.safety == "approximate"' .struo/validation/selected.json >/dev/null
git clone -q . "$fresh"
git -C "$fresh" remote set-url origin https://example.invalid/coverage-selection.git
if [[ -n ${STRUO_FIXTURE_SETUP:-} ]]; then (cd "$fresh"; bash -c "$STRUO_FIXTURE_SETUP"); fi
"$STRUO_BINARY" --directory "$fresh" query test-plan --changed-path "$zero" --coverage-selection --format json > .struo/validation/fresh.json
jq -e '.plan.selection.tests | length == 0' .struo/validation/fresh.json >/dev/null || { cat .struo/validation/fresh.json; exit 1; }
"$STRUO_BINARY" test --plan .struo/validation/selected.json --format json > .struo/validation/execute.json
jq -e '.success == true and (.execution.runs | length == 0)' .struo/validation/execute.json >/dev/null
query --coverage-selection > .struo/validation/repeat.json
jq -e '.plan.selection.tests | length == 0' .struo/validation/repeat.json >/dev/null
printf '\n' >> "$covered"
git add "$covered"
git commit -qm 'Change covered product source'
query --changed-path "$covered" --coverage-selection > .struo/validation/covered.json
jq -e '.plan.selection.tests | length > 0' .struo/validation/covered.json >/dev/null || { cat .struo/validation/covered.json; exit 1; }
for state in default selected fresh repeat covered; do
  printf '%s: ' "$state"
  jq -c '[.plan.selection.tests[].id] | sort' ".struo/validation/$state.json"
done
echo 'PASS: full baseline, strict default, opt-in exclusion, fresh worker, empty execution, repeat and covered-source retention'
