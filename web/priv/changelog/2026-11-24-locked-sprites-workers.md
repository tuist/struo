---
title: Reuse locked build environments on Sprites
date: 2026-11-24
---

Sprites execution can now reuse a bounded pool of workers prepared from
`mise.toml`, `mise.lock`, and explicit setup commands. The locked environment
is part of action identity, while source edits reuse the same preparation.
Changing the lock or setup rotates idle workers within the pool limit.

Struo installs its pinned, checksum-verified mise once per worker. A native
managed service retains verified blobs and read-only tools, while every action
runs with copied inputs in a fresh filesystem and process namespace. Successful
actions clean up without restoring a checkpoint. Interrupted or uncertain
actions retain checkpoint recovery. Exclusive leases work across CLI processes.

Worker reuse is opt-in and intended for trusted workspace builds. Linux
bubblewrap is required; execution fails rather than running without isolation.
CPU and memory admission remain bounded, and transfers stream through bounded
buffers. Struo sends no idle keepalives, leaving suspension to Sprites. Retained
workers continue to incur storage charges. With a Sprites or HTTP cache,
workers publish output files directly and downstream workers fetch inputs by
digest. The coordinator still restores outputs for inspection and local use.
Cache-only credentials stay outside actions, read-only policy is preserved,
and unavailable or corrupt shared objects fall back to coordinator transfers.

Cargo build and test graphs can use these prepared workers for individual
crate compilations, build scripts, and whole test targets. Run `struo build`
or `struo test --all --jobs 2` with `--remote --compute PROVIDER`. Struo resolves
the Rust compiler in the worker and verifies it against the coordinator's
compiler before execution. Both sides must use the same locked toolchain and
compatible native dependencies. Test schedules preserve the parent's resolved
execution configuration across their subprocesses.

See the [Sprites guide](/guide/infrastructure/sprites#locked-worker-pool) for
configuration, supported lockfiles, scope, and cleanup.
