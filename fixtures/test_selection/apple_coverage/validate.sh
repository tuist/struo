#!/usr/bin/env bash
set -euo pipefail
: "${STRUO_BINARY:?set STRUO_BINARY to an absolute macOS struo binary}"
fixture=$(cd "$(dirname "$0")" && pwd)
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
cp -R "$fixture"/. "$scratch/"
cd "$scratch"
git init -q
git add .
git -c user.name=Fixture -c user.email=fixture@example.invalid commit -qm baseline
"$STRUO_BINARY" build Product --format json > product-build.json
mv "$scratch" "$scratch-relocated"
scratch="$scratch-relocated"
cd "$scratch"
for target in SwiftTests XCTestTests; do
  "$STRUO_BINARY" test "$target" --format json > "$target.json"
  python3 - "$target" <<'PY'
import glob, json, sys
target = sys.argv[1]
reports = [json.load(open(path)) for path in glob.glob('.struo/out/**/test_results.json', recursive=True)]
report = next(report for report in reports if report['target'] == target)
assert report['status'] == 'passed', report
assert report['coverage']['partial'] is False, report
assert [case['id'] for case in report['cases']] == [target + '::invocation'], report
assert report['coverage_evidence']['status'] == 'collected', report
files = {entry['path']: entry for entry in report['coverage']['files']}
assert any(files['Product/Covered.swift']['execution_counts']), files
assert not any(files['Product/Unused.swift']['execution_counts']), files
print(target + ': complete atomic evidence with positive and zero-hit sources')
PY
  for control in CHILD SKIP FAIL; do
    cp struo.toml original.toml
    python3 - "$target" "$control" <<'PY'
import pathlib, sys
path = pathlib.Path('struo.toml')
text = path.read_text()
section = 'name = "' + sys.argv[1] + '"'
start = text.index(section)
offset = text.index('coverage_per_test = true', start)
text = text[:offset] + 'test_env = { STRUO_FIXTURE_' + sys.argv[2] + ' = "1" }\n' + text[offset:]
path.write_text(text)
PY
    status=0
    "$STRUO_BINARY" test "$target" --format json > "$target-$control.json" || status=$?
    if [[ "$control" == FAIL ]]; then test "$status" -ne 0; else test "$status" -eq 0; fi
    python3 - "$target" "$control" <<'PY'
import glob, json, sys
reports = [json.load(open(path)) for path in glob.glob('.struo/out/**/test_results.json', recursive=True)]
report = next(report for report in reports if report['target'] == sys.argv[1])
assert report.get('coverage', {}).get('partial', True), report
assert report.get('coverage_evidence', {}).get('status') != 'collected', report
print(sys.argv[1] + ': ' + sys.argv[2] + ' cannot establish complete evidence')
PY
    mv original.toml struo.toml
  done
done
echo 'PASS: Apple native atomic coverage controls'
