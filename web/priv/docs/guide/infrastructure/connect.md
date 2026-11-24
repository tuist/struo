# Connect a Project

Use `struo connect` to create a project with an infrastructure provider and bind
your repository to its shared cache in one command, or bind an existing project.

Tuist is currently the only provider that supports `struo connect`. For other
providers, add the binding to `struo.toml` by hand as described in their
[guides](/guide/infrastructure/#available-providers).

## Create and connect a project

Create a project on [Tuist](/guide/infrastructure/tuist) and bind this
repository to it:

```sh
struo connect --provider tuist --create
```

The command signs you in when needed, creates a project named after your
workspace directory under your personal account, and writes the provider
binding into the repository's root `struo.toml`:

```toml
[infrastructure.cache]
provider = "tuist"

[infrastructures.tuist]
kind = "tuist"
url = "https://tuist.dev"
account = "acme"
project = "app"
```

Commit the file so teammates and CI use the same project. It holds no
credentials: each machine still signs in with
`struo auth login --provider tuist`, or sets `TUIST_TOKEN` in CI, before it can
read or write the shared cache.

## Choose the account and name

Pass the account and project explicitly when the workspace directory name is not
the project handle you want, or when the project should belong to an
organization:

```sh
struo connect --provider tuist --create --account acme --project app
```

When `--account` is omitted, the provider creates the project under the account
that owns your session, such as your personal account.

## Bind an existing project

Leave out `--create` to bind a project that already exists:

```sh
struo connect --provider tuist --account acme --project app
```

With no account or project, Struo lists the projects your session can see. It
binds automatically when there is exactly one and asks you to choose when there
are several.

## Preview and verify

Print the binding without creating a project, signing in, or writing anything:

```sh
struo connect --provider tuist --create --account acme --project app --dry-run
```

A dry run with `--create` needs an explicit `--account`, since the account is
otherwise only known once the project exists.

Pass `--format json` for a machine-readable result. Re-running the same command
is safe: when the workspace already has a matching binding, Struo reports that
it is connected without rewriting the file or creating a second project.

## Next steps

- [Remote Execution](/guide/infrastructure/remote-execution), to run actions in
  fresh environments as well as caching them.
- [Tuist](/guide/infrastructure/tuist), for authentication in CI and how shared
  cache entries are verified across machines.
- [`struo connect` reference](/reference/cli/connect), for every option.
