import json
import os
import sys

needs = json.loads(os.environ["NEEDS"])
bad = [
    f"{name}={info.get('result')}"
    for name, info in sorted(needs.items())
    if info.get("result") != "success"
]
if not needs:
    sys.exit("gate has no needed jobs")
if bad:
    sys.exit("gate required success: " + ", ".join(bad))
