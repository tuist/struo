# Ruby SDK

Use the `struo` gem to store blobs and action results in a local or shared
Struo cache from Ruby applications and tools, with synchronous methods.

The gem exposes cache primitives only; script execution belongs to the command
line. See the [SDK overview](/guide/sdk/) to compare languages.

## Installation

Install the gem from RubyGems:

```sh
gem install struo
```

The gem requires Ruby 3.1 or later and includes prebuilt native Struo
libraries for supported platforms. Set `STRUO_LIBRARY_PATH` when you need to
load a custom `libstruo` build.

## Quick start

Store a first blob in the default local cache and read it back:

```ruby
require "struo"

cache = Struo::Cache.new
digest = cache.put_blob("hello")
bytes = cache.get_blob(digest)

raise unless bytes == "hello"
```

### Open a cache

| API | Use |
| --- | --- |
| `Struo::Cache.new` | Opens the [default local cache](/guide/sdk/#choose-a-cache). |
| `Struo::Cache.new(local_cache_root:)` | Opens an isolated local cache at a caller-owned root. |
| `Struo::Cache.new(workspace_root:)` | Resolves the same effective provider as the command line for a workspace. |
| `version` | Returns the linked Struo version. |

## Blobs

Blobs are content-addressed byte payloads. All methods are synchronous. When
`bytes` is a string, the gem uses the string's byte representation.

| API | Use |
| --- | --- |
| `digest(bytes)` | Returns the content digest for bytes without writing them to the cache. |
| `put_blob(bytes)` | Stores bytes and returns their content digest. |
| `put_file(path)` | Stores a file without loading its complete contents into Ruby memory. |
| `get_blob(digest)` | Reads bytes for a digest as a binary string. |
| `get_blob_to_file(digest, path)` | Writes a blob to a file and returns its byte count. |
| `has_blob?(digest)` | Returns whether a blob exists. |

Prefer `put_file` and `get_blob_to_file` for logs, archives, compiler outputs,
and other payloads whose size is not tightly bounded.

## Action keys and results

`Struo::ActionKey` builds a versioned identity from ordered, labeled inputs.
Input order is significant and must be deterministic:

```ruby
source = cache.put_file("inputs/source")
action_digest = Struo::ActionKey.new("example.compile")
                               .add_bytes("tool", "compiler")
                               .add_digest("source", source)
                               .digest
```

Store and read the result of an action you ran yourself. The gem does not run
commands:

```ruby
stdout_digest = cache.put_blob("compiled")
result = Struo::ActionResult.new(exit_code: 0, stdout: stdout_digest)
cache.put_action_result(result, action_digest: action_digest)
cached = cache.get_action_result(action_digest)
```

| API | Use |
| --- | --- |
| `put_action_result(result, action_digest:)` | Stores a cached result for an action digest. |
| `get_action_result(action_digest)` | Returns a cached `Struo::ActionResult`, or `nil` when none exists. |
| `forget_action(action_digest)` | Removes one cached action result and returns whether one existed. Referenced blobs are left intact. |
| `stats` | Returns local cache statistics. |

## Errors and types

Failures raise `Struo::Error`. Passing both `local_cache_root:` and
`workspace_root:` raises `ArgumentError`. A missing action result returns
`nil`; a remote service that cannot answer raises instead of reporting a miss.
See [misses, failures, and integrity](/guide/sdk/#misses-failures-and-integrity).

`Struo::ActionResult` holds cached action metadata:

```ruby
Struo::ActionResult.new(
  exit_code: 0,
  stdout: stdout_digest,
  stderr: nil,
  outputs: {},
)
```

`stats` returns `Struo::CacheStats` with `blob_count`, `blob_bytes`,
`action_count`, and `action_bytes`.

## Next steps

- [SDK overview](/guide/sdk/): understand cache selection, retries, and
  transfer budgets shared by every SDK.
- [Infrastructure](/guide/infrastructure/): configure the remote cache that a
  workspace-bound client resolves.
- [C SDK](/guide/sdk/c): see the native library the gem loads.
