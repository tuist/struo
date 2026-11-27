---
title: CMake projects work without a struo.toml
date: 2026-11-27
---

Struo now discovers a CMake project from its top-level `CMakeLists.txt`. While
the graph loads, it configures the project with CMake's file-based API and creates one target
per library or executable, named after the CMake target, plus one test target
per CTest test, all running in one shared build tree. `struo build`, `struo test`, and
`struo query affected-tests` work right away, and discovered libraries publish
their headers, including ones CMake generates, to native targets that depend on
them. An authored `cmake_project` or `cmake_test` that covers the project's
`CMakeLists.txt` still takes precedence.

`cmake_project` now also stages the file a product link points to, so a
versioned shared library product is reused from the cache instead of copied
again on every build.

With GCC or Clang, `cmake_project` also maps the workspace path to `.` while
compiling and links build-tree executables with an `$ORIGIN`-relative RUNPATH,
so debug builds of C and C++ code no longer record the checkout location in
their products.

`struo query affected-tests` selects only the CTest tests whose executables,
arguments, or fixtures a change reaches. Discovery keeps its configured tree
between loads and configures again only when the project's configuration
changes. A discovered versioned shared library also stages its SONAME link, and
assembly sources and `ExternalProject` builds no longer record the checkout
location.

Discovered products now come out of one incremental build tree, so a change
recompiles only what it reaches, and a product's identity depends only on the
sources its CMake target reaches.
