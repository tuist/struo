---
title: Cached builds skip unused dependency downloads
date: 2026-11-27
---

`struo build` now restores cached outputs for the requested target without
restoring each dependency's outputs first. When an action needs to run, Struo
restores the cached inputs it reads before starting it. A fully cached build
can leave intermediate libraries in the shared cache instead of downloading
them to produce an already-cached binary.

Actionless workspace targets still restore their selected dependency outputs,
and run and test commands still restore their prerequisite build roots.

Struo still checks whether cached results reference available content and
uses the existing memory and transfer budgets when restoring outputs.
