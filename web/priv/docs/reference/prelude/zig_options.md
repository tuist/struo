# `zig_options`

Generates a Zig module of build-time constants. It is the Struo counterpart of
a `build.zig` options step, `b.addOptions()`, and downstream Zig targets import
it like any other module.

## Attributes

| Attribute | Type | Required | Default | Description |
| --- | --- | --- | --- | --- |
| `import_name` | string | no | target name | Name downstream modules import, such as `build_options` |
| `options` | map | no | `{}` | Constants to declare |
| `option_types` | map&lt;string, string&gt; | no | `{}` | Zig types that override an option's inferred type |
| `output_name` | string | no | `options.zig` | Generated file name inside the target output directory |

Without an `option_types` entry, an option's Zig type is inferred from its value:

| Value | Zig type |
| --- | --- |
| boolean | `bool` |
| integer | `comptime_int` |
| string | `[]const u8` |
| list of strings | `[]const []const u8` |

Set `option_types` to give an option a narrower type, such as `u16`. An
optional type with a leading `?` and no value in `options` declares the option
as `null`. Any option name works, including Zig keywords such as `type`,
because names are written as quoted identifiers, `@"type"`.

## Providers

- `zig_module`

## Example

```toml
[[target]]
name = "build_options"
kind = "zig_options"

[target.attrs.options]
greeting = "hello"
enabled = true

[target.attrs.option_types]
port = "?u16"

[[target]]
name = "options_test"
kind = "zig_test"
srcs = ["src/options_test.zig"]
deps = ["build_options"]

[target.attrs]
main = "src/options_test.zig"
strip = true
```

The test imports the module with `@import("build_options")`.
