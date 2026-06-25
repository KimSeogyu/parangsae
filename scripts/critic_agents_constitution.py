from __future__ import annotations

import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
AGENTS = ROOT / "AGENTS.md"
README = ROOT / "README.md"
PYPROJECT = ROOT / "pyproject.toml"

REQUIRED_SECTIONS = [
    "# Parangsae Quant Constitution",
    "## 1. Project identity",
    "## 2. Non-negotiable boundaries",
    "## 5. What counts as a good strategy",
    "## 7. Data governance",
    "## 8. Backtesting and validation method",
    "## 10. Strategy addition and modification rules",
    "## 11. Result interpretation protocol",
    "## 12. Loop Engineering process",
    "## 14. Loop artifact format",
    "## 16. Commands, environment, and validation",
    "## 17. Testnet and live trading boundary",
    "## 19. Constitution change control",
    "## 21. Anti-patterns to reject",
]

REQUIRED_TERMS = [
    "self-contained",
    "NOT_READY",
    "TSMOM_VOL_TARGET_FUNDING_AWARE_V1",
    "Hypothesis first.",
    "OOS_net_utility",
    "data_snooping_risk",
    "cost/slippage/funding stress",
    "UTC timestamps",
    "closed bars",
    "Prefix-invariance/lookahead checks",
    "chronological splits",
    "cash/null return",
    "buy-and-hold",
    "commission/fees",
    "funding drag",
    "validation_depth_research_grade",
    "Verdict first",
    "Selection boundary",
    "Failed-gate action map",
    "PROMOTE_TO_NEXT_LOOP",
    "ITERATE_ON_SAME_HYPOTHESIS",
    "uv is mandatory",
    "uv sync --dev",
    "uv run quantctl",
    "uv run pytest -q",
    "testnet is mechanics",
    "live safety gates",
]

FORBIDDEN_PATTERNS = [
    r"docs/loop-engineering-quant-framework\.md",
    r"docs/templates/quant-loop",
]

RUBRIC_CHECKS = {
    "self_contained_authority": ["constitution", "self-contained", "source of truth"],
    "strategy_quality": ["falsifiable", "causal", "OOS-robust", "non-duplicative"],
    "data_governance": ["row count", "content hash", "UTC timestamps", "causal alignment"],
    "backtesting_validation": ["Pre-registration", "Split discipline", "Required costs"],
    "strategy_change_process": ["stable strategy ID", "parameter grid", "ablation"],
    "result_interpretation": ["Verdict first", "What passed?", "What failed?"],
    "safety_boundary": ["Do **not** submit live orders", "Testnet is not"],
    "operating_usefulness": ["Loop types", "Decision rules", "Minimum `loop.json` schema"],
}


def main() -> int:
    failures: list[str] = []
    agents = AGENTS.read_text()
    readme = README.read_text()
    pyproject = PYPROJECT.read_text()

    for section in REQUIRED_SECTIONS:
        if section not in agents:
            failures.append(f"missing section: {section}")

    for term in REQUIRED_TERMS:
        if term not in agents:
            failures.append(f"missing required term: {term}")

    for pattern in FORBIDDEN_PATTERNS:
        if re.search(pattern, agents) or re.search(pattern, readme):
            failures.append(f"forbidden reference/pattern present: {pattern}")

    for check_name, terms in RUBRIC_CHECKS.items():
        missing = [term for term in terms if term not in agents]
        if missing:
            failures.append(f"rubric {check_name} missing: {', '.join(missing)}")

    if "uv run quantctl" not in readme:
        failures.append("README quick commands do not use uv run quantctl")
    if "name = \"parangsae\"" not in pyproject:
        failures.append("pyproject project name must be parangsae")
    if "build-backend = \"uv_build\"" not in pyproject:
        failures.append("pyproject must use uv_build default src layout")
    if "quantctl = \"parangsae.apps.cli.quantctl:main\"" not in pyproject:
        failures.append("quantctl entry point must target parangsae package")

    result = {
        "verdict": "pass" if not failures else "fail",
        "checked_files": [str(AGENTS.relative_to(ROOT)), str(README.relative_to(ROOT)), str(PYPROJECT.relative_to(ROOT))],
        "required_sections": len(REQUIRED_SECTIONS),
        "required_terms": len(REQUIRED_TERMS),
        "failures": failures,
    }
    print(json.dumps(result, ensure_ascii=False, indent=2))
    return 0 if not failures else 1


if __name__ == "__main__":
    raise SystemExit(main())
