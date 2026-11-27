# `go_module`

Represents one checksum-bound Go module.

`go_dependencies` and [`go_workspace`](/reference/prelude/go_workspace)
generate these targets. A module comes from a vendor directory, from a local
replacement directory, or, for native discovery, from a snapshot of the host's
Go module cache. They remain visible through target,
dependency, and schema queries, but should not be declared manually.

## Attributes

| Attribute | Type | Required | Description |
| --- | --- | --- | --- |
| `module_path` | string | yes | Canonical Go module path |
| `version` | string | no | Selected module version |
| `checksum` | string | no | Source checksum from a declared Go checksum file |
| `replacement_path` | string | no | Replacement module or local path |
| `replacement_version` | string | no | Replacement module version |
| `source_root` | string | yes | Package-relative vendored module directory |
| `packages` | list&lt;string&gt; | no | Vendored import paths owned by this module |
| `_go_locked` | bool | resolver-owned | Proves that `go_dependencies` generated the target |
| `_go_cache_module` | string | resolver-owned | Escaped `path@version` directory in the host module cache that native discovery snapshots into `source_root` |
| `_go_source_files` | list&lt;string&gt; | resolver-owned | Workspace-relative files of a vendored or locally replaced module found by native discovery |

## Providers and Capabilities

The target emits `go_module` and exposes `build` with no output group. Its
provider includes the complete source tree as transitive build inputs.

