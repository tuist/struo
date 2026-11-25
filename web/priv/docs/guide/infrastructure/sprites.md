# Fly.io Sprites

[Sprites](https://fly.io/sprites) from Fly.io provide hosted Linux environments.
Struo runs each uncached action remotely in a disposable Sprite, then deletes
it.

Struo uploads declared inputs, runs the command, retrieves declared outputs,
and deletes the Sprite.

## Capabilities

| Capability | Support |
| --- | --- |
| Shared cache | No; use a [cache provider](/guide/infrastructure/#available-providers) |
| Remote execution | Disposable Fly.io Linux environments |
| Artifact publication | No |
| Project setup | Manual [configuration](#configuration) |
| Authentication | `SPRITES_TOKEN` |

## Prerequisites

- A Sprites account and token with creation, execution, file-transfer and
  deletion access.
- Tools your scripts need in Sprites' standard Linux environment. Struo does
  not accept custom images or templates, and that environment is not pinned to
  an immutable image, so verify tool versions before reusing cached results
  across environment updates. The guest must provide `tar` with
  `--hard-dereference` support, `mkdir`, and GNU `dd`.

## Configuration

Add a provider to the repository root `struo.toml`:

```toml
[infrastructures.remote_tests]
kind = "sprites"

[infrastructure.execution]
provider = "remote_tests"
```

You can also select the built-in name with `--compute sprites` without any
configuration.

### Authentication

Set `SPRITES_TOKEN` outside the script. `STRUO_SPRITES_TOKEN` takes precedence.
Struo does not forward it to the command unless you explicitly declare it in
the action environment.

`STRUO_SPRITES_API_URL` overrides `https://api.sprites.dev` for testing or a
compatible endpoint.

## Usage

Run a literal command on the provider:

```sh
struo exec --remote --compute remote_tests -- node --version
```

Omit `--compute remote_tests` to use the workspace's default execution provider.

### Run an annotated test script

Save this script as `scripts/test.sh`:

```sh
#!/bin/sh
# struo remote "remote_tests"
# struo cwd ".."
# struo input "../node_modules"
# struo input "../package.json"
# struo input "../tests"
# struo output "../reports"

node node_modules/vitest/vitest.mjs run \
  --reporter=json --outputFile=reports/vitest.json
```

Run it:

```sh
struo exec --script -- sh scripts/test.sh
```

Declare any additional source, configuration, or setup files your tests read.
Struo preserves executable modes and safe symbolic links. Install native
dependencies on the same Linux architecture as the Sprite rather than copying
them from macOS. To produce `node_modules` in a separate cacheable action, see
[Remote Execution](/guide/infrastructure/remote-execution).

## Limits and cleanup

- Stdout and stderr are preserved separately as binary.
- Execution interrupted without an exit status fails the action.
- Transfers must make progress within the stall timeout; quiet commands may
  remain silent.
- Sprites has no finite provider-side lifetime to back up Struo's cleanup. See
  the [Sprites documentation](https://sprites.dev/) for account limits and
  billing.

Struo deletes the Sprite after success, command failure, timeout, transfer
failure, or cancellation. Idle hibernation does not delete a Sprite or its
storage.

Failed deletions stay recorded in the workspace's `.struo` runtime state for a
later run, which deletes only a Sprite whose name and ID both match. Do not
replace a Struo-owned `struo-<nonce>` Sprite while cleanup runs. If a create
response is lost, Struo cannot prove ownership and will not delete the Sprite:
inspect such Sprites in your account and remove them manually, as you must if
the coordinator disappears permanently.

## Next steps

- [Remote Execution](/guide/infrastructure/remote-execution): split dependency
  installation and tests into separate cacheable actions.
- [Infrastructure](/guide/infrastructure#available-providers): pick a cache
  provider so other machines reuse remote results.
- [Execution provider contract](/reference/infrastructure/execution): see what
  every execution provider guarantees.
