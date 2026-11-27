---
title: Tuist remote execution works with larger capabilities responses
date: 2026-11-27
---

Remote builds on Tuist no longer fail with "the capabilities response exceeds
its size limit". The Tuist server now lists task-sizing details for its
execution gateway in the response Struo reads when a build starts, which grows
with the project and passed the 64 KiB Struo used to accept. Struo now accepts
up to 1 MiB and ignores fields it does not use.
