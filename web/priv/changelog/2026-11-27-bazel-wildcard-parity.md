---
title: Bazel workspaces build and test like a Bazel wildcard
date: 2026-11-27
---

`struo build` and `struo test` in a Bazel workspace now select the same rules
as `bazel build //...` and `bazel test //...`: test rules are built too, and
rules tagged `manual` are skipped unless you name them. An action that several
targets share runs once per invocation instead of once per target, so targets
no longer rewrite a generated file while another target is reading it.
Executable symlinks and executable file writes keep their executable mode,
Bazel 8 runfiles trees are materialized for owned tests, and owned tests run
with the environment Bazel analyzed for them, with make variables expanded.

Files Struo snapshots from the host, such as a Bazel external repository's
sources, are now keyed by their contents and mode rather than by where they
live. Two checkouts of the same workspace, whose Bazel output bases differ,
now produce the same keys for those copies and everything built from them.
