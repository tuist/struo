---
title: Cached tests no longer read Cargo's target directory
date: 2026-11-27
---

Before and after every test run, `struo test` checks whether the files that
decide which tests exist have changed. That check now skips the paths a target
excludes from its sources. A cached test of a Cargo package whose `target`
directory sits inside the workspace no longer reads and hashes every build
output and `.git` object, which on a large workspace took minutes. Source
entries without wildcards are looked up directly instead of walked, and a
symbolic link that points outside the workspace still counts by what it
reaches.
