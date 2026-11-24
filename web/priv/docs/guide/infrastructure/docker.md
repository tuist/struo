# Docker Execution

Run Struo build actions in fresh local Docker containers to test remote
execution on your own machine before moving to a hosted provider.

With `struo build --remote`, Struo schedules ready actions independently,
transfers their declared inputs into containers, and collects their declared
outputs. Dependencies still determine when each action can start.

**Capabilities:** local container execution through Struo's remote-execution
interface. Docker does not provide shared caching; configure it separately.

## Prerequisites

Install Docker and prepare an image containing the action's tools, `sleep`,
`mkdir`, `rm`, and `tar` with `--hard-dereference` support, such as GNU tar.
Pull or build the image yourself, then obtain its local immutable image ID:

```sh
docker image inspect --format '{{.Id}}' your-build-image
```

## Configuration

Configure that exact `sha256:` ID in the repository root `struo.toml`:

```toml
[infrastructures.workers]
kind = "docker"
image = "sha256:REPLACE_WITH_THE_64_CHARACTER_IMAGE_ID"

[infrastructure.execution]
provider = "workers"
```

Tags and shortened IDs are rejected. Struo does not pull images implicitly.
Changing the image changes the execution binding and partitions cached results.
The example value above is a placeholder, not a runnable image ID.

Docker needs no credentials beyond the coordinator's access to the Docker
daemon.

## Usage

Build a target with its actions running in containers:

```sh
struo build --remote --compute workers your_target
```

Omit `--compute workers` to use the workspace's default execution provider.
Omit `--remote` for the local build path. `--remote` cannot be combined with
reproducibility trials.

The build header names the selected provider. Add `-v` to see each completed
action's name, target, execution or cache status, and duration.

Graph analysis still runs on the coordinator. Generic toolchains need matching
coordinator and worker tool paths. Compilation target and worker operating
system are separate: cross-compilation requires an appropriate toolchain and SDK
in the image.

## Resolve Xcode on Mac and compile on Linux

Keep the original Xcode project and its `xcode_workspace` target on the Mac.
Struo resolves native targets and source-package dependencies there, then sends
declared sources, headers, generated files, and dependency outputs to workers.
There is no graph export step or manual source checkout inside each worker.

The immutable Linux image must already contain Linux-compatible Swift, Clang,
Darwin SDK tools and resources, and `rcodesign`. Copying Xcode's macOS compiler
executables into Linux is not sufficient. Prepare the SDK according to its
licensing terms; Struo does not redistribute Xcode or upload the SDK per action.

Describe paths **inside that exact image**, not paths on the Mac:

```toml
[infrastructure.execution]
provider = "workers"

[infrastructure.execution.properties.apple]
schema_version = 1
environment = "sha256:REPLACE_WITH_THE_64_CHARACTER_IMAGE_ID"
os = "linux"
arch = "arm64"
bin = "/usr/bin"
developer_dir = "/opt/darwin/Developer"
tools = "/opt/darwin/toolset/bin"

[infrastructure.execution.properties.apple.programs]
rcodesign = "/usr/local/bin/rcodesign"

[infrastructure.execution.properties.apple.sdks.iphonesimulator]
sdk_path = "/opt/darwin/Developer/Platforms/iPhoneSimulator.platform/Developer/SDKs/iPhoneSimulator.sdk"
resource_dir = "/opt/darwin/Developer/Toolchains/XcodeDefault.xctoolchain/usr/lib/swift"
```

These paths are illustrative. The descriptor's `environment` must equal the
selected provider's immutable image ID. Add SDK records such as `iphoneos`
for other destinations. Optional SDK `plugins` and `plugin_server` paths must
be configured together. Optional `env` entries describe worker library paths;
the Mac's environment is not inherited. Optional `programs` paths can select
`libtool`, `lipo`, or installed resource compatibility tools.

Run `struo build --remote --compute workers Atlas` from the Mac project.
Independent compile actions use separate containers. Outputs return to the
Mac, and a subsequent build can restore them from cache without new workers.
The description and image selection both participate in cache identity.

This path supports ordinary C/Swift libraries and applications with ad-hoc
signing. It does not make arbitrary Xcode projects Linux-compatible. Swift
macro compilation, SwiftPM build-tool plugins, explicit modules, Catalyst, test
runtimes, simulator execution, and production signing are not supported.
Unsupported features fail rather than selecting Mac tools silently. Resource
processing requires compatible tools in the image.

## Limits and cleanup

- Docker is a development execution backend, not a production fleet or a
  hostile multi-tenant isolation guarantee. Action containers receive neither a
  host workspace mount nor the Docker socket, but do not give untrusted code
  control of the coordinator or its Docker access.
- There is no pool of warm workers. Each uncached action transfers a bounded
  archive, and intermediate outputs return through the coordinator before
  downstream actions receive them. Docker does not provide macOS workers.
- Docker workers share Struo's memory budget with local actions and also
  reserve capacity against the Docker daemon's reported CPUs and two thirds of
  its memory, which matters when a Docker VM is smaller than the host.
  Containers receive the admitted CPU and memory limits. Target kinds can
  declare command resources with [`run_action`](/reference/modules/).
- Unsupported action policies fail rather than silently running locally or
  weakening their requirements. Archive transfers have progress-stall limits
  and an overall deadline.

Struo creates a fresh container for each uncached command and removes it
afterwards. Cache hits restore outputs without starting a container. If removal
fails, Struo keeps the action's resource reservation and retries cleanup.
Changing the current Docker context mid-run does not redirect a running action
or its cleanup.

## Next steps

- [Remote Execution](/guide/infrastructure/remote-execution): understand the
  declared input and output boundary every provider shares.
- [Infrastructure](/guide/infrastructure#available-providers): choose a hosted
  provider when local validation passes.
- [Execution provider contract](/reference/infrastructure/execution): see what
  every execution provider guarantees.
