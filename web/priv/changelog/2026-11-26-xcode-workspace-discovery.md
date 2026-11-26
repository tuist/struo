---
title: Discovered projects choose their entry point, or say exactly how to
date: 2026-11-26
---

When Struo discovers a native project without a `struo.toml`, its target kind
now picks the entry point from an ordered list of patterns. A discovered Xcode
project resolves the single `.xcworkspace` at the top of the directory, the one
Xcode opens, so repositories with demo apps, tools, or playground projects no
longer fail with "multiple Xcode projects found". Without a workspace, the
single top-level `.xcodeproj` is used. A root that holds only an
`.xcworkspace`, with every project in a subdirectory, is now discovered as one
seed for the workspace instead of one seed per nested project.

When a directory holds several equally preferred entry points, only that seed
reports a `native_entry_point_ambiguous` diagnostic. Each repair is the exact
`struo.toml` target that selects one candidate, and the rest of the graph keeps
loading. Module authors can use the same `native_entry_point` helper for other
ecosystems; see the [modules reference](/reference/modules).
