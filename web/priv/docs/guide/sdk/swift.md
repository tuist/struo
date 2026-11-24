# Swift SDK

Use the Struo Swift SDK to store blobs and action results in a local or shared
Struo cache from apps and tools on Apple platforms, with async APIs.

The SDK is a thin asynchronous wrapper over the C library in the
`Struo.xcframework` release artifact. It exposes cache primitives only; script
execution belongs to the command line. See the [SDK overview](/guide/sdk/) to
compare languages.

## Installation

Reference the released XCFramework from your package manifest with a
[Swift Package Manager](https://www.swift.org/documentation/package-manager/)
binary target:

```swift
// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "MyPackage",
    products: [
        .library(name: "MyPackage", targets: ["MyPackage"]),
    ],
    targets: [
        .target(
            name: "MyPackage",
            dependencies: ["Struo"]
        ),
        .binaryTarget(
            name: "Struo",
            url: "https://github.com/tuist/struo/releases/download/0.1.0/Struo.xcframework.zip",
            checksum: "<checksum>"
        ),
    ]
)
```

Replace the version in the download address with the Struo release you want to
use. The checksum is published next to the release asset and can also be
computed locally with `swift package compute-checksum Struo.xcframework.zip`.

Vendor the `Struo.swift` wrapper that ships with the matching release tag into
the Swift target that depends on `Struo`. The wrapper imports the C module from
the binary target and gives callers the high-level Swift interface.

## Quick start

Store a first blob in the default local cache and read it back:

```swift
import Foundation
import Struo

func example() async throws {
    let cache = StruoCache()
    let digest = try await cache.putBlob(Data("hello".utf8))
    let bytes = try await cache.getBlob(digest)

    assert(bytes == Data("hello".utf8))
}
```

### Open a cache

`StruoCache` is the cache client. Choose the initializer based on who owns
provider selection. All operations that touch storage are `async throws`.

| API | Use |
| --- | --- |
| `init()` | Opens the [default local cache](/guide/sdk/#choose-a-cache). |
| `init(localCacheRoot:)` | Opens an isolated local cache at a caller-owned file URL. |
| `init(workspaceRoot:)` | Resolves the same effective provider as the command line for a workspace file URL. |
| `version` | Returns the linked Struo version. |

## Blobs

Blobs are content-addressed byte payloads. Store bytes once, then refer to
them by digest from action results, manifests, or other integration state.
Digests are lowercase hexadecimal strings.

| API | Use |
| --- | --- |
| `digest(bytes:)` | Returns the content digest for bytes without writing them to the cache. |
| `putBlob(_:)` | Stores bytes and returns their content digest. |
| `putBlob(contentsOf:)` | Stores a file without loading its complete contents into Swift memory. |
| `getBlob(_:)` | Reads bytes for a digest. |
| `getBlob(_:writeTo:)` | Writes a blob to a file URL and returns its byte count. |
| `hasBlob(_:)` | Returns whether a blob exists. |

Prefer the file methods for logs, archives, compiler outputs, and other
payloads whose size is not tightly bounded.

## Action keys and results

`StruoActionKey` builds a versioned identity from ordered, labeled inputs.
Input order is significant and must be deterministic:

```swift
let cache = StruoCache(workspaceRoot: workspaceURL)
let source = try await cache.putBlob(contentsOf: sourceURL)
var key = StruoActionKey(namespace: "example.compile")
key.add(bytes: Data("swiftc".utf8), label: "compiler")
key.add(digest: source, label: "source")
let actionDigest = try await key.digest()
```

An action result associates an action digest with an exit code, stdout digest,
stderr digest, and output digests. The SDK stores and retrieves metadata for
completed actions; it does not run commands.

```swift
let stdout = try await cache.putBlob(Data("compiled".utf8))
let result = StruoActionResult(exitCode: 0, stdout: stdout)
try await cache.putActionResult(result, for: actionDigest)
let cached = try await cache.getActionResult(actionDigest)
```

| API | Use |
| --- | --- |
| `putActionResult(_:for:)` | Stores a cached result for an action digest. |
| `getActionResult(_:)` | Returns a cached result, or `nil` when none exists. |
| `forgetAction(_:)` | Removes one cached action result and returns whether one existed. Referenced blobs are left intact. |
| `stats()` | Returns local cache statistics. |

## Errors and types

| Type | Use |
| --- | --- |
| `StruoActionResult` | Stores `exitCode`, optional `stdout` and `stderr` digests, and an `outputs` map of digests. |
| `StruoActionKey` | Builds a versioned action digest from ordered, labeled inputs. |
| `StruoCacheStats` | Reports `blobCount`, `blobBytes`, `actionCount`, and `actionBytes` for the local tier. |
| `StruoError` | Reports failures: `.api(message)` for an error returned by Struo, and `.invalidUTF8` when a request or response cannot be encoded. |

A missing action result returns `nil`. A remote service that cannot answer
throws instead of reporting a miss; see
[misses, failures, and integrity](/guide/sdk/#misses-failures-and-integrity).

## Next steps

- [SDK overview](/guide/sdk/): understand cache selection, retries, and
  transfer budgets shared by every SDK.
- [C SDK](/guide/sdk/c): see the request and response format the Swift
  wrapper is built on.
- [Infrastructure](/guide/infrastructure/): configure the remote cache that a
  workspace-bound client resolves.
