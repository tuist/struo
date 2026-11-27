# Head Start

A benchmark series: a well-known open source project, built with its own build
tool and with Struo, from a fresh checkout that can use a shared cache. The name
is the claim. Struo gives the project's contributors and CI a head start; it is
never a story about the project being slow, and the post credits the project.

Every number posted about a project comes from `<project>/results/`, which the
harness writes and the repository keeps, so anyone can check and rerun it.

## Method

`run.sh <project>` does the whole run:

1. Clones the project at the pinned `revision` and applies `setup()`. Setup only
   removes differences the comparison is not about, such as a network download
   in a build script or a profile default Struo does not apply, and the same
   setup applies to both tools. `setup_notes` says what it changed.
2. Replays the last `HISTORY_COMMITS` (500) first-parent commits onto Struo's
   graph of `root_target` with `history.mjs`: how many targets each commit
   invalidates once dependents are included, and which targets change most.
3. Starts `struo cache serve` and puts `proxy.mjs` in front of it. Struo only
   talks to the proxy, which logs every request and emulates a network path:
   one round trip per request plus one per new connection, and a bandwidth
   limit each way. It runs without root and on macOS, unlike `tc netem`, and
   shapes only cache traffic, not the native tool's.
4. Fills the shared cache with one Struo build, then runs `RUNS` (3) rounds.
   Each round builds every scenario from a fresh clone at the same path with
   the native tool and with Struo over every network profile, then once with
   Struo and no shared cache. Rounds interleave tools and scenarios so machine
   drift spreads evenly.
5. Times edits in an already-built checkout, where the native tool keeps its
   build directory and Struo its local cache.
6. Records one native build's per-unit timeline and writes `meta.json`
   (versions, date, machine, parameters), `summary.json`, `timeline.json` and
   `RESULTS.md`.

Scenarios: `fresh` (nothing changed), `edit-<dir>` for every file in `edits`
(appends a comment, so the file's target and its dependents rebuild), `cold`
(no shared cache), and `incremental-edit-<dir>`.

Default network profiles, as `name:round-trip ms:Mbit/s` (0 is unlimited), set
with `PROFILES`. The `rtt-*` profiles vary only latency; the others add the
bandwidth of a real setup, since restoring a build downloads every cached
output it needs:

| Profile | Round trip | Bandwidth | Stands for |
| --- | --- | --- | --- |
| `loopback` | 0 | unlimited | the cache on the same machine |
| `rtt-10ms`, `rtt-50ms`, `rtt-150ms` | 10, 50, 150 ms | unlimited | latency alone |
| `same-region` | 2 ms | 5 Gbit/s | CI runners in the cache's cloud region |
| `office` | 20 ms | 1 Gbit/s | a developer on a good wired connection |
| `home` | 40 ms | 200 Mbit/s | a developer at home |

The proxy charges one round trip per request and one per new connection. A server that closes connections after each response pays both for every request. Request latency starts when the proxy receives the request and includes any remaining simulated handshake delay. The connection delay is an application-level simulation, not an emulation of TCP or TLS. Run `node --test benchmarks/head-start/proxy.test.mjs` on an otherwise idle machine to check connection-delay and reuse behavior.

## Run

Build or download the Struo release being measured, then:

```sh
STRUO_BINARY=path/to/struo mise exec -- benchmarks/head-start/run.sh helix
```

Measure a published release, not a local build, and run on an otherwise idle
machine: no renders, browsers or other builds alongside. A run of Helix takes
about 30 minutes and 5 GiB of disk under `<project>/.state/`, which is ignored.
Commit `results/` with the project.

## Add a project

Pick a project people know, whose logo the project publishes under an open
license with no trademark policy against this use, and that Struo builds from
its own manifests. Then copy `helix/project.sh` into `<project>/project.sh`:

| Variable | Meaning |
| --- | --- |
| `name_display`, `repository`, `revision` | the project and the full commit SHA measured; update `revision` deliberately, never to a branch |
| `root_target` | the Struo target that corresponds to the native build, from `struo query targets` |
| `edits` | two files: one in the target closest to the root, one in a target most of the graph depends on |
| `manifests` | globs for dependency manifests and lockfiles, counted separately in the history |
| `native_name`, `native_version`, `native_fetch`, `native_build` | the native tool, how to print its version, fetch dependencies before timing, and build the same output |
| `native_timeline_build`, `native_timeline` | a native build that records per-unit timings, and a command that prints them as `{seconds, targets: {id: {start, end}}}` given `targets.json` and the root target |
| `setup`, `setup_notes` | see Method |
| `isolate_env` | unsets environment variables that change the native tool's build, such as Cargo profile or incremental settings a parent `mise.toml` exports; Struo reads the project's manifests instead, so leaving them set compares different builds |

Before the full run, smoke-test with
`RUNS=1 PROFILES="loopback:0:0" HISTORY_COMMITS=20`. Check that Struo's binary
works, that the native and Struo builds produce the same artifact, and that
`compiled_targets` in `summary.json` is 0 for `fresh` and small for the edits.

## Reporting

- Show where Struo is slower too: `cold`, the incremental edits, and any
  profile where the native tool wins stay in `RESULTS.md` and in the project's
  README.
- The card and post use only numbers in `results/`, and say which profile
  they come from.
- The social card and posting steps are in `socials/head-start/AGENTS.md`.
