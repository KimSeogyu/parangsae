from __future__ import annotations

from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]
LIVE_ENDPOINTS = {
    "/api/v3/order",
    "/fapi/v1/order",
}
ALLOWED_ENDPOINTS = {
    "/api/v3/order/test",
    "/fapi/v1/order/test",
    "/fapi/v1/algoOrder",
}
violations = []
for path in ROOT.rglob("*.py"):
    rel = path.relative_to(ROOT).as_posix()
    text = path.read_text()
    if rel == "scripts/verify_no_live_order_defaults.py" or rel.startswith("tests/"):
        continue
    for match in re.findall(r"/[a-z0-9]+/v[0-9]/[A-Za-z0-9_/]+", text):
        if match in ALLOWED_ENDPOINTS:
            continue
        if match in LIVE_ENDPOINTS:
            violations.append(f"{rel}: contains live endpoint {match}")
        elif match.startswith(("/api/v3/order/", "/fapi/v1/order/")):
            violations.append(f"{rel}: contains unapproved order endpoint {match}")
if violations:
    print("\n".join(violations))
    sys.exit(1)
print("no direct live order endpoint defaults found")
