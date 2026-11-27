---
title: More concurrent Rust compilation within the memory budget
date: 2026-11-27
---

Local Rust compiler actions now reserve an estimated 512 MiB instead of 2 GiB.
The previous estimate limited an 8 GiB machine to two concurrent Rust
compilations under the default memory budget, leaving CPU cores idle during
cold builds.

Local build scripts reserve 512 MiB per declared job. A four-job native build
script still reserves four CPU slots and 2 GiB. Compilation, source staging,
and cache operations continue to use the shared resource pool.

Provider-backed Rust actions retain their 2 GiB allocation because isolated
workers may enforce it as a hard limit.
