---
title: Xcode targets retain their Swift compilation conditions
date: 2026-11-27
---

Struo now preserves target conditions such as `FEATURE_X` in a list entry like
`$(inherited) FEATURE_X`, even when the project sets different inherited
conditions for Debug and Release. Quoted text in list entries stays together.
