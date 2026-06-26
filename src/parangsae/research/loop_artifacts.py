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
