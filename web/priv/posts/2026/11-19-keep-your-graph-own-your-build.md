%{
  title: "Builds are the new bottleneck",
  description: "Agents produce more code than toolchains can build and test. Why we built Struo to bring remote caching and execution to the projects you already have.",
  authors: ["pedro"]
}
---

When we started building infrastructure at [Tuist](https://tuist.dev), we expected agents to change how much code teams could produce. More code would mean more builds and tests, often with several agents working on the same repository at once. Companies would need more compute and a way to make the most efficient use of it. That meant reusing results instead of compiling the same dependencies again, and scheduling the remaining work to make good use of the machines available. Adding capacity alone would leave teams paying for work they could have skipped.

We built infrastructure to help with both. A shared cache lets one machine reuse work another has done. Remote execution gives a build more machines for the work that remains. We wanted teams to get their results sooner without spending compute on work they could have skipped.

But having that infrastructure available doesn't mean a project can use it. The build toolchain has to know which results it can reuse and which steps it can send to another machine. We found that many teams didn't want to change either their project definition or their build toolchain to get those capabilities. They preferred to wait for the tools they already used to catch up.

At the same time, sandbox providers were making it easier to give agents environments in which to run code. We saw more places to execute work, alongside projects and automation that had no common way to use them. We wanted to build that connection, for our infrastructure and for other providers too. That became [Struo](https://struo.run).

## Keeping the project you already have

[Google](https://about.google) and [Meta](https://about.meta.com) have dealt with high build volumes for years. Their experience went into [Bazel](https://bazel.build) and [Buck](https://buck2.build), which support **remote caching and execution**. For many teams, adopting those capabilities has meant adopting one of those build systems too.

That means describing the project again in build files, then maintaining them alongside the `Cargo.toml`, `Package.swift`, or Xcode project their tools already use. Agents make that migration cheaper, but the team still has to keep those descriptions in sync. Native tools are making progress: [Xcode 26 introduced compilation caching](https://developer.apple.com/documentation/xcode-release-notes/xcode-26-release-notes), and [Cargo is working toward a cache shared across workspaces](https://goals.rust-lang.org/2026/cargo-cross-workspace-cache.html), with remote caching identified as a possible future extension. We wanted teams to have an option besides migrating or waiting.

UTF-8 offers a useful example of adoption without conversion. In 1992, [Ken Thompson designed it with Rob Pike at a New Jersey diner](https://doc.cat-v.org/bell_labs/utf-8_history). It could encode Unicode text while leaving existing ASCII files valid, byte for byte. Developers still had work to do to support multilingual text, but they didn't have to convert their ASCII files first. We wanted the same starting point for builds: **keep the project definition and change how the work runs**.

## A narrow waist between projects and infrastructure

We think of Struo as a narrow waist. On one side are projects with their own formats and toolchains, along with scripts and other automation. On the other are providers of storage and compute, including sandboxes. In between, Struo describes work as actions with commands, inputs, and outputs, so providers don't need to understand each project's format.

With that description, Struo can fingerprint an action and look for a previous result in a shared cache. If it finds one, it downloads the result instead of running the action. If the action needs to run, Struo can schedule it locally or send it to a remote worker. Adding machines helps with the remaining work; reusing results avoids work altogether.

Our own infrastructure gave us a reason to start, but we want teams to use compute more efficiently wherever they build. Tuist is the first provider, and others have shown interest. Supporting a new project format shouldn't require a separate integration with each provider, and connecting a provider shouldn't require teams to rewrite their projects.

The name comes from Latin: *struo* means "I build" or "I arrange". It seemed fitting for a tool that takes the pieces of an existing project and works out how to build them.

## What Struo does

Struo reads the project you already have. It recognizes a `Cargo.toml`, an Xcode project, a Swift package, a Mix project, or an Nx workspace, asks the native tool what the project contains, and turns the answer into a graph of small steps with declared inputs and outputs. You don't add build files or run an init step, and Struo doesn't write anything back to your repository.

For a Cargo project, the switch starts with **changing the build command**. From the directory containing your root `Cargo.toml`, replace `cargo build` with `struo build`. You keep your manifests, lockfile, and Rust toolchain; Struo reads the project and caches its build actions locally. Once you connect a remote cache provider, the same `struo build` command also shares those results with your teammates and CI. You don't have to add cache logic to each crate or change the build command again.

Take a Cargo workspace with a few crates, some dependencies from crates.io, a crate whose build script links a C library, and some tests. This is what happens when you run `struo build my-cli`:

<figure data-part="pipeline-diagram" style="margin: 2rem 0;">
<svg viewBox="0 0 1000 250" width="100%" role="img" aria-label="Struo building a Cargo workspace in five steps. Discovery finds Cargo.toml at the workspace root. Resolution runs cargo metadata and emits a target per crate, test, build script, and locked package. Analysis creates one rustc action per crate, with build.rs as its own action. Scheduling runs actions concurrently within a resource budget. Execution reuses cached results and runs the rest locally or on remote workers." style="font-family: inherit; color: inherit;">
  <defs>
    <marker id="pd-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0,0 L10,5 L0,10 z" fill="currentColor" fill-opacity="0.55"/></marker>
  </defs>
  <g font-size="14" font-weight="600" fill="currentColor" text-anchor="middle">
    <text x="90" y="22">Discovery</text>
    <text x="292" y="22">Resolution</text>
    <text x="494" y="22">Analysis</text>
    <text x="696" y="22">Scheduling</text>
    <text x="900" y="22">Execution</text>
  </g>
  <g fill="currentColor" fill-opacity="0.06" stroke="currentColor" stroke-opacity="0.4">
    <rect x="10" y="36" width="160" height="150" rx="8"/>
    <rect x="212" y="36" width="160" height="150" rx="8"/>
    <rect x="414" y="36" width="160" height="150" rx="8"/>
    <rect x="616" y="36" width="160" height="150" rx="8"/>
  </g>
  <rect x="818" y="36" width="170" height="150" rx="8" fill="#f97316" fill-opacity="0.08" stroke="#f97316" stroke-opacity="0.7"/>
  <g font-size="13" fill="currentColor" text-anchor="middle">
    <text x="90" y="94"><tspan x="90">Finds Cargo.toml</tspan><tspan x="90" dy="18">at the workspace</tspan><tspan x="90" dy="18">root</tspan></text>
    <text x="292" y="76"><tspan x="292">Runs cargo metadata</tspan><tspan x="292" dy="18">and emits a target</tspan><tspan x="292" dy="18">per crate, test,</tspan><tspan x="292" dy="18">build script, and</tspan><tspan x="292" dy="18">locked package</tspan></text>
    <text x="494" y="85"><tspan x="494">One rustc action</tspan><tspan x="494" dy="18">per crate; build.rs</tspan><tspan x="494" dy="18">compiles and runs</tspan><tspan x="494" dy="18">as its own action</tspan></text>
    <text x="696" y="94"><tspan x="696">Actions run</tspan><tspan x="696" dy="18">concurrently within</tspan><tspan x="696" dy="18">a resource budget</tspan></text>
    <text x="903" y="85"><tspan x="903">Reuses cached</tspan><tspan x="903" dy="18">results; runs the</tspan><tspan x="903" dy="18">rest locally or on</tspan><tspan x="903" dy="18">remote workers</tspan></text>
  </g>
  <g stroke="currentColor" stroke-opacity="0.55" stroke-width="1.5" marker-end="url(#pd-arrow)">
    <line x1="172" y1="111" x2="208" y2="111"/>
    <line x1="374" y1="111" x2="410" y2="111"/>
    <line x1="576" y1="111" x2="612" y2="111"/>
    <line x1="778" y1="111" x2="814" y2="111"/>
  </g>
  <text x="903" y="216" font-size="13" text-anchor="middle" fill="#f97316" font-weight="600">Where the time is saved</text>
</svg>
</figure>

Cargo still decides what the project contains: its members, features, resolved versions, and build scripts. Struo decides how to build it. Each crate compiles in its own `rustc` action, and each build script runs as a separate action whose output goes to the crates that depend on it. Struo fingerprints every action from its command, environment, inputs, and toolchain, which gives you **a cache entry per crate** rather than one for the whole build. When you change one crate, only that crate and the crates that depend on it rebuild.

Once you connect a cache provider, every machine that builds the project shares those entries. For the actions that still need to run, `struo build --remote --compute <provider>` sends them to a remote worker while your machine schedules the build and caches the results. Remote execution is newer than caching, and we're still working on it.

The other ecosystems Struo reads follow the same model. When the default behavior doesn't fit your project, you can describe parts of the graph yourself in `struo.toml` or write your own target kinds in Starlark, and keep the native project alongside them. Declaring targets and their dependencies yourself is the normal starting point in Bazel. Struo gives you that control too, but you can start with your native project and take over the parts that need it.

## Trusting the cache

A cache entry per crate is only worth having if its results are correct. When a step reads a file it didn't declare, a cache hit can return an outdated result, and a remote worker can fail because the file isn't there. Build systems call the goal hermeticity: every step depends only on what it declares.

An imported graph doesn't guarantee that a dependency declares everything it uses. Builds can read installed SDKs or system libraries, and some dependencies access the network. Struo includes declared inputs and toolchain identity in its fingerprints, and we're exploring checks for undeclared dependencies. `struo query validate-actions` runs a target's actions in a private sandbox and reports undeclared reads and writes it can detect, along with suggested repairs. It isn't a complete audit of everything a process can access. Agents can run the same check over MCP to catch problems before the results enter a shared cache.

## Built for agents

The agent that writes a change also needs to know whether the build succeeded and what to fix if it failed. Struo exposes its graph through MCP, so an agent can discover target kinds, query dependencies, validate an edit, and run a build through tool calls. We're also building a protocol for live run events to let agents follow progress and failures without parsing terminal output.

## Connecting a provider

We care as much about setting this up as we do about running the build. Connecting a project to a provider takes one command:

```sh
struo connect --provider tuist --create
```

It signs you in if needed, creates the project, and records the binding in `struo.toml` without storing credentials in the repository. Teammates and CI sign in with `struo auth login` or a token.

Struo is open source under the MIT license, and the provider contract is documented. If you run infrastructure that should plug in, we'd welcome a contribution. We want teams to choose a provider for the service it offers, without changing their project to suit it.

## Why we're building it now

We couldn't find a tool that let teams keep their project definitions, use different infrastructure providers, and give agents structured access to the work. A few years ago, we wouldn't have attempted to build one ourselves. Supporting native project formats across this many toolchains would have been too much work for a team our size.

Agents changed what we could take on. We use them heavily to build Struo and test its importers against open source projects. They make it possible for us to cover more formats, but they don't remove the need to check the results. There will be gaps. Struo can export your project's manifests, with every other file left empty, together with a drafted issue, so you can report one without sharing your code. Fixes are welcome too.

[Mitchell Hashimoto wrote](https://x.com/mitchellh/status/2074225453217505494), "The pursuit of excellence does not need justification." We share that feeling about developer tools. Agents have reduced the cost of building them, and we want to put that time into making Struo easier to use, from the first build to connecting a provider and diagnosing a failure. We're excited that a team our size can now attempt this.

It would be a dream for us if native toolchains caught up and teams could get these capabilities without another build tool. Until then, we want to help them use compute more efficiently across the infrastructure available to them. Give Struo a try on the project you already have, and tell us where it falls short.
