---
title: Action validation reports undeclared reads
date: 2026-11-25
---

`struo query validate-actions` and the `struo_validate_actions` MCP tool now
trace the files an action reads. Each undeclared workspace file the action
tried to read becomes an `undeclared_read` diagnostic with a repair, including
reads the action recovered from and reads that walked out of its sandbox. The
probe stages each one and reruns, so a single validation lists every
undeclared read before the action succeeds. On Linux, each action also lists the
files it read outside the workspace, such as toolchains and system libraries.

Tracing works on Linux (x86-64 and arm64) and on macOS. When the probe cannot
trace an action's reads, the action reports a `trace_unavailable` diagnostic and
is not valid. Declared inputs that an action never read are not reported.
