---
title: Cargo metadata snapshots skip feature-disabled optional dependencies
date: 2026-11-26
---

A Cargo graph loaded from a checked-in `metadata_file` snapshot no longer turns
optional dependencies that the selected features leave disabled into build
targets. Cargo metadata lists every optional dependency of a package, enabled or
not, so a crate that depends on `sqlx` with only its Postgres driver used to
pull the SQLite and MySQL drivers into the graph and fail the build compiling
them. Struo now keeps an optional dependency only when an enabled feature turns
it on, the same way Cargo does. Graphs resolved live from Cargo already behaved
this way.
