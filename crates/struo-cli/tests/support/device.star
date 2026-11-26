def _impl(ctx):
    target = ctx["label"]["id"]
    out = _test_output_dir(ctx)
    results = out + "/test_results.json"
    info = {
        "schema": "struo.test_info.v1", "target": target,
        "runner": {"type": "fixture", "display_name": "Fixture", "metadata": {}},
        "command": {"argv": ["/bin/sh", "runner.sh"], "env": {}, "cwd": "."},
        "outputs": {"results": results, "logs": [], "native_results": [], "coverage": []},
        "listing": {"supported": False},
        "filtering": {"case_filtering": "none"},
        "sharding": {"supported": False},
        "retries": {"supported": False, "default_attempts": 1},
        "execution": {"cacheable": False, "run_from_workspace_root": True},
        "labels": [], "metadata": {},
    }
    if ctx["capability"] == "test":
        case = {"id": target + "::device", "name": "device", "suite": "Device", "status": "passed", "runner_metadata": {}, "attempts": [{"status": "passed"}]}
        payload = {"schema": "struo.test_results.v1", "target": target, "runner": {"type": "fixture", "metadata": {}}, "status": "passed", "summary": {"total": 1, "passed": 1, "failed": 0, "flaky": 0, "skipped": 0}, "cases": [case], "artifacts": {"logs": [], "native_results": []}}
        template = out + "/payload.json"
        write_path(template, _json_encode(payload))
        run_action(
            argv = ["/bin/sh", "runner.sh", ctx["label"]["name"], template, results],
            inputs = ["runner.sh", template],
            outputs = [results],
            create_dirs = [out],
            cacheable = False,
            resources = {"slots": {"device": 1}},
            identifier = "device",
        )
    return {"test_info": info}

device_test = target_kind(providers = ["struo_test_info"], capabilities = [capability("test", ["default", "test_results"])], impl = _impl)
