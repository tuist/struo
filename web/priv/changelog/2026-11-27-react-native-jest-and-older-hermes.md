---
title: Expo exports with npm and pnpm, Jest test selection, and React Native 0.81 and 0.82
date: 2026-11-27
---

`struo query affected-tests` now reads the files a `jest_test` or
`vitest_test` target declares in `data` and `config`. A change to an app
source that a Jest suite lists as test data now selects that suite, in
addition to the targets that already owned the path.

`jest_test` no longer fails with Haste duplicate-module errors when another
target, such as a React Native bundle, has staged a copy of the project under
`.struo`. Struo keeps that directory out of Jest's module map and keeps the
project's own ignore patterns.

React Native 0.81 and 0.82 projects now load and bundle with Hermes. Those
releases ship the Hermes compiler inside `react-native`, so their lockfiles
have no usable `hermes-compiler` package; Struo uses the bundled compiler, as
React Native's Gradle plugin does.

`javascript_build` has a new `command` attribute that runs an installed
package binary, such as `expo`, through the package manager's own exec
command. The documented Expo export now works with npm and pnpm, whose `run`
only runs `package.json` scripts.

`react_native_bundle` now gives Metro a private temporary directory. Metro
keeps its cache there and every bundle resets it, so bundles that run at the
same time, for Android and iOS or in two checkouts, no longer clear each
other's cache in the shared system temporary directory.

`react_native_bundle` now caps Metro at four transform workers by default and
reserves memory for each, so a large app no longer runs the host out of memory
and hangs when a worker is killed. Set `max_workers` to change the cap.

`react_native_dependencies` no longer keeps pnpm's install state files in the
installed tree. They record the absolute store path and a timestamp, so bundles
staged from a pnpm install now produce the same cache keys in every checkout.
