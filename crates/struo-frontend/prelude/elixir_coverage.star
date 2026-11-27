def _elixir_coverage_hook():
    return _elixir_test_formatter_source() + '''
defmodule Struo.Coverage do
  def run(options) do
    root = options["root"]
    target = options["target"]
    output = options["output"]
    ExUnit.start(autorun: false)
    tracer = spawn(fn -> trace(false) end)
    :erlang.trace(:all, true, [:ports, :set_on_spawn, {:tracer, tracer}])
    {:ok, _} = :cover.start()
    sources = MapSet.new(Enum.map(options["sources"], &Path.expand/1))
    modules = for directory <- options["ebin"], beam <- Path.wildcard(Path.join(directory, "*.beam")), reduce: [] do
      acc ->
        {:ok, {module, [compile_info: info]}} = :beam_lib.chunks(String.to_charlist(beam), [:compile_info])
        source = info[:source] |> to_string() |> Path.expand()
        metadata = get_in(options, ["compile_metadata", directory])
        source_root = if is_binary(metadata) do
          metadata |> File.stream!() |> Enum.find_value(fn line ->
            case String.split(String.trim(line), "\t") do
              ["source_root", encoded] -> encoded |> Base.decode64!() |> :erlang.binary_to_term([:safe])
              _ -> nil
            end
          end)
        end
        source = if is_binary(source_root) do
          relative = Path.relative_to(source, source_root)
          if relative == source or relative == ".." or String.starts_with?(relative, "../"), do: raise("coverage source outside compilation root")
          Path.expand(relative, root)
        else
          source
        end
        if MapSet.member?(sources, source) do
          {:ok, ^module} = :cover.compile_beam(String.to_charlist(beam))
          [{module, source} | acc]
        else
          acc
        end
    end
    if options["mix"] and not options["no_start"], do: Mix.Task.run("app.start", ["--no-compile", "--no-deps-check"])
    paths = if options["native_defaults"] do
      config = Mix.Project.config()
      pattern = config[:test_pattern] || if(Version.compare(System.version(), "1.19.0") == :lt, do: "*_test.exs", else: "*.{ex,exs}")
      filters = config[:test_load_filters] || [&String.ends_with?(&1, "_test.exs")]
      Enum.flat_map(config[:test_paths] || ["test"], &Path.wildcard(Path.join([&1, "**", pattern])))
      |> Enum.filter(fn file -> Enum.any?(filters, fn
        %Regex{} = filter -> Regex.match?(filter, file)
        filter when is_binary(filter) -> filter == file
        filter when is_function(filter, 1) -> filter.(file)
      end) end)
    else
      options["tests"]
    end
    helpers = if options["mix"], do: Enum.filter(Enum.map(Mix.Project.config()[:test_paths] || ["test"], &Path.join(&1, "test_helper.exs")), &File.regular?/1), else: []
    Enum.each(Enum.uniq(helpers ++ paths), &Code.require_file/1)
    ExUnit.configure(autorun: false)
    if is_integer(options["timeout_ms"]), do: ExUnit.configure(timeout: options["timeout_ms"])
    tests = for {module, _} <- :code.all_loaded(), function_exported?(module, :__ex_unit__, 0), test <- module.__ex_unit__().tests, do: test
    identity = fn test -> target <> "::" <> inspect(test.module) <> "/" <> to_string(test.name) end
    units = Enum.map(tests, identity) |> Enum.sort()
    if units == [] or length(units) != MapSet.size(MapSet.new(units)), do: raise("empty or ambiguous ExUnit inventory")
    if options["inventory"] do
      File.write!(output, :json.encode(%{schema: "struo.test_inventory.v1", target: target, units: units}))
      System.halt(0)
    end
    [test] = Enum.filter(tests, &(identity.(&1) == options["unit"]))
    summary_path = output <> ".counts"
    System.put_env("STRUO_EXUNIT_SUMMARY", summary_path)
    File.rm(summary_path)
    ExUnit.configure(only_test_ids: MapSet.new([{test.module, test.name}]), formatters: [Struo.TestFormatter], max_cases: 1)
    result = ExUnit.run([test.module])
    :erlang.trace(:all, false, [:ports, :set_on_spawn])
    ref = :erlang.trace_delivered(:all)
    receive do {:trace_delivered, :all, ^ref} -> :ok after 5000 -> raise("coverage trace did not drain") end
    send(tracer, {:result, self()})
    spawned = receive do {:ports, value} -> value after 5000 -> true end
    native = summary_path |> File.read!() |> :json.decode()
    cases = Enum.map(native["cases"], fn record ->
      status = record["status"]
      %{id: target <> "::" <> record["module"] <> "/" <> record["name"], name: record["name"], suite: record["module"],
        file: Path.relative_to(Path.expand(record["file"]), root), status: status, attempts: [%{status: status}], runner_metadata: %{line: record["line"]}}
    end)
    lines = Enum.reduce(modules, %{}, fn {module, source}, files ->
      {:ok, measured} = :cover.analyse(module, :calls, :line)
      counts = Map.new(measured, fn {{^module, line}, count} -> {line, count} end)
      Map.update(files, source, counts, &Map.merge(&1, counts, fn _, left, right -> left + right end))
    end)
    complete = not spawned and not Node.alive?() and MapSet.new(Map.keys(lines)) == sources and
      Enum.all?(lines, fn {_, counts} -> map_size(counts) > 0 end) and length(cases) == 1 and
      hd(cases).id == options["unit"] and hd(cases).status == "passed"
    files = for {source, counts} <- Enum.sort(lines) do
      numbers = Map.keys(counts) |> Enum.filter(&(&1 > 0)) |> Enum.sort()
      %{path: Path.relative_to(source, root), line_numbers: numbers, execution_counts: Enum.map(numbers, &Map.fetch!(counts, &1))}
    end
    status = if result.failures == 0, do: "passed", else: "failed"
    report = %{schema: "struo.test_results.v1", target: target, runner: %{type: "elixir_exunit", metadata: %{}}, status: status,
      summary: native["summary"], cases: cases, artifacts: %{logs: [], native_results: []},
      coverage: %{tool: "otp-cover", tool_version: options["version"], platform: options["platform"], partial: not complete, files: files}}
    File.write!(output, :json.encode(report))
    System.halt(if status == "passed", do: 0, else: 1)
  end

  defp trace(opened) do
    receive do
      {:trace, _, :open, _, _} -> trace(true)
      {:result, caller} -> send(caller, {:ports, opened})
      _ -> trace(opened)
    end
  end
end

[config] = System.argv()
options = config |> File.read!() |> :json.decode()
if options["mix"] do
  # As under `mix test`, processes a test spawns get no build code paths.
  System.delete_env("ERL_LIBS")
  Mix.start()
  Mix.Project.in_project(:struo_coverage_runner, ".", [compilers: []], fn _ ->
    if Keyword.has_key?(Mix.Project.config()[:aliases] || [], :test), do: raise("isolated coverage cannot replace a native test alias")
    Mix.Task.run("loadconfig")
    Mix.Task.run("loadpaths", ["--no-deps-check"])
    Struo.Coverage.run(options)
  end)
else
  Struo.Coverage.run(options)
end
'''

def _elixir_coverage_adapter():
    return _native_coverage_process_support() + '''import argparse

parser = argparse.ArgumentParser()
parser.add_argument('--struo-config', required=True)
parser.add_argument('--struo-results', required=True)
parser.add_argument('--struo-native-results', required=True)
parser.add_argument('--struo-test-unit')
parser.add_argument('--struo-inventory', action='store_true')
args = parser.parse_args()
options = json.loads(pathlib.Path(args.struo_config).read_text())
options['root'] = str(pathlib.Path(options['root']).resolve())
options['sources'] = [str((pathlib.Path(options['cwd']) / source).resolve()) for source in options['sources']]
output = pathlib.Path(args.struo_results).resolve()
output.parent.mkdir(parents=True, exist_ok=True)
output.unlink(missing_ok=True)
native = pathlib.Path(args.struo_native_results).resolve()
config = output.parent / 'invocation.json'
options.update(output=str(output), unit=args.struo_test_unit, inventory=args.struo_inventory)
config.write_text(json.dumps(options))
with native.open('wb') as stream:
    execution = subprocess.run(options['argv'] + [str(config)], cwd=options['cwd'], stdout=stream, stderr=stream)
raise SystemExit(execution.returncode)
'''

def _elixir_coverage_configuration(ctx, provider):
    if _elixir_attr(ctx, "setup", "") or _elixir_attr(ctx, "setup_tasks", []) or _elixir_attr(ctx, "test_args", []) or _elixir_attr(ctx, "staged_workspace", False):
        fail("coverage_per_test currently requires the native default test configuration without setup tasks, extra test_args or staged_workspace")
    sources = _file_globs(_elixir_attr(ctx, "coverage_sources", []))
    python = _resolve_host_executable("python3")
    elixir = _resolve_host_executable("elixir")
    elixir = _version_manager_tool("elixir", elixir) or elixir
    if not sources or not python or not elixir:
        fail("coverage_per_test requires coverage_sources, Python 3 and Elixir")
    inputs = _unique(_elixir_test_input_sources(ctx, _elixir_test_sources(ctx)) + _elixir_config_inputs(ctx) + _elixir_data_inputs(ctx) + _elixir_tool_inputs(ctx) + glob(["mix.exs", "mix.lock", "**/mix.exs", "**/mix.lock"]))
    version = host_command([elixir, "--version"], merge_stderr = True).strip()
    platform = host_os() + "/" + host_command([python, "-c", "import platform;print(platform.machine())"]).strip()
    provider["test_inventory"] = ctx["build_dir"] + "/test/inventory.json"
    provider["test_discovery_inputs"] = inputs
    provider["affected_inputs"] = _unique(provider["affected_inputs"] + sources)
    provider["test_info"]["listing"] = {"supported":True,"strategy":"exunit"}
    provider["test_info"]["filtering"] = {"case_filtering":"exact"}
    provider["coverage_selection"] = {"tool":"otp-cover","tool_version":version,"platform":platform,"inputs":inputs,
        "configuration":{"runtime":_portable_test_configuration(ctx, [], [version], _elixir_user_env(ctx)),"sources":sources}}
    return {"sources":sources,"python":python,"elixir":elixir,"version":version,"platform":platform}

def _elixir_coverage_actions(ctx, provider, tools, apps, srcs, env, inputs, mix_config):
    directory = ctx["build_dir"] + "/test"
    hook = directory + "/coverage.exs"
    adapter = directory + "/coverage_adapter.py"
    config = directory + "/coverage_config.json"
    inventory = provider["test_inventory"]
    write_path(hook, _elixir_coverage_hook())
    write_path(adapter, _elixir_coverage_adapter())
    argv = [tools["elixir"]] + _elixir_code_path_args(ctx, apps) + _elixir_interpreter_opts(ctx) + [_elixir_from_package(ctx, hook)]
    write_path(config, _json_encode({"argv":argv,"cwd":ctx["label"]["package"] or ".","root":workspace_root(),"target":ctx["label"]["id"],
        "sources":[workspace_root() + "/" + path for path in tools["sources"]],"ebin":[workspace_root() + "/" + app["ebin_dir"] for app in apps if app.get("ebin_dir")],
        "compile_metadata":{workspace_root() + "/" + app["ebin_dir"]:workspace_root() + "/" + app["compile_metadata"] for app in apps if app.get("ebin_dir") and app.get("compile_metadata")},
        "tests":_elixir_test_paths(ctx, srcs),"native_defaults":_elixir_attr(ctx,"_mix_test_defaults",False),"mix":bool(mix_config),"no_start":_elixir_attr(ctx,"no_start",False),"timeout_ms":_elixir_attr(ctx,"timeout_ms",None),"version":tools["version"],"platform":tools["platform"]}))
    inputs = _unique(inputs + tools["sources"] + [app["compile_metadata"] for app in apps if app.get("compile_metadata")] + [hook, adapter, config])
    base = [tools["python"], adapter, "--struo-config", config]
    run_action(argv = base + ["--struo-results",inventory,"--struo-native-results",directory + "/listing.log","--struo-inventory"], inputs = inputs, outputs = [inventory,directory + "/listing.log"], clean_paths = [inventory], create_dirs = [directory], env = env, cacheable = False, identifier = ctx["label"]["id"] + ":coverage-discovery", resources = {"memory_bytes":1024 * 1024 * 1024})
    if ctx["capability"] == "test-discovery":
        return
    expand_actions(implementation = "isolated_test_coverage_plan", inputs = [inventory], outputs = [directory + "/test_results.json"], args = {
        "target":ctx["label"]["id"],"inventory":inventory,"directory":directory,"argv":base,"inputs":inputs,"env":env,"runner_args":[],
        "filters":(ctx.get("test") or {}).get("filters") or [],"output":directory + "/test_results.json","success_exit_codes":[0,1,2],"identity":"struo.exunit.coverage.v1\x00" + tools["version"],"resources":{"memory_bytes":1024 * 1024 * 1024},
    })
