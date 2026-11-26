---
title: Lower build-memory overhead and faster cache recovery
date: 2026-11-26
---

Struo now shares immutable dependency-provider metadata between consumers and
defers decoding cached providers until a consumer needs them. Consumer mutations
remain isolated. Cached builds also avoid repeated configuration parsing and
reject missing outputs before loading their digest maps.

Linux commands use a process-spawning backend that preserves process-group
cancellation and signal handling. Action concurrency and memory limits still
govern admission, and cache recovery retains its corruption checks and durable
publication guarantees.

Rust cache integrations can supply exact byte counts through
`put_stream_sized`, `put_produced_sized`, and `put_produced_pipelined_sized` to
reduce compression workspace allocations. Each method rejects a length mismatch
before publishing the blob. See the [Rust SDK guide](/guide/sdk/rust).
