# Cache Existing Scripts with Struo

Turn an existing shell, Python, or Ruby script into a cached Struo action by
adding a few header comments that declare its inputs and outputs.

The script remains the source of truth. The header tells Struo which inputs
affect the result, which outputs to restore, and which host values belong in
the cache key.

## Create a cacheable script

Suppose a repository packages its `site/` directory into
`build/site.tar.gz` before deploying it. Save this script as
`scripts/package-site.sh`:

```sh
#!/usr/bin/env -S struo exec -- bash
# struo input "../site"
# struo output "../build/site.tar.gz"
# struo cwd ".."

set -eu
mkdir -p build
tar -czf build/site.tar.gz -C site .
tar -tzf build/site.tar.gz
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
chmod +x scripts/package-site.sh
mkdir -p site
printf '<h1>Welcome</h1>\n' > site/index.html
./scripts/package-site.sh
```

The first run executes the script and reports a cache miss. Run the same command
again:

```sh
./scripts/package-site.sh
```

The second run reports a cache hit and restores `build/site.tar.gz` from the
recorded result. If any file under `site/` changes, the input digest changes
and the script runs again.

## Inspect the contract

Inspect the parsed contract before execution:

```sh
struo query script scripts/package-site.sh
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
