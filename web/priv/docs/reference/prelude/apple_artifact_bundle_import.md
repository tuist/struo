# `apple_artifact_bundle_import`

Prebuilt Swift Package Manager artifact bundle import.

## Description

Selects one variant of a static library `.artifactbundle` and exposes it to
Apple consumers as a static-library dependency. The bundle's `info.json` is
read to find the `staticLibrary` artifact and the variant whose
`supportedTriples` match the destination. Matching follows Swift Package
Manager: architecture, vendor, operating system, and environment must agree,
and operating system versions are ignored, so `arm64-apple-macosx` matches a
macOS build for `arm64`.

The selected variant's archive is linked into the final consumer. The headers
listed in its `staticLibraryMetadata.headerPaths` and its module map are
exported to downstream compilation, so Swift sources can `import` the module.
The module name defaults to the module declared by that module map.

When a Swift package declares a binary target whose path or download URL names
an `.artifactbundle`, such as `CHegel-0.41.0.artifactbundle.zip`, its native
integration lowers the target to this kind. A downloaded archive is fetched by
a generated `archive_download` dependency, and `bundle` then names the
extracted directory that contains the bundle.

## Attributes

| Attribute | Type | Required | Default | Description |
| --- | --- | --- | --- | --- |
| `bundle` | string | yes |  | Workspace-relative `.artifactbundle` directory, or a directory that contains exactly one, such as an extracted archive |
| `platform` | string | yes |  | Apple platform whose variant is imported |
| `sdk_variant` | string | no | `simulator` | `simulator` or `device` variant selection; ignored on macOS |
| `arch` | string | no | host architecture | Architecture the variant must support |
| `module_name` | string | no | module map's module | Clang module name |

`platform`, `sdk_variant`, `arch`, and `module_name` are not configurable by
platform select.

## Providers

The target emits `apple_linkable`, `apple_framework`, and `apple_bundle`, so
applications, frameworks, libraries, executables, and test bundles can depend
on it through ordinary `deps` entries.

The optional `deps` edge accepts one `artifact` provider. It lets a generated
`archive_download` dependency materialize a remote bundle before this target is
analysed.

## Capabilities

| Capability | Output groups |
| --- | --- |
| `build` | `default`, `framework` |

## Limitations

Only `staticLibrary` artifacts are supported. Resolution fails with a focused
error when no bundle with `info.json` is found, when the bundle has no
`staticLibrary` artifact, when no variant supports the destination triple (the
error lists the triples the bundle offers), or when the selected library is
missing. One target imports one variant, so building for both a simulator and
a device means selecting the variant through the target's attributes.

## Example

```toml
[[target]]
name = "Example"
kind = "apple_artifact_bundle_import"
srcs = ["Example.artifactbundle/info.json"]

[target.attrs]
bundle = "Example.artifactbundle"
platform = "macos"
arch = "arm64"
```
