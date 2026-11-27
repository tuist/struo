---
title: Builds stop on a dependency that names no target
date: 2026-11-27
---

`struo build`, `struo run`, and `struo test` used to skip a dependency that
named no target and build as if it were not declared. They now stop with the
`missing_dependency` diagnostic that `struo query validate-workspace` reports.
A dependency without `./` resolves from the project root, so when the
manifest's own package declares a target with that name, the diagnostic
suggests the `./` reference to write. When the target is missing because a
resolver failed to load, the resolver's error is reported instead.
