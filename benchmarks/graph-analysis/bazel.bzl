Symbols = provider(fields = ["symbols", "artifact"])

def _impl(ctx):
    output = ctx.actions.declare_file(ctx.label.name + "/artifact.txt")
    inputs = [ctx.file.runner] + ctx.files.srcs
    count = 0
    for dependency in ctx.attr.deps:
        inputs.append(dependency[Symbols].artifact)
        count += len(dependency[Symbols].symbols)
    ctx.actions.run(
        executable = ctx.attr.node,
        arguments = [ctx.file.runner.path, output.path, ctx.label.name, str(count)] + [f.path for f in inputs],
        inputs = inputs,
        outputs = [output],
        mnemonic = "Fixture",
    )
    return [
        DefaultInfo(files = depset([output])),
        Symbols(artifact = output, symbols = [ctx.label.name + "/exported_symbol_" + str(i) for i in range(ctx.attr.symbols)]),
    ]

fixture = rule(implementation = _impl, attrs = {
    "srcs": attr.label_list(allow_files = True),
    "deps": attr.label_list(providers = [Symbols]),
    "symbols": attr.int(),
    "node": attr.string(mandatory = True),
    "runner": attr.label(default = ":action.mjs", allow_single_file = True),
})
