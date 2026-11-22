# Fly.io Sprites

[Sprites](https://fly.io/sprites) provides hosted Linux environments. Struo
creates a disposable Sprite for each uncached action, uploads declared inputs,
runs the command, retrieves declared outputs, and deletes the Sprite.

## Configure And Authenticate

Add a provider to the root `struo.toml`:

```toml
[infrastructures.remote_tests]
kind = "sprites"

[infrastructure.execution]
provider = "remote_tests"
```

Set `SPRITES_TOKEN` outside the script. `STRUO_SPRITES_TOKEN` takes precedence.
Struo uses this bearer token for creation, file transfer, command execution,
and deletion. Struo does not forward it to the command unless you explicitly
declare it in the action environment.

```sh
struo exec --remote --compute remote_tests -- ./scripts/test.sh
```

An annotated script can select the provider with `# struo remote "remote_tests"`.
You can also select the built-in name with `--compute sprites`.
See [Remote Execution](/guide/infrastructure/remote-execution) for separate
dependency-installation and Vitest actions. Declare `node_modules` as an output
of installation and an input to testing. Install native dependencies on the
same Linux architecture as the Sprite, rather than copying them from macOS.

## Environment And Transfers

Sprites supplies its standard Linux environment. Struo does not accept custom
images or templates for this provider, and cannot pin the provider's base
environment to an immutable image. Verify the Node.js and other tool versions
your scripts need before relying on cached results across environment updates.
The environment must provide `tar` with `--hard-dereference` support, `mkdir`,
and GNU `dd`.

Struo transfers archives as raw binary data through the filesystem API. It
splits uploads into requests of at most 16 MiB, then assembles the archive in
the Sprite. Large dependency directories do not require one request per file.
Struo preserves executable modes and safe workspace symbolic links, copies
hard-linked outputs as independent files, and validates downloaded archives
before installing declared outputs.

Struo uses non-TTY WebSocket exec to preserve separate binary stdout and
stderr streams and the reported process exit code. A disconnected stream
without an exit status fails the action. Struo does not use HTTP exec, whose
current framing relies on HTTP transport chunk boundaries.
Partial WebSocket messages and blocked writes must make progress within the
configured stall timeout. Quiet commands may remain silent between messages.

## Disposal And Recovery

Struo sends `DELETE /v1/sprites/{name}` after success, command failure, timeout,
or transfer failure. Idle hibernation does not delete a Sprite or its storage.
Cancellation cleanup retains the admission lease until deletion finishes.

Before creation, Struo journals a unique `struo-<nonce>` name in workspace
runtime state, then records the immutable Sprite ID from the create response.
Recovery requires the original credential and an exact name-and-ID match.
Struo checks that identity again before deletion. A replacement Sprite with
the same name is not accepted as owned. The API deletes by name and offers no
documented atomic ID precondition, so do not replace a Struo-owned name while
cleanup is running.

Failed deletions remain journaled for retry. A lost create response leaves an
ambiguous intent: Struo cannot prove ownership from a reusable name alone and
will not automatically delete it. Inspect such Sprites in your account and
resolve them manually. Keep the workspace runtime state and original
credential available for recovery. If the coordinator disappears permanently,
delete orphaned Sprites through your account; no finite provider-side lifetime
backs up this cleanup.

`STRUO_SPRITES_API_URL` overrides `https://api.sprites.dev` for testing or a
compatible endpoint. Protocol details follow the
[canonical API specification](https://api.sprites.dev/openapi.json) and the
[exec protocol](https://sprites.dev/api/sprites/exec).
