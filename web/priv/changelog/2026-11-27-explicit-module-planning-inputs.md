---
title: Explicit Apple module planning restores cached inputs
date: 2026-11-27
---

Explicit Apple module planning now declares the compilation inputs it
inspects alongside the dependency scan. Clean cached builds restore those
inputs before planning, so hashing a cached dependency's Swift module no
longer fails when its output is absent from disk. Other unused dependency
outputs remain in the cache.
