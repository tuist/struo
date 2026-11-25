# Rust SDK

Use the `struo` crate to store blobs and action results in a local or shared
Struo cache from Rust applications and tools, with async, bounded-memory APIs.

The crate exposes cache primitives only; script execution belongs to the
command line. See the [SDK overview](/guide/sdk/) to compare languages.

## Installation

Add the crate and an async runtime to your `Cargo.toml`:

```toml
[dependencies]
struo = { git = "https://github.com/tuist/struo" }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

The crate is released from the Struo repository for source integration. It is
not currently published to the crates.io registry.

## Quick start

Store a first blob in the default local cache and read it back:

```rust
#[tokio::main]
async fn main() -> struo::Result<()> {
    let cache = struo::Cache::new();
    let digest = cache.put_blob(b"hello").await?;
    let bytes = cache.get_blob(digest).await?;

    assert_eq!(bytes, b"hello");
    Ok(())
}
```

### Open a cache

`struo::Cache` is the cache client. It is cheap to clone and can be reused
across blob and action-result operations. Choose a constructor based on who
owns provider selection:

| API | Use |
| --- | --- |
| `Cache::new()` | Opens the [default local cache](/guide/sdk/#choose-a-cache). |
| `Cache::local(root)` | Opens an isolated local cache at a caller-owned root. |
| `Cache::from_workspace(root)` | Resolves the same effective provider as the command line for this workspace, including Tuist, object storage, HTTP cache, and retained Sprites cache providers. Returns a `Result`. |
| `Cache::with_provider(cache)` | Wraps an existing `CacheProvider`. |

To display diagnostics or pass the provider into code that already works with
content-addressed storage, use `provider()` for the underlying `CacheProvider`
and `root()` for its local root directory.

With a [Sprites cache binding](/guide/infrastructure/sprites), constructing a
workspace cache performs no provisioning. The first remote operation lazily
creates or reuses the user's retained cache Sprite. `SPRITES_TOKEN` (or
`STRUO_SPRITES_TOKEN`) is required; the SDK uses the same credentials directory
and retained cache as the command line. Local hits do not wake the remote
cache. Other constructors keep their existing local-only or explicit-provider
behavior.

## Blobs

Blobs are content-addressed byte payloads. Store bytes once, then refer to
them by digest from action results, manifests, or other integration state.

| API | Use |
| --- | --- |
| `put_blob(bytes)` | Stores bytes and returns their content digest. |
| `put_stream(reader)` | Stores an asynchronous reader with bounded memory use. |
| `put_file(path)` | Stores a file without loading it completely into memory. |
| `get_blob(digest)` | Reads bytes for a digest. |
| `get_blob_to_file(digest, path)` | Writes a blob to a file, creating parent directories, and returns its byte count. |
| `has_blob(digest)` | Returns whether a blob exists. |

Prefer the file and stream methods for logs, archives, compiler outputs, and
other payloads whose size is not tightly bounded.

## Action keys and results

`ActionKeyBuilder` gives cached automation a versioned, unambiguous identity.
Start with a namespace owned by the integration, then add labeled values and
content digests in deterministic order:

```rust
let source = cache.put_file("inputs/source").await?;
let mut key = struo::ActionKeyBuilder::new("example.compile");
key.push_bytes("tool", "compiler")
    .push_bytes("configuration", "release")
    .push_digest("source", source);
let action = key.finish();
```

Changing the namespace, a label, an input value, a digest, or input order
produces a different action key. Builder memory stays bounded regardless of
input size.

An action result associates an action digest with an exit code, stdout digest,
stderr digest, and output digests:

```rust
use std::collections::BTreeMap;

#[tokio::main]
async fn main() -> struo::Result<()> {
    let cache = struo::Cache::new();
    let stdout = cache.put_blob(b"compiled").await?;
    let mut key = struo::ActionKeyBuilder::new("example.compile");
    key.push_bytes("tool", "example");
    let action = key.finish();
    let result = struo::ActionResult {
        exit_code: 0,
        stdout: Some(stdout),
        stderr: None,
        outputs: BTreeMap::new(),
    };

    cache.put_action_result(action, &result).await?;
    let cached = cache.get_action_result(action).await?;

    assert_eq!(cached, Some(result));
    Ok(())
}
```

These methods store and retrieve metadata for a completed action. They do not
run commands. The caller executes the work, includes every relevant input in
the action key, and decides which blob digests belong in the result.

| API | Use |
| --- | --- |
| `put_action_result(action, result)` | Stores a cached result for an action digest. |
| `get_action_result(action)` | Returns a cached result when one exists. |
| `forget_action(action)` | Removes one cached action result and returns whether one existed. Referenced blobs are left intact. |
| `stats()` | Returns local cache statistics. |

## Stage blobs for read-only access

Use `cache.provider().stage_blob(&digest, parent).await` to prepare a blob for
read-only, seekable access without loading the whole payload into memory. The
parent directory must already exist and is owned by the caller. The returned
`StagedBlob` owns its temporary storage; `into_reader()` transfers that
ownership to a `StagedBlobReader` implementing synchronous `Read` and `Seek`.
Neither type exposes a mutable file handle or storage path. Dropping the lease
or its reader removes the temporary storage.

Staged blobs are verified before a reader is returned. A successful lease stays
readable even if the cache later removes the original blob. Corrupt local
content is removed; a remote provider refetches it once before staging fails.
Destinations written by `get_blob_to_file` remain independent, mutable copies.

`stage_blob_guarded(&digest, parent, guard)` additionally keeps a
`Send + 'static` guard alive while remote download and local decoding run. If
staging is cancelled, active workers hold the guard until they finish, so it
can be dropped on a worker thread after the calling future is gone. The
completed lease does not keep the guard.

When you already know a set of required digests,
`cache.provider().prefetch_blobs(&digests).await` fetches missing remote
content into the local tier before staging. A local-only provider does nothing.
Prefetch does not pin content, so later reads and staging still verify it and
recover from eviction or corruption. `prefetch_blobs_guarded(&digests, guard)`
keeps a `Send + 'static` guard alive through active downloads if the caller is
cancelled. Prefetch uses the provider's existing
[transfer budgets](/guide/sdk/#transfer-budgets).

## Write blobs from a producer

`cache.provider().put_produced(guard, producer)` stores the output of a
synchronous serializer without writing an intermediate file. The producer runs
on a blocking worker and writes sequential bytes through `std::io::Write`. Both
arguments must be `Send + 'static`. Use `()` as the guard when no lifetime
reservation is needed:

```rust
let digest = cache.provider().put_produced((), |writer| {
    writer.write_all(b"generated content")
}).await?;
```

No blob is published unless the producer and the final encoding succeed.
Dropping the future removes scratch files immediately and makes later writes
fail. Producers must bound their own buffers. Remote mirroring follows the same
size limit as `put_stream`.

If your integration already owns capacity for two workers, use
`put_produced_pipelined(guard, producer)` to overlap digest calculation with
encoding. Its guard must cover both workers. When that extra capacity is not
available, use `put_produced` instead.

## Errors and types

The crate root re-exports the types an embedder needs:

| Type | Use |
| --- | --- |
| `Digest` | Identifies blobs and action cache entries. |
| `ActionResult` | Stores an action exit code, stdout digest, stderr digest, and output digests. |
| `Stats` | Reports local cache size and entry counts. |
| `StagedBlob` | Owns temporary storage for read-only blob access. |
| `StagedBlobReader` | Reads and seeks within a staged blob while keeping its storage lease. |
| `CacheProvider` | Lower-level cache access for integrations that already own a provider. |
| `TuistCacheConfig` | Configures a manually constructed Tuist cache provider. |
| `ActionKeyBuilder` | Builds a versioned action digest from ordered, labeled inputs. |
| `Result<T>` | Result alias for every fallible SDK operation. |
| `Error` | Reports invalid digest strings and cache failures. |

Use `digest_from_hex(hex)` when a digest crosses a string boundary, such as
structured data, configuration, or another process. It accepts 64-character
lowercase BLAKE3 hexadecimal strings and returns `Error::InvalidDigest` for
anything else.

`Error::Cache` wraps a cache failure that can classify itself. Call
`is_not_found()` for confirmed absence, `is_unavailable()` for a remote
service that could not answer, and `is_corrupt()` for content that failed its
integrity check. `kind()` returns the full classification, which also
distinguishes refused credentials, malformed remote answers, configuration
problems, transfers larger than their budget, and local filesystem failures.
Only confirmed absence means the content is not cached; see
[misses, failures, and integrity](/guide/sdk/#misses-failures-and-integrity).

React to cache errors through `kind()` or the `is_*` methods rather than by
matching on the wrapped error's variants, which can change between releases.
Keep a wildcard arm when matching on `kind()`, because new kinds can be added
without a breaking release:

```rust
match cache.get_action_result(action).await {
    Ok(Some(result)) => replay(result),
    Ok(None) => run_and_store().await?,
    Err(struo::Error::Cache(error)) if error.is_unavailable() => run_without_cache().await?,
    Err(error) => return Err(error.into()),
}
```

## Next steps

- [SDK overview](/guide/sdk/): understand cache selection, retries, and
  transfer budgets shared by every SDK.
- [Swift SDK](/guide/sdk/swift): use the same cache from Apple platforms
  through the C library.
- [Infrastructure](/guide/infrastructure/): configure the remote cache that
  `Cache::from_workspace` resolves.
