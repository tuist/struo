# Query Action Evidence

Query Struo's durable record of each action run: status, cache hit or miss,
action and input digests, and which input changed between two runs.

Evidence complements the cache: the cache stores reusable results, while
evidence describes what happened when Struo requested a result.

## Run and query an action

This walkthrough uses `scripts/greet.sh` from the
[scripted workflow guide](/guide/scripted/). Give it a new input value so the
first run computes a result:

```sh
printf 'evidence %s\n' "$(date +%s)" > message.txt
STRUO_CACHE_PROVIDER=local ./scripts/greet.sh
```

Now inspect the newest record. The example uses
[`jq`](https://jqlang.org/) to select the final item from Struo's
[JavaScript Object Notation](https://www.json.org/json-en.html) output:

```sh
struo --format json query evidence \
  | jq '.[-1] | {subject, status, cache, action_digest, input_digest}'
```

The record reports `passed` and `miss`. Run the unchanged script again and
repeat the query:

```sh
STRUO_CACHE_PROVIDER=local ./scripts/greet.sh
struo --format json query evidence \
  | jq '.[-1] | {subject, status, cache, action_digest, input_digest}'
```

The new record reports `hit` and keeps the same action and input digests. This
shows that Struo restored the previously computed result.

Change the declared input, run again, and compare the two newest records:

```sh
printf 'evidence changed %s\n' "$(date +%s)" > message.txt
STRUO_CACHE_PROVIDER=local ./scripts/greet.sh
struo --format json query evidence \
  | jq '.[-2:] | map({status, cache, action_digest, input_digest})'
```

The latest record reports a cache miss with different action and input digests.
The previous record remains available as history, but its digests describe the
earlier input state.

## Filter evidence by subject

Use [`struo query evidence`](/reference/cli/query/evidence) to list all records
with human-readable output:

```sh
struo query evidence
```

Graph actions have stable target subjects, so you can filter them by target or
by target and capability:

```sh
struo query evidence cli
struo query evidence cli:test
```

Use the structured output to select or compare literal `struo exec` records.

Tools can request the same record shape through
[`struo_query_evidence`](/reference/mcp/tools#struo_query_evidence) in the
[Model Context Protocol](https://modelcontextprotocol.io/) catalog.

## Read a record

An action evidence record contains:

- **Subject**: The command action, target, or target capability.
- **Status**: Whether the action passed or failed.
- **Action digest**: The identity of the complete action.
- **Input digest**: The identity of declared inputs when available.
- **Input fingerprint**: For declared graph actions, categorized component
  digests for module code, toolchains, command shape, environment, sources,
  generated inputs, and dependencies.
- **Cache state**: Whether the result was a hit, miss, or bypass.
- **Exit code**: The process exit code.
- **Stream digests**: References to captured standard output and standard error
  when available.
- **Output digests**: References to declared outputs when available.
- **Creation time**: When Struo wrote the record.

Compare digests before applying an older result to current work. Matching input
and action digests show that two records describe the same action state;
different digests show that some relevant part of that state changed.

### Find what changed

Input fingerprint components explain a change. Each has a category, label, and
digest, and never includes raw command arguments or environment values. Compare
components with the same category and label across two records to find the
source, dependency, toolchain, or action property that changed. Declared
environment variables have individual components labeled `variable:<name>`, so
you can see which variable changed without exposing its value.

## Limitations

- Evidence is historical. A passing record does not prove that the current
  inputs are unchanged; rerun the action for a current result.
- Literal commands and older records may omit the input fingerprint.
- A literal `struo exec` command uses its action digest as its subject, so it
  cannot be filtered by target name.

## Next steps

- [Memory reference](/reference/memory/): the record and storage contract.
- [Typed Graph guide](/guide/graph/): give scripted work stable target subjects
  and named capabilities such as build, run, or test.
- [Coding harnesses](/guide/harness#use-project-memory-safely): how agents
  should use evidence.
