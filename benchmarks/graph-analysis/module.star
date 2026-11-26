def _impl(ctx):
    output = declare_output("artifact.txt")
    inputs = ["action.mjs"] + sorted(glob(ctx["srcs"]))
    symbol_count = 0
    for provider in ctx["deps"]:
        inputs.append(provider["artifact"])
        symbol_count += len(provider.get("symbols", []))
    run_action(
        argv = ["node", execution_path("action.mjs"), execution_path(output), ctx["label"]["id"], str(symbol_count)] + inputs,
        inputs = inputs,
        outputs = [output],
        sandbox = "off",
        toolchain_identity = host_command(["node", "--version"]),
    )
    return {
        "artifact": output,
        "symbols": [ctx["label"]["id"] + "/exported_symbol_" + str(i) for i in range(ctx["attr"]["symbols"])],
    }

fixture = target_kind(
    attrs = [attr("symbols", "int", default = "0", configurable = False)],
    deps = [dep("deps", ["artifact"], "Artifacts and exported symbols.")],
    providers = ["artifact"],
    capabilities = [capability("build", ["default"])],
    tools = [tool("node", executables = ["node"])],
    impl = _impl,
)
