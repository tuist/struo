# Project Memory and Action Evidence

Struo records evidence for the actions that builds, tests, runs, and scripts
execute, so you and coding agents can query what ran and whether it was cached.

The cache answers whether Struo can reuse a result. Memory records what already
happened, so a person or tool can inspect status and provenance without scraping
terminal output.

## What Struo records

Struo records action evidence after:

- `struo exec`
- `struo run`
- `struo build`
- `struo test`

Each record identifies its subject, pass or fail status, action digest, cache
state, exit code, captured stream digests, and declared output digests when they
exist. Records belong to the current project and remain queryable across command
invocations.

## Query evidence

List the records for the current project:

```sh
struo query evidence
```

The [Evidence guide](/guide/memory/evidence) walks through running an action
twice, changing an input, and comparing the resulting records.

## Limitations

Evidence does not decide whether a result can be reused; the action cache does.
A passing record describes the inputs at the time it ran, not the current state
of the workspace.

## Next steps

- [Evidence](/guide/memory/evidence): run an action, query its record, and
  compare records across input changes.
- [Coding harnesses](/guide/harness): let a coding agent query the same
  evidence through MCP.
- [Memory reference](/reference/memory/): the record and storage contract.
