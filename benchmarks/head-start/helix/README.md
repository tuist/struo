# Head Start: Helix

[Helix](https://github.com/helix-editor/helix) is a modal terminal text editor
written in Rust. This benchmark builds its `hx` binary with Cargo and with
Struo, which loads Helix's graph straight from its `Cargo.toml` with no Struo
configuration. Thanks to the Helix maintainers for a codebase this pleasant to
build.

`results/RESULTS.md` has every number, with the revision, versions, machine
and parameters in `results/meta.json`. Method and network profiles are in
[`../AGENTS.md`](../AGENTS.md). To rerun:

```sh
STRUO_BINARY=path/to/struo mise exec -- benchmarks/head-start/run.sh helix
```

## Historical findings

Measured on 2026-11-27 with a patched Struo development build and Cargo 1.99
on a 16-CPU, 8 GiB Linux machine, Helix at `1a3fdfd`, median of three runs.
The binary reports `struo 0.0.0`; these are not published-release results.
The run used source base `6af79d48` with the local Rust admission patch. It
predates the proxy handshake fix and HTTP cache connection/restore changes.
The old proxy did not enforce its extra delay for new connections, so the
network-profile results do not measure the current proxy or Struo. Cold
builds use no shared cache and are unaffected by that proxy issue.

Both tools build with Helix's own settings: the harness clears Cargo settings from the caller's
environment, such as `CARGO_INCREMENTAL=0`.

- **A fresh clone with the cache nearby restores the whole build.** With the
  cache in the same region (2 ms, 5 Gbit/s), a fresh clone builds `hx` in 4.0 s
  against Cargo's 27.1 s, compiling none of its 232 targets. With `helix-term`
  edited it rebuilds 2 targets in 13.3 s, and with `helix-core` edited 8
  targets in 17.2 s, against Cargo's 27.7 s and 31.1 s.
- **Most commits need nothing rebuilt.** Of Helix's last 500 commits, 55%
  touch no file of any target in the graph, and the 90th percentile
  invalidates 8 targets. `helix-term` changes in 26% of commits and is the hot
  path; `helix-view` changes in 11%, and each of those also invalidates
  `helix-tui`, `helix-term` and `hx`.
- **Distance to the cache decides the result.** Struo wins from an office
  (20 ms, 1 Gbit/s: 14.9 s) and up to about 50 ms of latency with ample
  bandwidth (25.7 s, about even). From home (40 ms, 200 Mbit/s: 45.1 s) and at
  150 ms (70.4 s), Cargo compiling locally on 16 CPUs is faster.
- **Cargo wins edits in a checkout it already built.** Its incremental
  compilation rebuilds a `helix-term` edit in 3.0 s and a `helix-core` edit in
  3.7 s. Struo compiles crates without incremental state and takes 9.2 s and
  16.1 s.
- **Cold builds now roughly match Cargo.** The patched build takes 25.7 s
  with an empty cache against Cargo's 27.1 s. Its local Rust action estimates
  reserve 512 MiB per job instead of 2 GiB per action, admitting more work
  within the same memory budget. Cold runs were 25.74, 24.66 and 36.58 s;
  this is not a claim that Struo always wins.

## Run validation

The full three-round, seven-profile run completed with 89 timing samples and
no HTTP 5xx responses across 74 proxy logs. The measured proxy reserved one
of the old cache server's eight connection slots for downloads still closing;
the current proxy and cache server retain their upstream fixes.

One of 21 no-change fresh builds, loopback round three, recompiled
`helix-core` and `helix-term` and took 11.31 s rather than 3.31 or 3.40 s.
Historical cache index blobs confirm variant retention caused these misses.
Each target had accumulated the baseline plus 14 distinct edit variants;
only the latest eight remained discoverable. Cache hits do not refresh that
index. Before round three, every retained variant disagreed with the original
source. The original records still match all 42 core inputs and 56 term inputs,
but their keys were no longer in the index. Recompiling restored the baseline
entry. The outlier remains in the results. The median compiled-target count
of zero must not be read as every run being a complete cache hit.

## What would make Struo faster here

These come from the cache traffic in `results/summary.json`:

- **Fewer bytes.** A no-change fresh clone downloads 871 MiB, every crate's
  compiled output, to produce one binary. At 200 Mbit/s that alone is about
  36 s. Fetching only the outputs a build needs, as Bazel's
  `--remote_download_minimal` does, and compressing stored content could
  remove much of it; neither is measured here.
- **Fewer round trips.** The measured build makes about 2,800 cache requests,
  12 per target, and its cache server closes each connection after responding.
  Current Struo reuses connections and removes redundant HEADs. These
  historical results do not include those changes or enforce the intended
  extra handshake delay, so they cannot quantify the current latency slope.
- **Incremental compilation for local edits.** Struo's crate actions start
  from scratch, which is what loses the built-checkout case to Cargo. Starting
  dependents from `.rmeta`, as Cargo does, is another possible optimization;
  its benefit was not measured here.
