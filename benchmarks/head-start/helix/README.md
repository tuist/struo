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

## Findings

Measured on 2026-11-27 with Struo 2.24.4 and Cargo 1.99 on a 16-CPU, 8 GiB
Linux machine, Helix at `1a3fdfd`. The committed results contain one smoke-test
run with loopback and 50 ms profiles and 20 history commits. Both tools build
with Helix's own settings: the harness clears Cargo settings from the caller's
environment, such as `CARGO_INCREMENTAL=0`.

These results predate the proxy handshake fix. The old proxy charged one RTT
per request but did not enforce the extra RTT for a new connection. Reruns
with the current proxy charge both. They also predate the HTTP cache restore
fix, so they do not measure the current Struo implementation.

- A fresh clone restores all 232 targets in 3.85 s on loopback and 24.32 s at
  50 ms, against Cargo's 21.19 s.
- A `helix-term` edit rebuilds 2 targets; a `helix-core` edit rebuilds 8.
- Cargo wins edits in a built checkout: 2.7 s and 3.1 s, against Struo's
  8.3 s and 13.0 s.
- A Struo build without a shared cache takes 40.3 s.
- Over the 20 sampled commits, 5% invalidate no targets and the 90th
  percentile invalidates 8. This small sample does not establish the
  distribution over Helix's longer history.

## What would make Struo faster here

These come from the cache traffic in `results/summary.json`:

- **Fewer bytes.** A no-change fresh clone downloads 871 MiB, every crate's
  compiled output, to produce one binary. At 200 Mbit/s that alone is about
  36 s. Fetching only the outputs a build needs, as Bazel's
  `--remote_download_minimal` does, and compressing stored content could
  remove much of it; neither is measured here.
- **Fewer round trips.** A build makes about 2,800 cache requests, 12 per
  target. In Struo 2.24.4, `struo cache serve` closes the connection after each
  response. Current Struo reuses connections and removes redundant HEADs;
  these historical results do not include that change.
- **Incremental compilation for local edits.** Struo's crate actions start
  from scratch, while Cargo retains incremental state. This benchmark does
  not measure how much of the difference comes from incremental compilation
  versus pipelining dependent compilation.
