import json
import os
import sys


def shown(info):
    if not isinstance(info, dict):
        return "null"
    result = info.get("result")
    if result is None:
        return "null"
    return str(result)


needs = json.loads(os.environ["NEEDS"])
if not isinstance(needs, dict):
    sys.exit("gate needs must be an object")
bad = [
    f"{name}={shown(info)}"
    for name, info in sorted(needs.items())
    if shown(info) != "success"
]
if not needs:
    sys.exit("gate has no needed jobs")
if bad:
    sys.exit("gate required success: " + ", ".join(bad))
