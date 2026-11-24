# Reference

Look up exact Struo commands, `struo.toml` manifest fields, target kinds, and
protocol behavior. Start with a guide when you are learning a workflow.

- The [`struo.toml` Manifest](/reference/manifest) covers workspace discovery,
  root-only configuration, target declarations, and path resolution.
- The [Infrastructure Provider Contracts](/reference/infrastructure/) cover
  shared caching and remote execution, including the existing transports,
  failure semantics, and acceptance checks.
- The [Command-line Reference](/reference/cli/) covers every subcommand the
  `struo` binary exposes, with its synopsis, options, and arguments.
- The [Modules Reference](/reference/modules/) documents how projects define
  target kinds and actions.
- The [Target-kind Reference](/reference/prelude/) enumerates every
  built-in target kind, including its attributes, dependency edges,
  capabilities, and outputs.
- The [Memory Reference](/reference/memory/) documents project-local
  runtime state, the evidence database, and location rules.
- The [Model Context Protocol Reference](/reference/mcp/) documents the
  [Model Context Protocol](https://modelcontextprotocol.io/) surface that
  `struo mcp` serves: transport, handshake, error
  model, an example session, and the complete tool catalog.
- The [Live Run Event Protocol Reference](/reference/events/) documents
  the gRPC surface Struo uses to stream typed events to a compatible
  ingest server: what the client sends, delivery guarantees, the event
  catalog, redaction rules, and how to enable ingest.
