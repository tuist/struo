def _action_history(ctx, ecosystem, step, variants = []):
    owner = (ctx.get("label") or {}).get("id") or ""
    if not owner:
        return None
    return action_history_key("struo." + ecosystem + ".v1", [owner, step] + variants)

def _artifact_info(artifacts, automatic_publication = False):
    return {"schema": "struo.artifact_info.v2", "artifacts": artifacts, "automatic_publication": automatic_publication}

def _artifact_entry(source, path, kind = "file", mode = 420):
    return {"kind": kind, "source": source, "path": path, "mode": mode, "directory_mode": 493, "owner_id": 0, "group_id": 0, "mtime": 0}

def _action_metadata(package = None, platforms = [], context = []):
    return {"package": package, "platforms": platforms, "context": context}

def _action_platform(scheme, identifier, label = "", usage = "product"):
    return {"scheme": scheme, "id": identifier, "label": label, "usage": usage}

def _action_context(key, value, label = ""):
    return {"key": key, "value": str(value), "label": label}

def _action_package(ecosystem, name, version = "", revision = "", digest = "", origin = ""):
    return {"ecosystem": ecosystem, "name": name, "version": version, "revision": revision, "digest": digest, "origin": origin}

def attr(name, ty, required = False, default = None, docs = "", configurable = True, implemented = True, allowed_values = [], disallowed_values = []):
    return {
        "name": name,
        "ty": ty,
        "required": required,
        "default": default,
        "docs": docs,
        "configurable": configurable,
        "implemented": implemented,
        "allowed_values": allowed_values,
        "disallowed_values": disallowed_values,
    }

def dep(name, expected_providers, docs = "", min_count = 0, max_count = None):
    return {
        "name": name,
        "expected_providers": expected_providers,
        "docs": docs,
        "min_count": min_count,
        "max_count": max_count,
    }

def capability(name, output_groups, requires_outputs = []):
    return {
        "name": name,
        "output_groups": output_groups,
        "requires_outputs": requires_outputs,
    }

def tool(name, executables = []):
    return {
        "name": name,
        "executables": executables or [name],
    }

def example(slug, name, use_when, path = None, platforms = None):
    return {
        "_struo_example": True,
        "slug": slug,
        "name": name,
        "use_when": use_when,
        "path": path or ("examples/" + slug),
        "platforms": platforms or [],
    }

def source_reference(system, symbol, url, use_when, content_digest = None):
    return {
        "_struo_source_reference": True,
        "system": system,
        "symbol": symbol,
        "url": url,
        "use_when": use_when,
        "content_digest": content_digest,
    }

def native_project(target_kind, markers, name = None, target_name = None, docs = "", inputs = [], exclude = [], input_exclude = [], manifests = [], on_match = "descend", max_depth = 16, requires_tools = [], owns_descendants = False, workspace_markers = [], marker_exclude = [], enclosing_on_demand = False, claimed_by = []):
    return {
        "_struo_native_project": True,
        "target_kind": target_kind,
        "name": name,
        "target_name": target_name,
        "docs": docs,
        "markers": markers,
        "marker_exclude": marker_exclude,
        "inputs": inputs,
        "exclude": exclude,
        "input_exclude": input_exclude,
        "manifests": manifests,
        "on_match": on_match,
        "max_depth": max_depth,
        "requires_tools": requires_tools,
        "owns_descendants": owns_descendants,
        "workspace_markers": workspace_markers,
        "enclosing_on_demand": enclosing_on_demand,
        "claimed_by": claimed_by,
    }

# Version managers that put a shim named after each tool on PATH, a symbolic
# link to the manager itself, which picks the real binary from the user's
# environment and configuration. An action runs with neither, so the shim
# fails there; `<manager> which <tool>` names the binary it would run instead.
_VERSION_MANAGERS = ["mise", "rtx"]

# The binary a version-manager shim at `path` runs for `name`, or "" when the
# manager selects none here. A path that is not such a shim comes back as is.
def _version_manager_tool(name, path):
    if not path:
        return ""
    target = host_symlink_target(path)
    if not target or target.split("/")[-1] not in _VERSION_MANAGERS:
        return path
    if not target.startswith("/"):
        target = path.rsplit("/", 1)[0] + "/" + target
    real = host_command([target, "which", name], cwd = workspace_root(), check = False).strip()
    if real.startswith("/") and host_file_exists(real):
        return real
    return ""

def _native_project_generated_dirs():
    return ["_build", "deps", "target", "third_party", "vendor"]

def target_kind(kind = None, docs = "", attrs = [], deps = [], providers = [], capabilities = [], examples = [], impl = None, resolver = None, tools = [], source_references = []):
    return {
        "_struo_target_kind": True,
        "kind": kind,
        "docs": docs,
        "attrs": attrs,
        "deps": deps,
        "providers": providers,
        "capabilities": capabilities,
        "tools": tools,
        "examples": examples,
        "source_references": source_references,
        "impl": impl,
        "resolver": resolver,
    }

def rule(kind = None, docs = "", attrs = [], deps = [], providers = [], capabilities = [], examples = [], impl = None, resolver = None, tools = [], source_references = []):
    return target_kind(
        kind = kind,
        docs = docs,
        attrs = attrs,
        deps = deps,
        providers = providers,
        capabilities = capabilities,
        examples = examples,
        impl = impl,
        resolver = resolver,
        tools = tools,
        source_references = source_references,
    )

def _resolver_snapshot_inputs(files, excluded_paths = []):
    excluded = {}
    for path in excluded_paths:
        if path:
            excluded[path[2:] if path.startswith("./") else path] = True
    return {
        path: content
        for path, content in (files or {}).items()
        if not excluded.get(path)
    }

def _ends_with(value, suffix):
    if len(value) < len(suffix):
        return False
    return value[len(value) - len(suffix):] == suffix

def _filter_by_extensions(paths, extensions):
    out = []
    for path in paths:
        for ext in extensions:
            if _ends_with(path, ext):
                out.append(path)
                break
    return out

def _file_globs(patterns, exclude = []):
    expanded = []
    for pattern in patterns:
        expanded.append(pattern)
        if _ends_with(pattern, "/**"):
            expanded.append(pattern + "/*")
    return glob(expanded, exclude = exclude)

def _package_relative(ctx, path):
    if not path:
        return path
    if path.startswith("../"):
        package = ctx["label"]["package"]
        parts = package.split("/") if package else []
        for part in path.split("/"):
            if part == ".." and parts and parts[-1] != "..":
                parts.pop()
            elif part and part != ".":
                parts.append(part)
        return "/".join(parts)
    if path.startswith("/") or path.startswith("."):
        return path
    package = ctx["label"]["package"]
    if package:
        return package + "/" + path
    return path

def _resolve_host_executable(requested):
    path_like = "/" in requested or "\\" in requested
    resolved = "" if path_like else host_which_optional(requested)
    if resolved or not requested:
        return resolved
    absolute = requested.startswith("/") or (
        len(requested) > 2 and
        requested[1] == ":" and
        (requested[2] == "/" or requested[2] == "\\")
    )
    candidate = requested if absolute else workspace_root() + "/" + requested
    return candidate if host_file_exists(candidate) else ""

def _apple_materialize_native_dep(ctx, dep, state = None):
    return dep

def _android_materialize_native_dep(ctx, dep, state = None):
    return dep

# The package manager a JavaScript workspace declares in its root
# package.json `packageManager` field, else the one whose lockfile is present.
def _javascript_package_manager():
    root = workspace_root() + "/"
    if host_file_exists(root + "package.json"):
        declared = json_decode(host_file_read(root + "package.json")).get("packageManager") or ""
        if declared:
            return declared.split("@")[0]
    for lockfile, manager in [("pnpm-lock.yaml", "pnpm"), ("yarn.lock", "yarn"), ("bun.lock", "bun"), ("bun.lockb", "bun")]:
        if host_file_exists(root + lockfile):
            return manager
    return "npm"

# PATH for a JavaScript workspace task run from `cwd`, built from stable parts
# so the action cache key does not change with the caller's shell: the
# `node_modules/.bin` chain up to the workspace root, the directories of
# Node.js and the workspace's package manager, and the system directories.
# Graph loading is not cached as an action, so it passes `host_path` to keep
# the caller's PATH as well.
def _javascript_task_path(node, cwd = ".", host_path = False):
    segments = []
    parts = [part for part in (cwd or ".").split("/") if part and part != "."]
    for index in range(len(parts), -1, -1):
        directory = "/".join(parts[:index])
        segments.append(execution_path((directory + "/" if directory else "") + "node_modules/.bin"))
    for tool in [node, host_which_optional(_javascript_package_manager())]:
        directory = _parent_dir(tool or "")
        if directory and directory not in segments:
            segments.append(directory)
    if host_os() == "windows":
        segments.extend(["C:\\Windows\\System32", "C:\\Windows"])
    else:
        segments.extend(["/usr/local/bin", "/usr/bin", "/bin", "/usr/sbin", "/sbin"])
    if host_path and host_env("PATH"):
        segments.append(host_env("PATH"))
    return (";" if host_os() == "windows" else ":").join(segments)

def _parent_dir(path):
    idx = -1
    for i in range(len(path)):
        if path[i] == "/":
            idx = i
    if idx < 0:
        return ""
    return path[:idx]

# Kotlin Multiplatform source sets compiled as kotlinc fragments, the way the
# Kotlin Gradle plugin compiles one platform target. `source_sets` maps a name
# such as `commonMain` to source patterns and `depends_on` mirrors `dependsOn`
# between them. Files in `platform_files` outside every source set form the
# `platform` fragment, which refines every declared source set. Returns the
# sources to compile and the kotlinc flags. Each file gets its own
# -Xfragment-sources flag, which kotlinc merges, so a large module never
# exceeds the per-argument length limit.
def _kotlin_source_set_fragments(label_id, source_sets, depends_on, platform_files, platform, extensions):
    if not source_sets and not depends_on:
        return (platform_files, [])
    names = sorted(source_sets.keys())
    for name in names:
        if name == platform or ":" in name or "," in name or not name:
            fail(label_id + ": source set name `" + name + "` is reserved or contains `:` or `,`; rename it, for example `commonMain`")
    for name, parents in depends_on.items():
        for candidate in [name] + parents:
            if candidate not in source_sets:
                fail(label_id + ": source_set_depends_on names `" + candidate + "`, which is not a key of source_sets; declared source sets: " + ", ".join(names))
    for name in names:
        reached = {}
        pending = list(depends_on.get(name) or [])
        for _ in range(len(names) * len(names) + 1):
            if not pending:
                break
            parent = pending.pop()
            if parent == name:
                fail(label_id + ": source_set_depends_on has a cycle through `" + name + "`; a source set cannot depend on itself")
            if parent not in reached:
                reached[parent] = True
                pending.extend(depends_on.get(parent) or [])
    owner = {}
    fragments = []
    for name in names:
        files = _filter_by_extensions(glob(source_sets[name]), extensions)
        for path in files:
            if path in owner:
                fail(label_id + ": `" + path + "` matches source sets `" + owner[path] + "` and `" + name + "`; each file belongs to one source set")
            owner[path] = name
        fragments.append((name, files))
    fragments.append((platform, [path for path in platform_files if path not in owner]))
    refines = []
    for name in names:
        for parent in sorted(depends_on.get(name) or []):
            refines.append(name + ":" + parent)
        refines.append(platform + ":" + name)
    sources = []
    args = [
        "-Xmulti-platform",
        "-Xfragments=" + ",".join([name for name, _ in fragments]),
        "-Xfragment-refines=" + ",".join(refines),
    ]
    for name, files in fragments:
        sources.extend(files)
        for path in files:
            if "," in path:
                fail(label_id + ": Kotlin source set file `" + path + "` contains `,`, which kotlinc cannot pass in -Xfragment-sources; rename the file")
            args.append("-Xfragment-sources=" + name + ":" + path)
    return (sources, args)

def _unique(values):
    seen = {}
    out = []
    for value in values:
        if value not in seen:
            seen[value] = True
            out.append(value)
    return out

def _action_source_files(paths):
    return {"_struo_workspace_source_files": paths}

def _transitive_list(value):
    """Items of a provider field that holds a list or a transitive set."""
    if type(value) == "transitive_set":
        return value.to_list()
    return value or []

def _linkopt_group(label_id, linkopts):
    return label_id + "\x00" + "\x00".join(linkopts)

def _linkopt_groups(label_id, own, transitive):
    """Each library's link options once, as a transitive set of groups.

    Options are not deduplicated one by one, because an option and its
    argument, such as `-framework Foo`, must stay together, and a library
    reached along several paths contributes its group once. `transitive`
    holds what `_dep_linkopt_groups` returns for each dependency.
    """
    return transitive_set(direct = [_linkopt_group(label_id, own)] if own else [], transitive = transitive)

def _dep_linkopt_groups(dep, flat):
    """A dependency's link option groups, or its flat `flat` list as one group
    when its target kind does not provide groups."""
    groups = dep.get("transitive_linkopt_groups")
    if groups != None:
        return groups
    if flat:
        return [_linkopt_group(dep.get("label_id") or "", flat)]
    return None

def _linkopts_of_groups(groups):
    out = []
    for group in groups.to_list(order = "topological"):
        out.extend(group.split("\x00")[1:])
    return out

def _collect_transitive(deps, key, own_values):
    out = []
    for value in own_values:
        out.append(value)
    for dep in deps:
        for value in _transitive_list(dep.get(key)):
            out.append(value)
    return _unique(out)

def _unique_args(values, option_arity = {}, forwarder = "", forwarded_option_arity = {}):
    seen = {}
    out = []
    consumed_until = 0
    for index in range(len(values)):
        if index < consumed_until:
            continue
        value = values[index]
        group = [value]
        if forwarder and value == forwarder and index + 1 < len(values):
            forwarded_option = values[index + 1]
            group.append(forwarded_option)
            cursor = index + 2
            for _unused in range(forwarded_option_arity.get(forwarded_option) or 0):
                if cursor < len(values) and values[cursor] == forwarder and cursor + 1 < len(values):
                    group.extend([values[cursor], values[cursor + 1]])
                    cursor += 2
                elif cursor < len(values):
                    group.append(values[cursor])
                    cursor += 1
            consumed_until = cursor
        else:
            arity = option_arity.get(value) or 0
            consumed_until = index + 1 + arity
            if consumed_until > len(values):
                consumed_until = len(values)
            group.extend(values[index + 1:consumed_until])
        key = repr(group)
        if key not in seen:
            seen[key] = True
            out.extend(group)
    return out

def _collect_transitive_args(deps, key, own_values):
    out = list(own_values)
    for dep in deps:
        out.extend(_transitive_list(dep.get(key)))
    return out

def _struo_executable_fields(path, runtime_files = [], os = "", architecture = "", variant = "", linkage = "unknown"):
    return {
        "struo_executable": True,
        "executable": {
            "path": path,
            "runtime_files": _unique(runtime_files),
            "os": os,
            "architecture": architecture,
            "variant": variant,
            "linkage": linkage,
        },
    }

def _struo_normalize_architecture(value):
    return {
        "aarch64": "arm64",
        "arm64": "arm64",
        "x86_64": "amd64",
        "amd64": "amd64",
        "x86": "386",
        "i386": "386",
        "i686": "386",
    }.get(value) or value

def _struo_normalize_os(value):
    return {
        "macos": "darwin",
        "win32": "windows",
    }.get(value) or value

# Host binaries and anything linked on the host carry the host C library's
# symbol versions: a build script linked against glibc 2.39 does not load on
# a glibc 2.35 host. Toolchain identities include this so a shared cache
# never hands such outputs to an older host. Hosts that select their C
# library ABI through deployment targets, and hosts without `getconf`,
# report an empty string.
def _struo_host_libc():
    if host_os() != "linux":
        return ""
    getconf = host_which_optional("getconf")
    if not getconf:
        return ""
    return host_command([getconf, "GNU_LIBC_VERSION"], check = False).strip()

def _configuration_tokens(ctx, extra = []):
    configured = (ctx.get("configuration") or {}).get("tokens") or []
    return _unique(extra + configured + ["default"])

def _is_select_value(value):
    return type(value) == type({}) and len(value) == 1 and type(value.get("select")) == type({})

def _resolve_configured_value(value, tokens, label_id, attr_name):
    if _is_select_value(value):
        branches = value["select"]
        for token in tokens:
            if token in branches:
                return _resolve_configured_value(branches[token], tokens, label_id, attr_name)
        fail(label_id + ": attribute `" + attr_name + "` uses select() without a branch matching the target configuration or a `default` branch")
    if type(value) == type([]):
        return [_resolve_configured_value(item, tokens, label_id, attr_name) for item in value]
    if type(value) == type({}):
        return {key: _resolve_configured_value(item, tokens, label_id, attr_name) for key, item in value.items()}
    return value

def _configured_attr(ctx, name, default, extra_tokens = []):
    value = ctx["attr"].get(name)
    if value == None:
        return default
    return _resolve_configured_value(
        value,
        _configuration_tokens(ctx, extra_tokens),
        ctx["label"]["id"],
        name,
    )

def _configured_attrs(ctx, extra_tokens = []):
    tokens = _configuration_tokens(ctx, extra_tokens)
    return {
        name: _resolve_configured_value(value, tokens, ctx["label"]["id"], name)
        for name, value in ctx["attr"].items()
    }

def _test_unit_suffix(ctx, unit):
    prefix = ctx["label"]["id"] + "::"
    if not unit.startswith(prefix):
        fail("test unit `" + unit + "` does not belong to target `" + ctx["label"]["id"] + "`")
    return unit[len(prefix):]

def _test_output_dir(ctx):
    batch_id = (ctx.get("test") or {}).get("batch_id")
    if batch_id:
        return ctx["build_dir"] + "/test/batches/" + batch_id
    return ctx["build_dir"] + "/test"

def _basename(path):
    normalized = path.replace("\\", "/")
    parts = normalized.split("/")
    return parts[len(parts) - 1]

def _native_library_key(library):
    return (library.get("abi") or "") + "\x00" + (library.get("path") or "")

def _unique_native_libraries(libraries):
    seen = {}
    out = []
    for library in libraries:
        abi = library.get("abi") or ""
        path = library.get("path") or ""
        if not abi or not path:
            continue
        key = _native_library_key(library)
        if key not in seen:
            seen[key] = True
            out.append({"abi": abi, "path": path})
    return out

def _shell_quote(value):
    if not value:
        return "''"
    return "'" + value.replace("'", "'\"'\"'") + "'"

def _powershell_quote(value):
    return "'" + value.replace("'", "''") + "'"

def _json_string(value):
    out = ["\""]
    for ch in value.elems():
        if ch == "\"":
            out.append("\\\"")
        elif ch == "\\":
            out.append("\\\\")
        elif ch == "\n":
            out.append("\\n")
        elif ch == "\r":
            out.append("\\r")
        elif ch == "\t":
            out.append("\\t")
        elif ord(ch) < 32:
            out.append("\\u00" + "0123456789abcdef"[ord(ch) // 16] + "0123456789abcdef"[ord(ch) % 16])
        else:
            out.append(ch)
    out.append("\"")
    return "".join(out)

def _diagnostic_fail(code, attribute, message, repair):
    fail("struo.diagnostic.v1 " + _json_encode({"code": code, "message": message, "attribute": attribute, "repairs": [repair] if type(repair) == "string" else repair}))

def _native_entry_point_matches(relative, pattern):
    segments = relative.split("/")
    expected = pattern.split("/")
    if len(segments) != len(expected):
        return False
    for index in range(len(segments)):
        part = expected[index]
        if part.startswith("*"):
            if not _ends_with(segments[index], part[1:]):
                return False
        elif segments[index] != part:
            return False
    return True

def native_entry_point(ctx, kind, attribute, preferences):
    # Each preference is a pattern, or a list of equally preferred patterns,
    # relative to the package. The entry point is the top-level file or
    # directory a pattern matched.
    package = ctx["label"]["package"]
    prefix = package + "/" if package else ""
    for preference in preferences:
        patterns = preference if type(preference) == "list" else [preference]
        entries = []
        for path in glob(patterns):
            if not path.startswith(prefix):
                continue
            relative = path[len(prefix):]
            if not any([_native_entry_point_matches(relative, pattern) for pattern in patterns]):
                continue
            entry = relative.split("/")[0]
            if entry not in entries:
                entries.append(entry)
        if len(entries) == 1:
            return prefix + entries[0]
        if len(entries) > 1:
            entries = sorted(entries)
            manifest = prefix + "struo.toml"
            _diagnostic_fail(
                "native_entry_point_ambiguous",
                attribute,
                ctx["label"]["id"] + ": " + str(len(entries)) + " entry points match " + ", ".join(["`" + pattern + "`" for pattern in patterns]) + ": " + ", ".join(entries) + ". Set `" + attribute + "` to choose one.",
                [
                    "Declare the target in `" + manifest + "` to use `" + entry + "`:\n\n[[target]]\nname = \"" + ctx["label"]["name"] + "\"\nkind = \"" + kind + "\"\n\n[target.attrs]\n" + attribute + " = \"" + entry + "\""
                    for entry in entries
                ],
            )
    return None

def _json_encode(value):
    if value == None:
        return "null"
    if type(value) == type(True):
        return "true" if value else "false"
    if type(value) == type(0):
        return str(value)
    if type(value) == type(""):
        return _json_string(value)
    if type(value) == type([]):
        return "[" + ",".join([_json_encode(item) for item in value]) + "]"
    if type(value) == type({}):
        return "{" + ",".join([
            _json_string(key) + ":" + _json_encode(value[key])
            for key in sorted(value.keys())
        ]) + "}"
    fail("cannot encode " + type(value) + " as JSON")

def _run_result_json(target_id):
    return "{\"schema\":\"struo.run_result.v1\",\"target\":" + _json_string(target_id) + ",\"exit_code\":0}\n"

def _portable_test_configuration(ctx, tool_attributes, tool_identity, env):
    return {
        "attributes": {key: value for key, value in ctx["attr"].items() if key not in tool_attributes},
        "sources": ctx["srcs"],
        "tools": tool_identity,
        "environment": env,
    }
