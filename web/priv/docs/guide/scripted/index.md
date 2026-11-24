# Cache Existing Scripts with Struo

Turn an existing shell, Python, or Ruby script into a cached Struo action by
adding a few header comments that declare its inputs and outputs.

The script remains the source of truth. The header tells Struo which inputs
affect the result, which outputs to restore, and which host values belong in
the cache key.

## Create a cacheable script

Suppose a repository turns `message.txt` into `build/greeting.txt`. Save this
script as `scripts/greet.sh`:

```sh
#!/usr/bin/env -S struo exec -- bash
# struo input "../message.txt"
# struo output "../build/greeting.txt"
# struo cwd ".."

set -eu
mkdir -p build
cp message.txt build/greeting.txt
cat build/greeting.txt
```

The `struo` headers form the cache contract:

- `input` tracks a file, directory, or glob that can change the result.
- `output` identifies a file or directory that Struo should restore on a cache
  hit.
- `cwd` chooses the working directory for the script body.

Paths in the headers are relative to the script file, which keeps the contract
stable when the repository moves.

Make the script executable, create its input, and run it:

```sh
chmod +x scripts/greet.sh
printf 'hello from Struo\n' > message.txt
./scripts/greet.sh
```

The first run executes the script and reports a cache miss. Run the same command
again:

```sh
./scripts/greet.sh
```

The second run reports a cache hit and restores `build/greeting.txt` from the
recorded result. If `message.txt` changes, the input digest changes and the
script runs again.

## Inspect the contract

Inspect the parsed contract before execution:

```sh
struo query script scripts/greet.sh
```

Coding agents can do the same through the MCP server: `struo_validate_script`
inspects the contract and `struo_exec_script` runs it. See
[Coding harnesses](/guide/harness#run-an-annotated-script).

## When to use a script

Use a scripted workflow when one existing executable already owns the work,
such as asset generation, test setup, packaging, or fixture updates. Move to a
[typed graph target](/guide/graph/) when the work needs multiple capabilities,
structured validation, or a shape that tools can inspect and edit.

## Next steps

- [Caching scripted workflows](/guide/scripted/caching): add environment
  variables, tool fingerprints, and dependent scripts to the contract.
- [Cache CLI](/guide/scripted/runtime): let a script probe and
  populate the cache itself.
- [Evidence](/guide/memory/evidence): inspect the records each run leaves
  behind.
