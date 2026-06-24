from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]
NAMES = [
    "ProductFamily",
    "ProductCapability",
    "SignalFrame",
    "TargetPortfolio",
    "OrderIntent",
    "ProtectiveOrderIntent",
    "RiskDecision",
    "ExecutionPlan",
    "ExecutionEvent",
    "EventEnvelope",
    "CapabilityRejected",
]


def test_no_duplicate_core_contract_definitions():
    violations = []
    for path in ROOT.rglob("*.py"):
        rel = path.relative_to(ROOT).as_posix()
        if rel == "packages/core/contracts.py" or rel.startswith("tests/"):
            continue
        text = path.read_text()
        for name in NAMES:
            if re.search(rf"^(class|@dataclass\(.*\)\nclass)\s+{name}\b", text, re.MULTILINE):
                violations.append(f"{rel}:{name}")
    assert violations == []


def test_exact_live_ack_text_only_in_docs_and_code():
    bad = []
    for path in list(ROOT.rglob("*.py")) + list((ROOT / "docs").rglob("*.md")) + list(ROOT.rglob("*.yaml")):
        rel = path.relative_to(ROOT).as_posix()
        text = path.read_text()
        if rel == "tests/test_static_contracts.py":
            continue
        if "TRADING_LIVE_ACK" in text or "or equivalent" in text:
            bad.append(rel)
    assert bad == []
