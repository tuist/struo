#!/usr/bin/env bash
set -euo pipefail

tools="$(dirname "$0")/tools"
mkdir -p "$tools"
base=https://repo.maven.apache.org/maven2/org/jacoco
curl -fsSL "$base/org.jacoco.agent/0.8.16/org.jacoco.agent-0.8.16-runtime.jar" -o "$tools/jacocoagent.jar"
curl -fsSL "$base/org.jacoco.cli/0.8.16/org.jacoco.cli-0.8.16-nodeps.jar" -o "$tools/jacococli.jar"
printf '%s  %s\n' \
  37ae2f374ad77e85c59c6e5b46565226c3ac3cb460ac19de0bcb844ac2f43e33 "$tools/jacocoagent.jar" \
  1f2a806225623fbd9f53d1ee0e42a6975323de0adebe01c12fc464ed147d756a "$tools/jacococli.jar" |
  sha256sum --check --status
