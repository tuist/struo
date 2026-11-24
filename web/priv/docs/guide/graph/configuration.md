# Build Configurations

Build the same Struo target for different platforms, architectures, or build
modes with workspace defaults, `--config` overrides, and `select`, each with
its own cached outputs.

The manifest stays the same; the configuration decides which `select` branch
a target uses and which platform and tokens its target kind builds for. The
effective configuration is part of a target's identity, so two configurations
never share outputs or cache entries.

## Declare workspace defaults

The root `struo.toml` sets the configuration every invocation starts from:

```toml
[workspace.configuration]
os = "linux"
arch = "arm64"
tokens = ["release"]
```

Struo normalizes common names (`darwin` to `macos`, `arm64` to `aarch64`,
`amd64` to `x86_64`), derives selection tokens from the operating system and
architecture, then appends the extra `tokens` and `default`. Inspect the
resolved set:

```sh
struo query workspace
```

The [manifest reference](/reference/manifest) describes the table and
normalization in full.

## Override per invocation

Pass `--config KEY=VALUE` to `struo build`, `struo lint`, `struo run`, or
`struo test` to change the configuration for one invocation. Recognized keys
are `os`, `arch`, and `token`:

```sh
struo build apps/Support --config os=ios --config arch=arm64
struo build apps/Support --config token=simulator
```

`os` and `arch` replace the workspace defaults, and each `token` is appended to
the selection tokens. When an override changes the operating system or
architecture, Struo derives the platform tokens again so stale tokens do not
survive. Only the resulting values affect target identity, not the flags that
produced them.

Building the same target under two configurations from one checkout keeps
both results:

```sh
struo build apps/Support                         # default configuration
struo build apps/Support --config token=release  # separate, scoped outputs
```

Repeating an invocation with the same overrides reports a cache hit, and an
invocation without overrides is unaffected.

## Choose values with select

Configurable attributes and dependencies use `select` to vary by token. A
target can choose its dependencies for the configured platform:

```toml
[target.deps.select]
macos = ["./apple_support"]
linux = ["./linux_support"]
default = ["./portable_support"]
```

An attribute can do the same inline:

```toml
[target.attrs]
sdk_frameworks = { select = { ios = ["UIKit"], macos = ["AppKit"] } }
```

When several branches match, the most specific one wins, and `default` is the
fallback. `struo query schema <kind>` shows which attributes are configurable
and any extra tokens the ecosystem understands. Each ecosystem guide, such as
[Apple](/guide/graph/apple), describes its tokens and restrictions in a
"Choose values by configuration" section.

## Next steps

- [Typed Build Graph](/guide/graph/): declare targets and dependencies to
  configure.
- [Manifest reference](/reference/manifest): see every
  `[workspace.configuration]` field.
- [Apple](/guide/graph/apple): choose values for iOS, macOS, simulators, and
  devices.
