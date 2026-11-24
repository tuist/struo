#!/usr/bin/env bash
set -euo pipefail
: "${STRUO_BINARY:?set STRUO_BINARY to an absolute macOS struo binary}"
fixture=$(cd "$(dirname "$0")" && pwd)
native="$fixture/../apple_native_coverage"
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
for kind in swift_package_workspace xcode_workspace; do
  workspace="$scratch/$kind"
  manifest=Package.swift
  if [[ "$kind" == xcode_workspace ]]; then manifest=Coverage.xcodeproj/project.pbxproj; fi
  mkdir -p "$workspace"
  cp -R "$fixture/Product" "$fixture/SwiftTests.swift" "$fixture/XCTestTests.swift" "$workspace/"
  cat > "$workspace/struo.toml" <<EOF
[[target]]
name = "native"
kind = "$kind"
srcs = ["$manifest", "Product/*.swift", "*Tests.swift"]
[target.attrs]
coverage_per_test = true
EOF
  if [[ "$kind" == swift_package_workspace ]]; then
    cp "$native/Package.swift" "$workspace/"
    printf 'minimum_os = "14.0"\n' >> "$workspace/struo.toml"
  else
    cp -R "$native/Coverage.xcodeproj" "$workspace/"
    printf 'project = "Coverage.xcodeproj"\n' >> "$workspace/struo.toml"
  fi
  (
    cd "$workspace"
    git init -q
    git add .
    git -c user.name=Fixture -c user.email=fixture@example.invalid commit -qm baseline
    "$STRUO_BINARY" test --all --format json > before-relocation.json
    mv "$workspace" "$workspace-relocated"
    cd "$workspace-relocated"
    "$STRUO_BINARY" test --all --format json > run.json
    python3 - "$kind" <<'PY'
import glob, json, sys
reports = [json.load(open(path)) for path in glob.glob('.struo/out/**/test_results.json', recursive=True)]
expected = 2 if sys.argv[1] == 'swift_package_workspace' else 1
assert len(reports) == expected, reports
for report in reports:
    assert report['status'] == 'passed', report
    assert report['coverage']['partial'] is False, report
    assert report['coverage_evidence']['status'] == 'collected', report
    assert [case['id'] for case in report['cases']] == [report['target'] + '::invocation'], report
    files = {entry['path']:entry for entry in report['coverage']['files']}
    assert any(files['Product/Covered.swift']['execution_counts']), files
    assert not any(files['Product/Unused.swift']['execution_counts']), files
    print(sys.argv[1] + ': complete native atomic coverage for ' + report['target'])
PY
  )
done
echo 'PASS: resolved SwiftPM and Xcode host coverage'
