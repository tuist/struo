#!/bin/sh
#
# Head Start: builds an open source project with its own build tool and with
# Struo, from fresh checkouts that share a cache, over several simulated network
# paths to that cache. Writes results/ next to the project's project.sh.
#
#   mise exec -- benchmarks/head-start/run.sh helix
#
# See AGENTS.md for the method and how to add a project.

set -eu

root="$(CDPATH= cd -- "$(dirname "$0")" && pwd)"
if [ $# -ne 1 ] || [ ! -f "$root/$1/project.sh" ]; then
  printf 'usage: %s <project>\nprojects: %s\n' "$0" "$(cd "$root" && ls -d */ | tr -d / | tr '\n' ' ')" >&2
  exit 2
fi
project="$root/$1"
raw_binary="${STRUO_BINARY:-$root/../../target/release/struo}"
binary="$(CDPATH= cd -- "$(dirname "$raw_binary")" && pwd)/$(basename "$raw_binary")"
runs="${RUNS:-3}"
# name:round-trip milliseconds:megabits per second (0 means unlimited)
profiles="${PROFILES:-loopback:0:0 rtt-10ms:10:0 rtt-50ms:50:0 rtt-150ms:150:0 same-region:2:5000 office:20:1000 home:40:200}"
history_commits="${HISTORY_COMMITS:-500}"
port="${PORT:-18093}"
state="$project/.state"
results="$project/results"
. "$project/project.sh"
# Both tools must build with the project's own settings, not the caller's.
isolate_env

if [ ! -x "$binary" ]; then
  printf 'struo binary not found at %s; set STRUO_BINARY\n' "$binary" >&2
  exit 2
fi

rm -rf "$state" "$results"
mkdir -p "$state/config/struo" "$state/server" "$state/runs" "$results"
upstream="$state/upstream"
source="$state/source.git"
work="$state/work"

git clone --quiet --no-checkout "$repository" "$upstream"
git -C "$upstream" -c advice.detachedHead=false checkout --quiet "$revision"
revision_date="$(git -C "$upstream" log -1 --format=%cI)"

# How often recent commits invalidate the graph, from the unmodified project.
node "$root/history.mjs" "$upstream" "$binary" "$root_target" "$history_commits" "$manifests" >"$results/history.json"

(cd "$upstream" && setup)
git -C "$upstream" -c user.name=benchmark -c user.email=benchmark@struo.run \
  -c commit.gpgsign=false commit --quiet --all --allow-empty --message "benchmark setup"
git clone --quiet --bare "$upstream" "$source"
rm -rf "$work"
git -c advice.detachedHead=false clone --quiet "$source" "$work"
(cd "$work" && sh -c "$native_fetch")
"$binary" -C "$work" query targets --format json >"$state/targets.json"

printf '{"token":"%s"}\n' "$(od -An -N24 -tx1 /dev/urandom | tr -d ' \n')" >"$state/credential.json"
chmod 600 "$state/credential.json"
BENCHMARK_CACHE_TOKEN="$(sed 's/.*"token":"\([^"]*\)".*/\1/' "$state/credential.json")"
export BENCHMARK_CACHE_TOKEN
"$binary" --memory-limit 512MiB cache serve --listen "127.0.0.1:$port" \
  --data-dir "$state/server" --token-file "$state/credential.json" --max-size 16GiB \
  >"$state/server.log" 2>&1 &
server=$!
proxy=
trap 'kill $server $proxy 2>/dev/null || true' EXIT

# Struo reaches the server only through proxy.mjs, which logs every cache
# request and applies the network profile.
cat >"$state/config/struo/config.toml" <<EOF
[infrastructures.benchmark]
kind = "http_cache"
url = "http://127.0.0.1:$((port + 1))"
token_env = "BENCHMARK_CACHE_TOKEN"

[infrastructure.cache]
provider = "benchmark"
EOF
export XDG_CONFIG_HOME="$state/config"

now() { date +%s%N; }
# A fresh checkout: new clone at the same path, no build output, empty local
# Struo cache. Only the shared cache server keeps state.
fresh() {
  rm -rf "$work" "$state/local"
  git -c advice.detachedHead=false clone --quiet "$source" "$work"
}
edit() { printf '\n// edited %s\n' "$(now)" >>"$work/$1"; }

time_native() { # output prefix
  start=$(now)
  (cd "$work" && sh -c "$native_build") >/dev/null 2>&1
  echo $((($(now) - start) / 1000000)) >"$1.ms"
}

time_struo() { # output prefix, round trip ms, megabits per second
  node "$root/proxy.mjs" --listen $((port + 1)) --upstream "$port" --log "$1.jsonl" --rtt-ms "$2" --mbit "$3" &
  proxy=$!
  sleep 0.5
  rm -rf "$state/logs"
  start=$(now)
  XDG_STATE_HOME="$state/logs" XDG_CACHE_HOME="$state/local" "$binary" -C "$work" build "$root_target" --quiet >/dev/null
  echo $((($(now) - start) / 1000000)) >"$1.ms"
  kill "$proxy"
  wait "$proxy" 2>/dev/null || true
  proxy=
  # Nested Struo invocations write short logs of their own; keep the build's.
  cp "$(ls -S "$state"/logs/struo/logs/*.log | head -1)" "$1.log"
}

cases="fresh:"
for file in $edits; do cases="$cases edit-${file%%/*}:$file"; done

# The first build fills the empty shared cache.
mkdir -p "$state/runs/seed"
fresh
time_struo "$state/runs/seed/struo-loopback-1" 0 0

# Runs are interleaved so drift on the machine spreads over every scenario.
for i in $(seq "$runs"); do
  for case in $cases; do
    name=${case%%:*}
    file=${case#*:}
    dir="$state/runs/$name"
    mkdir -p "$dir"
    fresh
    [ -z "$file" ] || edit "$file"
    time_native "$dir/native-$i"
    for profile in $profiles; do
      rest=${profile#*:}
      fresh
      [ -z "$file" ] || edit "$file"
      time_struo "$dir/struo-${profile%%:*}-$i" "${rest%%:*}" "${rest#*:}"
    done
  done
  # Struo with no shared cache at all, which also stores every result locally.
  mkdir -p "$state/runs/cold"
  fresh
  STRUO_CACHE_PROVIDER=local time_struo "$state/runs/cold/struo-loopback-$i" 0 0
done

# Edits in a checkout that is already built: the native tool keeps its build
# directory and Struo its local cache.
fresh
(cd "$work" && sh -c "$native_build") >/dev/null 2>&1
time_struo "$state/runs/seed/struo-loopback-2" 0 0
for i in $(seq "$runs"); do
  for file in $edits; do
    dir="$state/runs/incremental-edit-${file%%/*}"
    mkdir -p "$dir"
    # One edit for both tools, so each build sees exactly the changes since
    # its own previous build.
    edit "$file"
    time_native "$dir/native-$i"
    time_struo "$dir/struo-loopback-$i" 0 0
  done
done

# One native build with per-unit timings, for the replay card.
fresh
(cd "$work" && sh -c "$native_timeline_build" >/dev/null 2>&1 && sh -c "$native_timeline '$state/targets.json' '$root_target'") >"$state/native-timeline.json"

cat >"$results/meta.json" <<EOF
{
  "series": "Head Start",
  "project": "$name_display",
  "repository": "$repository",
  "revision": "$revision",
  "revision_date": "$revision_date",
  "root_target": "$root_target",
  "edits": "$edits",
  "date": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "struo": "$("$binary" --version)",
  "native": "$(cd "$work" && sh -c "$native_version")",
  "native_build": "$native_build",
  "setup": "$setup_notes",
  "machine": "$(uname -sm), $(nproc) CPUs, $(awk '/MemTotal/ {printf "%.0f GiB", $2 / 1048576}' /proc/meminfo 2>/dev/null || sysctl -n hw.memsize)",
  "runs": $runs,
  "profiles": "$profiles",
  "history_commits": $history_commits
}
EOF
node "$root/summarize.mjs" "$state" "$results"
