import json
import os
import sys


def shown(info):
    if not isinstance(info, dict):
        return "not-object"
    result = info.get("result")
    if result is None:
        return "null"
    return str(result)


try:
    needs = json.loads(os.environ["NEEDS"])
except json.JSONDecodeError:
    sys.exit("gate needs are not json")
if not isinstance(needs, dict):
    sys.exit("gate needs must be an object")
# tla and kani are path-filtered. A skip is the filter declining to run them.
# Every other job must be success. failure and cancelled still fail the gate.
PATH_FILTER_JOBS = {"kani", "tla"}


def acceptable(name, info):
    result = shown(info)
    if result == "success":
        return True
    return result == "skipped" and name in PATH_FILTER_JOBS


bad = [
    f"{name}={shown(info)}"
    for name, info in sorted(needs.items())
    if not acceptable(name, info)
]
if not needs:
    sys.exit("gate has no needed jobs")
if bad:
    sys.exit("gate required success: " + ", ".join(bad))
