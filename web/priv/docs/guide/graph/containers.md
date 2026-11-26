# Container Images

Build container images with Struo from an existing Dockerfile or from native executables, cache each instruction, and load or publish the result.

Struo supports two ways to build an image, and they combine:

- `dockerfile_image` builds an existing Dockerfile. Struo discovers it without
  a manifest, runs each instruction as a separate cached action through
  [BuildKit](https://docs.docker.com/build/buildkit/), and falls back to one
  whole-file BuildKit build for syntax it cannot split.
- `oci_layer` and `oci_image` package executables built by Struo into an image
  without a Dockerfile or Docker.

## Prerequisites

- `dockerfile_image` needs Docker with Buildx, installed and running.
- `oci_pull` and `oci_push` need [`crane`](https://github.com/google/go-containerregistry/tree/main/cmd/crane),
  not Docker. Install it with `mise use crane`.
- `oci_load` needs `docker` or `podman`.
- `oci_layer`, `oci_image`, `oci_index`, and `oci_import` need no container
  tools.

## Start from an existing project

### Build an existing Dockerfile

Struo discovers a target named `image` in each directory containing a
`Dockerfile`. The Dockerfile's directory is the build context, including the
files referenced by `COPY`. From the directory containing the `Dockerfile`:

```sh
struo build image
```

From a parent workspace, use the relative package path. For example, if the
file is `server/Dockerfile`, run:

```sh
struo build server/image
```

Each `Dockerfile.<name>` and `<name>.Dockerfile` is discovered too, as a target
named `<name>_image`. Its directory is the build context, so a monorepo that
keeps one Dockerfile per app at the root builds each app with the root as the
context, as `docker build -f Dockerfile.web .` does. A `Dockerfile.web.dockerignore`
beside it applies to that image only, ahead of the directory's `.dockerignore`:

```sh
struo build web_image
```

`struo query targets` lists the discovered target names. A bare `struo build`
does not select Dockerfile images; pass the target name.

Struo uses the current Buildx builder when it supports image-layout exports.
If Docker's built-in builder is the implicit default, Struo creates and reuses
a dedicated container builder for the current Docker endpoint, leaving your
selected builder unchanged. Set `BUILDX_BUILDER` to choose an existing
container or remote builder.

### Configure an image target

Add `struo.toml` only when you need a different Dockerfile name, context,
platform, tag, or caching policy. Both paths are relative to the package that
contains the target:

```toml
[[target]]
name = "server_image"
kind = "dockerfile_image"

[target.attrs]
dockerfile = "server/Dockerfile"
context = "."
platform = "linux/arm64"
tag = "service:latest"
```

```sh
struo build server_image
```

The default output is a Docker archive that you can pass to `docker image load`.
Set `format = "oci"` for an [Open Container Initiative image archive](https://github.com/opencontainers/image-spec).
Set `target` to export an intermediate Dockerfile stage instead of the final
image. Build arguments belong in `build_args`; do not put credentials there.

### Review inputs that can change

```sh
struo lint server_image
```

Lint warns, with the instruction location and a suggested repair, about inputs
that can change without changing the cache key: unpinned base images and
`# syntax=` frontends, mutable package repositories, downloaded pipelines,
authentication mounts, and `COPY` sources missing from the build context
(often a sign that the Dockerfile expects a parent directory as its context).
The checks are advisory and do not change cache eligibility. A clean report is
not proof of reproducibility.

## Declare targets

### Build a native image

`oci_layer` packages executables and files into a deterministic layer, and
`oci_image` assembles layers into an image:

```toml
[[target]]
name = "server_layer"
kind = "oci_layer"

[target.dependencies]
programs = ["server"]

[[target]]
name = "server_image"
kind = "oci_image"

[target.dependencies]
base = ["base"]
layers = ["server_layer"]

[target.attrs]
tag = "registry.example.com/team/server:latest"
tags = ["registry.example.com/team/server:v1"]
```

Run `struo edit materialize-example oci_image oci-image-minimal` for a
complete starter that packages a Go binary.

On an `oci_layer`, set `annotations` to record them on the layer's descriptor,
`compress = "gzip"` to store the layer as a deterministic gzip stream, and
`symlinks = { "/usr/bin/app" = "/usr/local/bin/app" }` to add links.

A `base` dependency makes the new layers extend an existing image, keeping its
layers, environment, and history. Setting `entrypoint` replaces the inherited
entry point and clears the inherited `cmd` unless you also set `cmd`, as a
Dockerfile `ENTRYPOINT` does. An empty list clears a value; leaving an
attribute unset keeps the base's. A base for a different platform than the
image is an error, and a base pulled with `platform = "all"` supplies the
platform the image asks for.

### Combine platforms into one index

Build one image per platform and combine them:

```toml
[[target]]
name = "server_index"
kind = "oci_index"

[target.dependencies]
images = ["server_image_amd64", "server_image_arm64"]
```

Two images for the same platform are an error.

## Manage dependencies

### Pin a base image

`oci_pull` imports a registry image by digest. The digest is required, so the
image cannot change under you, and Struo caches the result.

### Use other targets in a Dockerfile

Declare dependencies instead of copying files into the build context:

```toml
[[target]]
name = "base"
kind = "oci_pull"

[target.attrs]
image = "docker.io/library/alpine"
digest = "sha256:25109184c71bdad752c8312a8623239686a9a2071e8825f20acb8f2198c3f659"

[[target]]
name = "server_image"
kind = "dockerfile_image"

[target.dependencies]
images = ["base"]
programs = ["server"]

[target.attrs]
context_names = { base = "alpine:3.23" }
```

With this target, `FROM alpine:3.23` in the Dockerfile uses the pinned image
from the `base` target, and `COPY --from=server /server /usr/local/bin/server`
copies the executable built by the `server` target. A context is named after
its target unless `context_names` renames it. No registry is contacted, and
these dependencies count as pinned when `cacheable` is set.

### Import an existing image archive

`oci_import` brings an image exported by another tool into the graph. Point
`archive` at a `.tar`, `.tar.gz`, or `.tgz` image layout, as written by
`docker save` (Docker 25 and later), `docker buildx build --output type=oci`,
or `crane pull --format oci`. Replacing the archive rebuilds everything that
depends on it. The image keeps the names recorded in the archive and can be a
`base` for `oci_image`, loaded with `oci_load`, or published with `oci_push`.

```toml
[[target]]
name = "base"
kind = "oci_import"

[target.attrs]
archive = "images/base.oci.tar"
```

Older `docker save` archives that carry only `manifest.json` are not layouts;
re-export them with a current Docker or `crane pull --format oci`.

## Cache Dockerfile builds

### Cache individual instructions

Dockerfile builds bypass the Struo cache by default. Set `cacheable = true`,
`pull = false`, and an explicit `platform` once you accept that the build's
remote inputs are immutable. Every base image of a built stage must then be
pinned by digest (`name@sha256:...`); otherwise the build fails, because a
moving tag would keep serving a stale image from the cache. Pinning base images
is not enough if `RUN` downloads changing packages; keep caching disabled for
those recipes.

With caching on, each instruction is its own cached action. A changed source
copied late in the recipe does not invalidate earlier instructions, and only
the stages the selected image needs are built. Literal `COPY` sources are
tracked individually; wildcard sources, variable sources, and context bind
mounts track the whole context. Mutable cache mounts stay on the BuildKit
worker and are not cached by Struo.

Files keep their permission bits, and changing a file's mode invalidates the
step that copies it. Changing only a directory's mode does not; set the mode
with `COPY --chmod` or `RUN chmod` to force a rebuild.

### Get reproducible layers

Files in the final archive keep the timestamps from when their instruction ran.
`source_date_epoch` fixes the image creation time and history. Set
`reproducible_layers = true` for layer bytes that are identical across
independent rebuilds: each instruction rewrites its layer's timestamps to
`source_date_epoch`. Later instructions then see normalized timestamps, which
can change what timestamp-driven tools such as `make` decide to rebuild.

### Choose whole-file mode

The default `execution_mode = "auto"` switches to one whole-file BuildKit
action when the Dockerfile uses heredocs, `ONBUILD`, a `# syntax=` frontend, a
custom escape character, several variables in one `ARG`, or extended
`${VAR:-default}` expansions. It also switches when `export_cache`, `cache_to`,
or a `caches` dependency is set, or when a base image declares `ONBUILD`
triggers. `struo lint image` reports a `whole_file_execution` note naming the
reason. Lint and queries work on every Dockerfile.

Set `execution_mode = "buildkit"` to always build the complete Dockerfile as
one action, or `execution_mode = "instructions"` to fail with the line number
instead of switching. Use `"buildkit"` when Struo cannot reach the local Docker
engine or the registry to check a base image for `ONBUILD` triggers. Also use
it, with Docker's built-in builder and `pull = false`, when a base image exists
only in the local Docker image store, which the automatically created builder
cannot see. Otherwise, publish the base image to a registry and reference it by
digest.

### Share BuildKit layer caches

Whole-file mode supports portable BuildKit layer caches, which BuildKit reuses
even when Struo must run the build. Set `export_cache = true` to keep
intermediate layers as a declared output, and import it in another image
target with `[target.dependencies] caches = ["base_image"]`.

For registry-backed sharing, set `cache_from` and `cache_to` to lists of image
references. Registry exports keep all intermediate stages and need registry
credentials. Keep `cacheable` off when publishing a registry cache.

### Check cache restoration

A successful build does not prove Struo cached it. A result with
`cache = "bypass"` still runs BuildKit, which can reuse its own layers. To
check restoration, delete only the target's generated outputs in an isolated
workspace, build again, and compare the archive checksum. Do not prune a shared
Docker cache for this check.

To compare independent builds, run:

```sh
struo build server_image --verify-reproducible
```

It compares every declared output, not only the image archive. Buildx metadata
files contain per-build references and can differ even when the image archive
matches, so inspect the reported paths before concluding the image bytes
differ. To compare by hand, use separate empty Struo caches and set
`no_cache = true` to disable BuildKit's instruction cache too.

## Build on a remote worker

Provision a [remote BuildKit builder](https://docs.docker.com/build/builders/drivers/remote/)
and set its name in `builder`. Buildx sends the declared context to the worker
and returns the image to Struo. Configure authentication and memory limits on
the worker; Struo's local memory limit covers only the client process.

## Load and publish

```sh
struo run server_load    # oci_load: docker load, or podman load; list several images to load a bundle
struo run server_push    # oci_push: publish the image or index
```

`oci_push` sends the exact bytes Struo built, so the registry digest equals the
digest of the local image or index. `crane` performs the transfer and reads
credentials from your Docker configuration. Set `insecure = true` for a
registry without TLS.

Set `sign = true` to sign the pushed digest with
[cosign](https://docs.sigstore.dev/cosign/signing/overview/). `cosign_key` names
a key file or key management URI; leave it empty for keyless signing.
`cosign_args` passes extra flags to `cosign sign`.

## Limitations

- Dockerfile builds are not cached by Struo unless `cacheable` is set with the
  requirements above.
- Contents of mutable cache mounts stay on the BuildKit worker and are not
  included in snapshots or exported layer caches.
- Whole-file mode runs the Dockerfile as one action; individual instructions
  are not cached separately.
- Changing only a directory's mode does not invalidate a cached instruction.

## Target kinds

- [`dockerfile_image`](/reference/prelude/dockerfile_image): build a Dockerfile.
- [`oci_layer`](/reference/prelude/oci_layer) and
  [`oci_image`](/reference/prelude/oci_image): build images without Docker.
- [`oci_index`](/reference/prelude/oci_index): combine platform images.
- [`oci_pull`](/reference/prelude/oci_pull) and
  [`oci_import`](/reference/prelude/oci_import): bring in existing images.
- [`oci_load`](/reference/prelude/oci_load) and
  [`oci_push`](/reference/prelude/oci_push): load into a local engine or
  publish to a registry.

## Next steps

- [Go](/guide/graph/go): build the static executables that native images
  package.
- [Docker](/guide/infrastructure/docker): run Struo actions in local
  containers.
- [Starlark modules](/reference/modules/): the input, provider, cache
  dependency, and archive contracts behind these target kinds.
