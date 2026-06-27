from __future__ import annotations

import json
from collections.abc import Collection
from pathlib import Path


def existing_loop_artifact_type(path: Path) -> str | None:
    """Best-effort local artifact type detection for loop overwrite guards."""

    if path.name.startswith("readiness-run-") or (path / "readiness.json").exists():
        return "readiness-run"
    manifest = path / "manifest.json"
    if manifest.exists():
        try:
            run_type = json.loads(manifest.read_text()).get("run_type")
        except json.JSONDecodeError:
            run_type = None
        if run_type:
            return str(run_type)
    loop_json = path / "loop.json"
    if loop_json.exists():
        try:
            payload = json.loads(loop_json.read_text())
        except json.JSONDecodeError:
            return None
        if "data_depth_loop_verdict" in payload:
            return "data-depth-run"
        if "tsmom_loop_verdict" in payload:
            return "tsmom-run"
    diagnostic_json = path / "diagnostic.json"
    if diagnostic_json.exists():
        try:
            payload = json.loads(diagnostic_json.read_text())
        except json.JSONDecodeError:
            return None
        if "tsmom_diagnostic_loop_verdict" in payload:
            return "tsmom-diagnostic-run"
    risk_loop_json = path / "risk_loop.json"
    if risk_loop_json.exists():
        try:
            payload = json.loads(risk_loop_json.read_text())
        except json.JSONDecodeError:
            return None
        if "tsmom_risk_loop_verdict" in payload:
            return "tsmom-risk-run"
    gross_edge_loop_json = path / "gross_edge_loop.json"
    if gross_edge_loop_json.exists():
        try:
            payload = json.loads(gross_edge_loop_json.read_text())
        except json.JSONDecodeError:
            return None
        if "tsmom_gross_edge_loop_verdict" in payload:
            return "tsmom-gross-edge-run"
    family_decision_json = path / "candidate_family_decision.json"
    if family_decision_json.exists():
        try:
            payload = json.loads(family_decision_json.read_text())
        except json.JSONDecodeError:
            return None
        if "tsmom_candidate_family_decision" in payload:
            return "tsmom-family-decision-run"
    decision_json = path / "decision.json"
    if decision_json.exists():
        try:
            payload = json.loads(decision_json.read_text())
        except json.JSONDecodeError:
            return None
        if "regime_only_tsmom_falsification_decision" in payload:
            return "tsmom-regime-only-falsification-run"
    baseline_harness_json = path / "baseline_harness.json"
    if baseline_harness_json.exists():
        try:
            payload = json.loads(baseline_harness_json.read_text())
        except json.JSONDecodeError:
            return None
        if "research_null_baseline_harness_loop" in payload:
            return "research-null-baseline-harness-run"
    alpha_signal_factory_json = path / "alpha_signal_factory.json"
    if alpha_signal_factory_json.exists():
        try:
            payload = json.loads(alpha_signal_factory_json.read_text())
        except json.JSONDecodeError:
            return None
        if "loop_namespace" in payload and "alpha_signal_factory" in str(payload.get("loop_namespace")):
            return "alpha-signal-factory-composition-run"
    alpha_signal_dag_json = path / "alpha_signal_dag_expansion.json"
    if alpha_signal_dag_json.exists():
        try:
            payload = json.loads(alpha_signal_dag_json.read_text())
        except json.JSONDecodeError:
            return None
        if "loop_namespace" in payload and "alpha_signal_dag" in str(payload.get("loop_namespace")):
            return "alpha-signal-dag-expansion-run"
    alpha_signal_oos_fdr_json = path / "alpha_signal_oos_fdr_validation.json"
    if alpha_signal_oos_fdr_json.exists():
        try:
            payload = json.loads(alpha_signal_oos_fdr_json.read_text())
        except json.JSONDecodeError:
            return None
        if "loop_namespace" in payload and "alpha_signal_oos_fdr" in str(payload.get("loop_namespace")):
            return "alpha-signal-oos-fdr-validation-run"
    alpha_signal_strict_json = path / "alpha_signal_strict_falsification.json"
    if alpha_signal_strict_json.exists():
        try:
            payload = json.loads(alpha_signal_strict_json.read_text())
        except json.JSONDecodeError:
            return None
        if "loop_namespace" in payload and "alpha_signal_strict_falsification" in str(
            payload.get("loop_namespace")
        ):
            return "alpha-signal-strict-falsification-run"
    multi_asset_alpha_json = path / "multi_asset_alpha_factory.json"
    if multi_asset_alpha_json.exists():
        try:
            payload = json.loads(multi_asset_alpha_json.read_text())
        except json.JSONDecodeError:
            return None
        if "loop_namespace" in payload and "multi_asset_alpha_generator_strict_prefilter" in str(
            payload.get("loop_namespace")
        ):
            return "multi-asset-alpha-generator-strict-prefilter-run"
    public_multi_asset_panel_json = path / "public_multi_asset_panel_coverage.json"
    if public_multi_asset_panel_json.exists():
        try:
            payload = json.loads(public_multi_asset_panel_json.read_text())
        except json.JSONDecodeError:
            return None
        if "loop_namespace" in payload and "public_multi_asset_data_panel_coverage_v6" in str(
            payload.get("loop_namespace")
        ):
            return "public-multi-asset-panel-coverage-run"
    return None


def prepare_loop_output_dir(
    output_dir: str | Path,
    *,
    run_type: str,
    known_files: Collection[str],
    overwrite: bool = False,
) -> Path:
    path = Path(output_dir)
    if path.exists():
        existing = existing_loop_artifact_type(path)
        if existing is not None and existing != run_type:
            raise FileExistsError(f"refusing to overwrite {existing} artifact with {run_type}: {path}")
        if not overwrite:
            raise FileExistsError(f"output directory already exists: {path}")
        if not path.is_dir():
            raise FileExistsError(f"output path exists and is not a directory: {path}")
        unexpected = [child.name for child in path.iterdir() if child.name not in known_files]
        if unexpected:
            raise FileExistsError(
                f"refusing to overwrite directory with unknown files for {run_type}: "
                f"{path} contains {unexpected}"
            )
        for child in path.iterdir():
            if child.is_file() and child.name in known_files:
                child.unlink()
        return path
    path.mkdir(parents=True, exist_ok=False)
    return path
