# Build Artifacts

Package build outputs such as applications, command-line tools, and static sites
as immutable artifacts, export them, and publish them to local or remote
infrastructure.

Struo builds, packages, and publishes the content; the provider decides how
people download, install, serve, or run it. An artifact can also be a server,
dataset, or report. No container runtime or running web server is required.

## Declare an artifact

The `artifact` target kind packages selected files. This target packages a
static site and declares how it can be served:

```toml
[[target]]
name = "site"
kind = "artifact"
srcs = ["public/**/*"]

[target.attrs.interfaces."dev.struo.static-http.v1"]
payload = "content"

[target.attrs.interfaces."dev.struo.static-http.v1".spec]
root = "public"
index = "index.html"
```

`artifact` creates a deterministic ZIP with package-relative paths. Its
`file_mode` defaults to 420 (0644); use 493 (0755) for executable files.
Permissions are declared, not inferred from the machine that packages them.
Custom target kinds can export existing file outputs directly; see the
[module contract](/reference/modules/#artifact-providers).

## Export and publish

Inspect what a target exports, write it to a directory, or publish it:

```sh
struo query artifacts site
struo artifact export site --output dist
struo artifact publish site --provider local
```

Export writes a self-contained directory and returns its immutable root
descriptor. Local publication keeps the package separately from the action cache
and writes a publication receipt once all content is present. Repeating it
repairs missing or changed content.

## Choose publication infrastructure

Select the provider that receives publications in the root `struo.toml`:

```toml
[infrastructure.artifacts]
name = "local"
```

`artifacts` is the capability being configured; `name` selects its provider.
The built-in `local` provider needs no separate definition. Without this
binding, `struo artifact publish` fails unless `--provider` selects one.
`--track` adds distribution context without rebuilding different content.

Cache, execution, and artifact bindings are independent. A cache hit does not
skip publication, and publication does not run the published content.
Provider-specific configuration, authentication, supported formats, and limits
live in each provider's guide. [Tuist](/guide/infrastructure/tuist) receives
artifacts and turns supported applications into previews.

## Publish after successful builds

List the targets to publish automatically in the root `struo.toml`:

```toml
[infrastructure]
publish_on_build = ["application"]
```

Then publish once explicitly and authorize the configured destination for
automatic publication:

```sh
struo artifact publish application --allow-on-build
struo build application
```

After all requested builds succeed, Struo publishes requested targets listed in
`publish_on_build`. The producing target kind must also approve automatic
publication through `struo.artifact_info.v2`; the older v1 declaration never
approves it.

Authorization is local machine state, not repository configuration. It covers
this checkout's path, the target, and the exact service destination. Moving the
checkout, using another machine, or changing the destination requires
authorizing again. Without the policy or the authorization, builds make no
publication network requests.

Remote publication uploads the full artifact set and is idempotent: repeating
the same publication returns the existing receipt without uploading content
again. If Struo cannot confirm completion it reports an error; rerun the same
command to recover the receipt. Different targets, tracks, or revisions produce
separate publications even when their content is equal.

## Describe how content is consumed

Each artifact has named payloads with a SHA-256 digest, size, and media type.
Interfaces bind a payload to a versioned consumption contract with `payload`
and `spec` fields:

- **Download:** no interface. Consumers retain the bytes.
- **Static site:** `dev.struo.static-http.v1` declares the archive `root` and
  `index`. Consumers never serve files outside the root.
- **CLI, server, or application:** a producer-owned contract declares the
  entrypoint, platform, ports, or installation requirements a consumer needs.

Interfaces are declarations. Unknown interfaces survive export and stay
downloadable, but never grant permission to run a command. Target paths, Git
revisions, tracks, credentials, and infrastructure URLs are not part of an
artifact's content identity.

Exported packages contain `index.json` and `blobs/sha256/<digest>` files. The
root describes a `once.artifact_set.v1` document whose members refer to
`once.artifact.v1` manifests, which refer to payloads. Consumers verify every
descriptor's digest and size before trusting content.

## Use artifacts from an agent

The MCP server exposes the same operations:

- `struo_query_artifacts` inspects capability support and the provider shape.
- `struo_export_artifacts` mirrors local export.
- `struo_publish_artifacts` mirrors explicit publication.

Export and publish require `struo mcp --allow-run`. Authorize the destination
before asking an agent to publish.

## Publication service contract

A remote artifact provider exposes an authenticated HTTP endpoint. The protocol
never interprets application metadata:

- `GET <endpoint>` returns `{"schema":"once.publication_service.v1","destination":"<stable destination>"}`.
- `GET <endpoint>/publications/<key>` returns a receipt, 404 for an unknown
  intent, or 410 for an expired intent that cannot be reactivated.
- `POST <endpoint>/publications/<key>/content/<digest>` streams exact bytes as
  `application/octet-stream`. The receiver verifies SHA-256 before acknowledging
  with 204. A committed intent is immutable and rejects further uploads with 409.
- `POST <endpoint>/publications/<key>` commits this JSON intent only after the
  complete reachable closure is durably retained:

```json
{
  "schema": "once.publication_intent.v1",
  "root": {"digest": "sha256:<64 lowercase hex digits>", "size_bytes": 123, "media_type": "application/vnd.struo.artifact-set.v1+json"},
  "target": "application",
  "track": null,
  "revision": null
}
```

`key` is `sha256:` plus the SHA-256 of this complete intent serialized as
compact UTF-8 JSON with recursively sorted object keys. Both optional values
are present as strings or `null`; descriptor sizes are nonnegative integers.
A successful commit or lookup returns exactly `schema`, `key`, `root`,
`destination`, and `url`, with schema `once.publication_receipt.v1`. The URL is
absolute HTTPS, except same-origin HTTP for local development, and contains no
embedded credentials.

Struo uploads each distinct exported digest, never files the server selects.
Responses are limited to one mebibyte, and redirects are not followed. Other
statuses and malformed or mismatching receipts are errors. An upload conflict
succeeds only if a separate lookup confirms the same publication. The client
does not retry ambiguous writes automatically.

## Limitations

- Struo does not host static sites, install command-line tools, or start
  servers. A compatible consumer validates the contract, unpacks safely,
  isolates untrusted content, binds secrets, and owns routing, access,
  retention, and cleanup.
- Local publications live in the workspace and are deleted with it. Export to
  an external destination for longer retention.
- Retention and expiry of remote publications are receiver policies, not a
  promise of permanent availability.

## Next steps

- [Triggers](/guide/infrastructure/triggers), to choose which targets CI
  builds and tests when a branch changes.
- [Tuist](/guide/infrastructure/tuist), to publish applications as installable
  previews.
- [`struo artifact` reference](/reference/cli/artifact), for every export and
  publish option.
