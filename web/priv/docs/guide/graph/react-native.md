# React Native Apps

Build, run, and cache React Native iOS and Android apps with Struo: dependency
installs, Metro bundles, Hermes bytecode, codegen, and native builds.

Struo models an application as separate dependency, discovery, code generation,
JavaScript bundle, native application, and development-server targets. The
package managers stay authoritative: [npm](https://www.npmjs.com/),
[pnpm](https://pnpm.io/), [Yarn](https://yarnpkg.com/), or
[Bun](https://bun.sh/) installs the JavaScript lockfile,
[CocoaPods](https://cocoapods.org/) integrates Apple native dependencies, and
[Gradle](https://gradle.org/) integrates Android native dependencies. Struo runs
their work as explicit, cacheable actions without replacing their resolution.

## Prerequisites

- Node.js and the package manager that matches the project's lockfile.
- For Apple builds, Xcode with `xcodebuild` and Bundler, which runs CocoaPods.
  `struo run` launches on a booted iOS Simulator.
- For Android builds, `java` and an Android SDK found through the
  `android_sdk` attribute or `ANDROID_HOME`, plus the
  [Android Debug Bridge](https://developer.android.com/tools/adb) and a
  connected device or emulator for `struo run`.

A first native build may fetch missing CocoaPods sources, a Gradle
distribution, or Gradle artifacts.

## Declare targets

### Start from the bundled application

The bundled starter uses React Native 0.86 with the
[New Architecture](https://reactnative.dev/architecture/landing-page),
[Fabric](https://reactnative.dev/architecture/fabric-renderer),
[Hermes](https://reactnative.dev/docs/hermes), and autolinked native modules.
It contains Apple and Android application targets in one package. Materialize
and validate it:

```sh
struo query example react_native_apple_application react-native-application-minimal --output ./HelloReactNative
cd HelloReactNative
struo query validate-workspace
```

### Build the applications

Build the dependency graph and platform applications:

```sh
struo build Dependencies
struo build StruoBaseline
struo build StruoBaselineAndroid
```

Build release JavaScript bundles separately when a distribution workflow needs
them:

```sh
struo build AppleBundle
struo build AndroidBundle
```

Each bundle is produced by Metro, compiled to Hermes bytecode with the compiler
that matches the locked React Native release, and accompanied by a composed
source map.

### Develop with Fast Refresh

Start [Metro](https://metrobundler.dev/) against the live workspace:

```sh
struo run Metro
```

In another terminal, launch a cached native application on the iOS Simulator:

```sh
struo run StruoBaseline
```

For Android:

```sh
struo run StruoBaselineAndroid
```

The Android run installs the application package and forwards Metro's port
through `adb`. Set `adb_serial` when more than one Android device is connected.

Editing JavaScript and TypeScript sources triggers Fast Refresh without
rebuilding a debug native application. A release application build includes
those sources in its cache key.

### Confirm caching

Dependency installs, Metro bundles, Hermes bytecode, generated native
interfaces, module metadata, and native application products are cached
independently. Device installation, application launch, port forwarding, and
the Metro server always run. Use repeated verbose builds to inspect individual
cache decisions:

```sh
struo -vv build Dependencies
struo -vv build StruoBaseline
```

Changes to the native project, lockfile, module discovery, or generated
interfaces invalidate the relevant native build, which then starts from clean
CocoaPods or Gradle sources. Generated native directories are excluded from
sources by default; use `exclude_srcs` for other project-specific output
directories.

## Manage dependencies

### Install JavaScript packages

Struo supports `package-lock.json`, `pnpm-lock.yaml`, `yarn.lock`, and
`bun.lock`. It detects the manager from the target attributes, the
`packageManager` field, or the declared lockfile.

Declare package-manager configuration with `package_manager_files` and include
it in `resolver_inputs`. For Yarn's `yarnPath`, also list the checked-in release
in `srcs`. Package-manager configuration must not contain credentials, because
inputs can enter a shared cache.

### Monorepos

When the app depends on sibling packages through package-manager workspaces,
the dependency target describes the whole workspace, not just the app. Take
this layout:

```text
package.json                 # "workspaces": ["apps/*", "packages/*"]
package-lock.json
apps/mobile/package.json     # depends on "@acme/theme"
packages/theme/package.json
packages/theme/theme.js
```

Declare the target in the root `struo.toml`, next to the lockfile:

```toml
[[target]]
name = "Dependencies"
kind = "react_native_dependencies"
srcs = [
  "package.json",
  "package-lock.json",
  "apps/mobile/package.json",
  "packages/theme/package.json",
  "packages/theme/theme.js",
]

[target.attrs]
package_json = "package.json"  # the manifest that declares `workspaces`
```

- `package_json` is the root manifest, not the app's.
- `srcs` lists every workspace member's `package.json`, plus any file a member
  needs at install time, such as its entry point or lifecycle scripts. Struo
  installs from the declared files only.
- With pnpm, also add `pnpm-workspace.yaml` to `srcs`.

The same layout works with Bun, pnpm, and Yarn.

#### Troubleshooting

| Error | Fix |
| --- | --- |
| `Workspace dependency "@acme/theme" not found` | Point `package_json` at the root manifest. |
| `resolves outside its owner package` | Move the target from the app's `struo.toml` to the root one. |
| `lockfile had changes, but lockfile is frozen` | Add the missing workspace member's `package.json` to `srcs`. |

npm does not always report these mistakes and can install an incomplete
`node_modules` instead. Fix the declaration rather than disabling frozen
installs or regenerating the lockfile.

### Native modules

`react_native_autolinking` runs the React Native community
[autolinking](https://github.com/react-native-community/cli/blob/main/docs/autolinking.md)
configuration and emits module metadata. Check that snapshot into the package
when you want one `react_native_module` node per native package in the graph.

`react_native_codegen` runs the official
[Codegen](https://reactnative.dev/docs/the-new-architecture/what-is-codegen)
for Apple, Android, or both. Its output is useful for inspection and
validation; CocoaPods and Gradle still regenerate the files they consume.

## Build libraries and Expo exports

Installing dependencies does not build a library or an application. Use
`javascript_build` for package scripts such as a library's `prepack` build:

```toml
[[target]]
name = "Library"
kind = "javascript_build"
srcs = ["src/**/*", "scripts/**/*", "tsconfig*.json", "babel.config.js"]
deps = ["./Dependencies"]

[target.attrs]
script = "prepack"
outputs = ["lib"]
```

`package_dir` selects a workspace member as the working directory. `outputs`
stay relative to the workspace root. Declare every source and configuration
file the script reads; only the declared products are cached.

An Expo workspace can export JavaScript and assets the same way, with
`script = "expo"`, `package_dir = "example"`,
`args = ["export", "--platform", "all", "--no-bytecode", "--output-dir", "dist"]`,
and `outputs = ["example/dist"]`, when its package manager supports `run` for
binaries. Some Expo versions embed random temporary filenames in Hermes
bytecode, so verify reproducibility before enabling bytecode. An export does
not produce an APK or an Apple application.

## Limitations

- `struo run` installs Apple simulator products only. Struo can build
  signed-device products but does not install them.
- Metro watches the application package and its locked dependencies. Packages
  outside that boundary need a project-specific Metro configuration.
- Expo projects without native directories need a separate prebuild step before
  the native application targets can build; Struo does not run it.
- Lifecycle scripts run with the package manager's normal permissions.

## Target kinds

- [`react_native_dependencies`](/reference/prelude/react_native_dependencies)
- [`react_native_autolinking`](/reference/prelude/react_native_autolinking)
- [`react_native_module`](/reference/prelude/react_native_module)
- [`react_native_codegen`](/reference/prelude/react_native_codegen)
- [`react_native_bundle`](/reference/prelude/react_native_bundle)
- [`react_native_metro`](/reference/prelude/react_native_metro)
- [`react_native_apple_application`](/reference/prelude/react_native_apple_application)
- [`react_native_android_application`](/reference/prelude/react_native_android_application)
- [`javascript_build`](/reference/prelude/javascript_build)

## Next steps

- [Apple](/guide/graph/apple): build native iOS targets alongside the app.
- [Android](/guide/graph/android): add native Android libraries and tests.
- [Ecosystems](/guide/graph/ecosystems): combine React Native with other
  toolchains in one graph.
