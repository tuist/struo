def isolated_test_coverage_plan(ctx):
    args = ctx["args"]
    inventory = json_decode(host_file_read(workspace_root() + "/" + args["inventory"]))
    if inventory.get("schema") != "struo.test_inventory.v1" or inventory.get("target") != args["target"]:
        fail("incompatible isolated coverage inventory")
    units = inventory.get("units") or []
    if len(units) != len(_unique(units)):
        fail("duplicate isolated coverage inventory units")
    selected = args.get("filters") or units
    if any([unit not in units for unit in selected]):
        fail("requested coverage unit is not in current inventory")
    observations = []
    for unit in selected:
        directory = args["directory"] + "/observations/" + content_sha256(unit)
        result = directory + "/test_results.json"
        log = directory + "/runner.log"
        native = directory + "/native.json"
        argv = args["argv"] + ["--struo-test-unit", unit, "--struo-results", result, "--struo-native-results", native] + args["runner_args"]
        run_action(
            argv = argv,
            inputs = args["inputs"],
            outputs = [directory, result, log, native],
            clean_paths = [directory],
            create_dirs = [directory, args["directory"] + "/home"],
            env = args["env"],
            stdout = log,
            stderr = log,
            success_exit_codes = args["success_exit_codes"],
            resources = args.get("resources") or {},
            cacheable = False,
            toolchain_identity = args["identity"],
            identifier = args["target"] + ":coverage:" + content_sha256(unit),
            display_name = "Measure test coverage · " + unit,
        )
        observations.append(result)
    measured_inventory = args["directory"] + "/measured_inventory.json"
    write_path(measured_inventory, _json_encode({"schema": "struo.test_inventory.v1", "target": args["target"], "units": selected}))
    merge_test_coverage(args["target"], measured_inventory, observations, args["output"], partial = bool(args.get("filters")))

def _native_coverage_process_support():
    return '''import glob
import json
import os
import pathlib
import platform
import re
import shutil
import subprocess


def single_native_process(directory):
    path = pathlib.Path(directory, 'processes')
    if path.stat().st_size > 16 * 1024 * 1024:
        return False
    trace = path.read_text()
    if len(re.findall(r'\\bexecve(?:at)?\\(', trace)) != 1 or '+++ exited with 0 +++' not in trace:
        return False
    for line in trace.splitlines():
        if re.search(r'\\b(?:fork|vfork)\\(', line) or (re.search(r'\\bclone[3]?\\(', line) and 'CLONE_THREAD' not in line):
            return False
    return True
'''
