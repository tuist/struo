# Infrastructure Provider Contracts

The contracts a service must satisfy to supply shared caching, remote execution,
or artifact publication to Struo, and how provider integrations are added.

Start with [Become an Infrastructure Provider](/guide/infrastructure/providers)
for a top-to-bottom cache integration guide. For the providers available today
and what each supplies, see the
[provider list](/guide/infrastructure/#available-providers).

## Independent capabilities

- **[Cache](/reference/infrastructure/cache):** store opaque blobs and action
  results; restore verified content across machines.
- **[Execution](/reference/infrastructure/execution):** run a prepared command
  in an isolated environment and return declared outputs, streams, and exit
  status.
- **[Artifacts](/guide/infrastructure/artifacts#publication-service-contract):**
  retain immutable build products for distribution through an authenticated
  publication service.

A repository binds each capability separately, so one provider can supply a
capability without the others. Struo owns graph analysis, dependency ordering,
action identity, and cache policy. Providers do not need to interpret target
kinds or implement the graph. [Live run events](/reference/events/) are an
optional reporting protocol, not an execution or cache protocol.

## Integration status

Provider dispatch is compiled into Struo; there is no dynamic provider
registration API. A new provider needs an adapter and configuration changes in
the [Struo repository](https://github.com/tuist/struo).

Two cache kinds need no adapter:

- `kind = "object_store"` works with any service that speaks the S3, Google
  Cloud Storage, or Azure Blob Storage API.
- `kind = "http_cache"` works with any HTTP cache that supports `GET`, `HEAD`,
  and `PUT` on a key path.

There is no generic REAPI cache or execution configuration. The Tuist adapter
is the only one that speaks REAPI, for both its cache and its remote execution;
the other execution adapters transfer archives to sandboxes. The cache reference
separates provider-neutral behavior from the Tuist wire format, and the
execution reference describes the command boundary and service requirements,
not a new network API.

Optional `struo connect` provisioning and `struo auth login` support are
currently implemented for Tuist only.

Keep a new provider's transport, authentication, and service behavior in its
adapter. Use generic cache, scheduling, and action primitives for shared
behavior. Do not add ecosystem-specific scheduling or build-system branches to
a provider integration.

See the [manifest reference](/reference/manifest#root-only-tables) for the
configuration customers use and the
[provider guide](/guide/infrastructure/providers#handoff-checklist) for the
integration handoff checklist.
