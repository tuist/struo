Symbols = provider(fields = ["symbols", "artifact"])

def _impl(ctx):
    output = ctx.actions.declare_output("artifact.txt")
    inputs = [ctx.attrs.runner] + ctx.attrs.srcs
    count = 0
    for dependency in ctx.attrs.deps:
        inputs.append(dependency[Symbols].artifact)
        count += len(dependency[Symbols].symbols)
    ctx.actions.run(
        cmd_args([ctx.attrs.node, ctx.attrs.runner, output.as_output(), ctx.label.name, str(count)] + inputs),
        category = "fixture",
        identifier = ctx.label.name,
    )
    return [
        DefaultInfo(default_output = output),
        Symbols(artifact = output, symbols = [ctx.label.name + "/exported_symbol_" + str(i) for i in range(ctx.attrs.symbols)]),
    ]

fixture = rule(impl = _impl, attrs = {
    "srcs": attrs.list(attrs.source()),
    "deps": attrs.list(attrs.dep(providers = [Symbols])),
    "symbols": attrs.int(),
    "node": attrs.string(),
    "runner": attrs.source(),
})

def _platforms(ctx):
    return [DefaultInfo(), ExecutionPlatformRegistrationInfo(platforms = [ExecutionPlatformInfo(
        label = ctx.label.raw_target(),
        configuration = ConfigurationInfo(constraints = {}, values = {}),
        executor_config = CommandExecutorConfig(local_enabled = True, remote_enabled = False, remote_cache_enabled = False),
    )])]

platforms = rule(impl = _platforms, attrs = {})
