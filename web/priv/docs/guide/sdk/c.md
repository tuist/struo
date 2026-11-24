# C SDK

Use `libstruo` and `struo.h` to store blobs and action results in a Struo cache
from C or any language with a C foreign-function interface, using JSON requests.

The C library is the lowest-level SDK; the Swift, Ruby, and Node.js SDKs are
built on it. Script and graph execution remain command-line features. See the
[SDK overview](/guide/sdk/) to compare languages.

## Installation

Build or obtain the shared or static `libstruo` library, then use `struo.h`
from the matching tagged source distribution. Add both to the host build
system: put `struo.h` on the include path and link against `libstruo`.

## Quick start

This complete example stores `hello` in the default local cache and prints the
response:

```c
#include <struo.h>
#include <stdio.h>

int main(void) {
    const char *request = "{\"bytes\":[104,101,108,108,111]}";
    char *response = struo_cache_put_blob_json(request);

    puts(response);
    struo_string_free(response);
    return 0;
}
```

The response contains the blob's digest in `value`. Pass it as `digest` to
`struo_cache_get_blob_json` to read the bytes back.

### Open a cache

Omit cache-selection fields to use the
[default local cache](/guide/sdk/#choose-a-cache). Add at most one of these
optional fields to any cache request:

| Field | Use |
| --- | --- |
| `local_cache_root` | Opens an isolated local cache at this path. |
| `workspace_root` | Resolves the same effective provider as the command line for this workspace. |

`struo_version` returns the linked Struo version.

## Ownership and responses

Every `struo_*` function that returns `char *` returns an owned UTF-8 string.
Release it with `struo_string_free`, including error responses. All functions
are thread-safe and reentrant.

Requests and responses use [JSON](https://www.json.org/json-en.html).
Responses have one of these shapes:

```json
{ "status": "ok", "value": "..." }
```

```json
{ "status": "error", "message": "..." }
```

## Blobs

| Function | Use |
| --- | --- |
| `struo_digest_bytes` | Hashes a byte buffer without touching storage. A null pointer is accepted only with length zero. |
| `struo_cache_put_blob_json` | Stores the `bytes` array from a request. |
| `struo_cache_put_file_json` | Stores the file at `path` without encoding its bytes in the request. |
| `struo_cache_get_blob_json` | Returns the blob for `digest` as a `bytes` array. |
| `struo_cache_get_blob_to_file_json` | Writes the blob for `digest` to `path` and returns its byte count. |
| `struo_cache_has_blob_json` | Returns whether `digest` exists. |

Use the file functions for payloads whose size is not tightly bounded.

## Action keys and results

`struo_action_key_json` accepts a namespace and ordered inputs. Input order is
significant and must be deterministic:

```json
{
  "namespace": "example.compile",
  "inputs": [
    { "kind": "bytes", "label": "tool", "bytes": [99, 111, 109, 112, 105, 108, 101, 114] },
    { "kind": "digest", "label": "source", "digest": "<64 lowercase hexadecimal characters>" }
  ]
}
```

Pass the returned digest as `action_digest` to the action-result functions:

| Function | Use |
| --- | --- |
| `struo_cache_put_action_result_json` | Stores the `result` object under `action_digest`. |
| `struo_cache_get_action_result_json` | Returns action metadata, or `null` when none exists. |
| `struo_cache_forget_action_json` | Removes action metadata without deleting referenced blobs and returns whether it existed. |
| `struo_cache_stats_json` | Reports statistics for the local cache tier. |

## Errors and types

Failures, including invalid JSON, invalid digests, and cache errors, return the
error response shape with a `message`. A missing action result returns `null`
as the `value`; a remote service that cannot answer returns an error instead.
See [misses, failures, and integrity](/guide/sdk/#misses-failures-and-integrity).

An action result uses snake case. `stdout`, `stderr`, and `outputs` are
optional:

```json
{
  "exit_code": 0,
  "stdout": "<stdout digest>",
  "outputs": { "out/app": "<output digest>" }
}
```

`struo_cache_stats_json` returns `blob_count`, `blob_bytes`, `action_count`,
and `action_bytes`.

## Next steps

- [SDK overview](/guide/sdk/): understand cache selection, retries, and
  transfer budgets shared by every SDK.
- [Swift SDK](/guide/sdk/swift): use a typed async wrapper over this library
  on Apple platforms.
- [Infrastructure](/guide/infrastructure/): configure the remote cache that
  `workspace_root` resolves.
