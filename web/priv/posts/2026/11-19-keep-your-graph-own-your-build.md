%{
  title: "Builds are the new bottleneck",
  description: "We want more teams to benefit from faster builds and tests, without having to change how they define their projects or where they run them.",
  authors: ["pedro"]
}
---

The world is going to need more compute. Agents produce more software, and that software needs to be built and tested. When we started building infrastructure at [Tuist](https://tuist.dev), we expected that demand to grow. But teams were already compiling the same dependencies over and over across developer machines and CI. More capacity would help; so would avoiding repeated work.

We built shared caching to reuse results and remote execution to distribute work. But projects couldn't use those services unless their build tools supported them. **Using compute efficiently shouldn't require changing how you define your software.**

Sandbox providers were also giving agents new places to run code. We wanted to connect existing projects and automation to all this infrastructure, including providers other than ourselves. That became [Struo](https://struo.run), Latin for "I build" or "I arrange".

**Our mission is to bring remote caching and execution to everyone.**

## Keeping the project you already have

[Google](https://about.google) and [Meta](https://about.meta.com) have dealt with high build volumes for years. [Bazel](https://bazel.build) and [Buck](https://buck2.build) brought their approach to remote caching and execution to other organizations.

Adopting them means changing your project definition and workflow, even with agents helping with the migration. In our view, Bazel's developer experience also carries assumptions from Google's way of working that don't fit every team.

Native tools are catching up. [Xcode 26 introduced compilation caching](https://developer.apple.com/documentation/xcode-release-notes/xcode-26-release-notes), and [Cargo is working toward a cache shared across workspaces](https://goals.rust-lang.org/2026/cargo-cross-workspace-cache.html), with remote caching as a possible future extension. We wanted to offer another option without asking teams to migrate or wait.

## A narrow waist between projects and infrastructure

Struo turns a project's work into actions: a command to run, the inputs it needs, and the outputs to keep. A cache can store the results, and a compute provider can run the command. Neither needs to understand the original project format.

This is the narrow waist: **a common interface to caching and execution, without prescribing how teams organize their work.** It works for existing projects and for internal automation teams build themselves.

We've integrated with many providers for caching and remote execution, and keep adding more. We want using them to become a solved problem, without requiring specialist Bazel expertise. Teams could stop wasting compute, and providers, including us, could work on other challenges.

## Start with a script

You don't have to move a whole project to try this. **You can adopt as much as you need, one piece of work at a time.** Start with a script:

For Vitest, save this as `scripts/test.sh`:

```sh
#!/usr/bin/env -S struo exec -- bash
# struo input "../package.json"
# struo input "../package-lock.json"
# struo input "../src"
# struo input "../tests"
# struo input "../vitest.config.ts"
# struo output "../reports"
# struo cwd ".."
# struo remote "remote_tests"

set -eu
npm ci --no-audit --no-fund
./node_modules/.bin/vitest run --reporter=json --outputFile=reports/vitest.json
```

The first line runs the script through Struo. The headers tell it what to send, what to bring back, and where to run. Configure `remote_tests` in `struo.toml` with an environment supplying Node.js, npm, and Bash, and adjust the input paths for your project.

After `chmod +x scripts/test.sh`, run `./scripts/test.sh`. Struo either restores a cached result or runs the script remotely and brings back the report. The [remote execution guide](/docs/guide/infrastructure/remote-execution) covers setup and caching dependency installation separately.

You can stop there. The rest of your project keeps building and running as before.

## Then use it for your project

For supported native projects, Struo reads the existing definition and derives the build graph. That includes Cargo, Xcode, Swift packages, Mix, and Nx. You don't add build files or run an init step.

For a Cargo project, replace `cargo build` with `struo build` from the directory containing your root `Cargo.toml`. You keep your manifests, lockfile, and Rust toolchain; Struo reads the project and caches its build actions locally. Once you connect a remote cache provider, the same command shares those results with your teammates and CI.

For a Cargo workspace with dependencies, build scripts, and tests, here's how `struo build my-cli` works:

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

Cargo resolves the project; Struo schedules and caches the individual compilation and build-script actions. Results are cached per action rather than for the whole build, so changing one crate doesn't discard the work done for unrelated crates.

For actions that still need to run, `struo build --remote --compute <provider>` sends them to a remote worker while your machine schedules the build and caches the results. Remote execution is newer than caching, and we're still working on it.

## Take control with Starlark when you need it

For more control, you can define the build graph yourself. This can help you optimize graph loading or connect components written in different languages. Targets and dependencies go in `struo.toml`; Starlark defines how to build them.

Inside a custom Starlark target kind, you could declare this action:

```python
generated = declare_output("api.ts")
run_action(
    argv = ["python3", "tools/generate.py", "api/schema.json", generated],
    inputs = ["tools/generate.py", "api/schema.json"],
    outputs = [generated],
)
```

It runs a Python generator to produce a TypeScript file that another target can use. Struo tracks the inputs and caches the result. This excerpt shows just the action; the [Starlark reference](/docs/reference/modules) explains how to define the full target kind.

You can define one part of the graph or all of it. The caching and execution stay the same.

Organizations are building their own systems too. Shopify is developing [Tectonix on Nix for its World monorepo](https://www.socallinuxexpo.org/scale/23x/presentations/tectonix-bedrock-shopifys-monorepo). We don't know its eventual shape, but we're confident systems like it will need caching and distributed execution. We want Struo to be an option for teams taking that route.

## Built for agents

You can ask an agent to run the same script:

> Run the tests in scripts/test.sh and tell me whether they passed and whether the result came from cache.

With Struo's MCP server connected and execution enabled through `struo mcp --allow-run`, the agent calls the `struo_exec_script` tool with the script's path. Struo returns the exit status, cache hit or miss, and command output as structured data. The agent doesn't have to work those out by reading terminal logs.

Agents can also query the graph and validate edits. We're building live run events so they can follow progress too.

## Connecting a provider

For Tuist, connecting a project takes one command:

```sh
struo connect --provider tuist --create
```

It signs you in if needed, creates the project, and records the binding in `struo.toml` without storing credentials in the repository. Teammates and CI sign in with `struo auth login` or a token.

Struo is MIT-licensed, and the provider contract is documented.

## Why we're building it now

A few years ago, we wouldn't have attempted this ourselves. Supporting native project formats across this many toolchains would have been too much work for a team our size.

We use agents heavily to build Struo and check it against open source projects. There will be gaps. You can report them using Struo's reproduction export, which includes project manifests but leaves source files empty. Contributions are welcome too.

[Mitchell Hashimoto wrote](https://x.com/mitchellh/status/2074225453217505494), "The pursuit of excellence does not need justification." Agents have given us room to pursue a better developer experience, from the first script to a custom build system.

It would be a dream for us if native toolchains caught up and teams no longer needed another build tool for this. We'd be happy to see Struo help teams use a competitor's infrastructure too. **We want builds and tests to be faster and more efficient for everyone**, whether they use Tuist or not.

If you provide caching or compute infrastructure, email [pedro@tuist.dev](mailto:pedro@tuist.dev). We'd love to work with you to make connecting Struo to your infrastructure as easy as possible for teams using it.
