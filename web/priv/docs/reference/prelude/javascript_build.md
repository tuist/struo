# `javascript_build`

Build JavaScript workspace packages through their locked package manager.

## Start Here

Declare a locked dependency target at the installation root, including all
workspace manifests, package-manager configuration, and lifecycle inputs. See
[React Native workspace dependencies](/guide/graph/react-native) for a complete
installation contract.

```toml
[[target]]
name = "Library"
kind = "javascript_build"
srcs = ["src/**/*", "tsconfig*.json", "scripts/**/*"]
deps = ["./Dependencies"]

[target.attrs]
script = "prepack"
outputs = ["lib"]
```

Run `struo build Library` to build the declared products. Installing dependencies
alone does not build the package.

## Attributes

- `script` (string): package script passed to the package manager's `run`
  command.
- `command` (string): installed package binary, such as `expo`, run through the
  package manager: `npm exec --no --`, `pnpm exec`, or `run` for Yarn and Bun.
  npm never downloads a binary the lockfile did not install. Set exactly one of
  `script` and `command`.
- `package_dir` (string, default `.`): working directory relative to the
  dependency installation root, even when this target is in a member manifest.
- `outputs` (required list of strings): product paths relative to the
  installation root, not `package_dir`. Paths must stay inside the staged
  workspace and cannot name its root.
- `args` (list of strings, default empty): additional script or command
  arguments. npm arguments are forwarded after `--`.
- `env` (map of strings, default empty): explicit non-secret environment values.
  With pnpm, Struo sets `pnpm_config_verify_deps_before_run` and
  `npm_config_verify_deps_before_run` to `false`, because the dependency target
  already installed the tree; set them in `env` to override.
- `node` (string, default `node`): Node.js executable.

Exactly one dependency must provide `javascript_dependency_set`. The provider
contract is described in [Starlark modules](/reference/modules).

## Sources and caching

Each build copies the complete installation into a private workspace, then
overlays `srcs` relative to the installation's original source root. Declare
all source files and configuration read by the script. Nested installations
and relative workspace links remain intact, and builds cannot mutate the
shared installation.

Sources, dependencies, arguments, environment, Node.js version, and the package
manager identity participate in the action cache key. Only declared products
are exposed through `products`. Successful actions can restore those products
without executing the package script again.

Package scripts retain their own shell semantics and must produce reproducible
outputs. An Expo export builds JavaScript and assets, not native application
binaries. See [library builds and Expo exports](/guide/graph/react-native) for
Hermes reproducibility and native prebuild boundaries.

## Runnable starter

The React Native starter includes a TypeScript library build alongside its
platform application targets:

```sh
struo query example javascript_build react-native-application-minimal --format json
```

The library build requires Node.js and the configured package manager. Native
application targets additionally require the corresponding platform toolchain.
