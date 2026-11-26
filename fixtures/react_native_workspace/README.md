# React Native workspace

A JavaScript workspace whose app, `apps/mobile`, depends on the sibling
workspace package `@acme/theme`. The `react_native_dependencies` target lives
at the workspace root, points `package_json` at the root manifest that declares
`workspaces`, and lists every member manifest and local source the install
needs in `srcs`.

`vendor/` holds empty `react-native` and `hermes-compiler` packages so the
frozen `npm ci` runs offline. Regenerate the lockfile with
`npm install --package-lock-only --offline` after changing a manifest.
