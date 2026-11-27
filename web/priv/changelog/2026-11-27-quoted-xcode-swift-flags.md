---
title: Quoted Swift compiler flags in Xcode projects
date: 2026-11-27
---

Struo keeps quoted values in `OTHER_SWIFT_FLAGS` together as single compiler
arguments. This lets Xcode projects use Swift availability macros whose
definitions contain spaces, without changing their build settings.
