---
title: CMake projects can publish the headers they generate
date: 2026-11-27
---

A `cmake_project` target can now pass headers that CMake generates during
configuration, such as a `config.h` written by `configure_file`, to the native
targets that depend on it. List each generated header in `products` and name
its directory, relative to the generated build directory, in the new
`generated_includes` attribute. Dependents compile against the staged header
and rebuild when its contents change. Previously only headers in the source
tree could be published, so libraries such as json-c, whose public `json.h` is
generated, could not be consumed.
