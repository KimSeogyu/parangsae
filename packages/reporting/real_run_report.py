from __future__ import annotations

import json
from dataclasses import asdict
from datetime import UTC, datetime
from importlib import metadata
from pathlib import Path
from typing import Any

import pandas as pd

from packages.marketdata.binance_vision import MarketDataBundle, canonical_urls, sha256_file
from packages.research.backtest_engine import (
    BacktestResult,
    RiskRunResult,
    RISK_SELECTION_FORMULA,
    RISK_SELECTION_OBJECTIVE,
    broker_settings_dict,
    strategy_settings_dict,
)

DEPENDENCIES = ("backtesting", "ccxt", "pandas", "numpy", "requests")
SUCCESS_FILES = {"report.md", "metrics.json", "trades.csv", "equity.csv", "manifest.json"}
RISK_SUCCESS_FILES = SUCCESS_FILES | {"candidates.csv"}
FAILURE_FILES = {"failure.md", "failure.json", "manifest.json"}
KNOWN_RUN_TYPES = {"real-run", "risk-run"}


def utc_timestamp_for_path() -> str:
    return datetime.now(UTC).strftime("%Y%m%dT%H%M%SZ")


def report_file_set(run_type: str) -> set[str]:
    return RISK_SUCCESS_FILES if run_type == "risk-run" else SUCCESS_FILES


def _json_run_type(path: Path) -> str | None:
    try:
        data = json.loads(path.read_text())
    except (OSError, json.JSONDecodeError):
        return None
    run_type = data.get("run_type")
    return run_type if run_type in KNOWN_RUN_TYPES else None


def existing_report_type(path: Path) -> str | None:
    manifest_type = _json_run_type(path / "manifest.json")
    if manifest_type:
        return manifest_type
    failure_type = _json_run_type(path / "failure.json")
    if failure_type:
        return failure_type
    if path.name.startswith("risk-run-"):
        return "risk-run"
    if path.name.startswith("real-run-"):
        return "real-run"
    names = {child.name for child in path.iterdir()} if path.exists() and path.is_dir() else set()
    if "candidates.csv" in names:
        return "risk-run"
    if names & (SUCCESS_FILES | FAILURE_FILES):
        return "real-run"
    return None


def prepare_output_dir(
    *,
    reports_root: str | Path = "reports",
    output_dir: str | Path | None = None,
    overwrite: bool = False,
    run_prefix: str = "real-run",
) -> Path:
    if output_dir is not None:
        path = Path(output_dir)
        if path.exists():
            if not overwrite:
                raise FileExistsError(f"output directory already exists: {path}")
            if not path.is_dir():
                raise FileExistsError(f"output path exists and is not a directory: {path}")
            existing_type = existing_report_type(path)
            if existing_type is not None and existing_type != run_prefix:
                raise FileExistsError(
                    f"refusing to overwrite {existing_type} report with {run_prefix}: {path}"
                )
            allowed = report_file_set(run_prefix) | FAILURE_FILES
            unexpected = [child.name for child in path.iterdir() if child.name not in allowed]
            if unexpected:
                raise FileExistsError(
                    f"refusing to overwrite directory with non-report files: {path} contains {unexpected}"
                )
            for child in path.iterdir():
                if child.is_file() and child.name in allowed:
                    child.unlink()
            return path
        path.mkdir(parents=True, exist_ok=False)
        return path
    root = Path(reports_root)
    root.mkdir(parents=True, exist_ok=True)
    stamp = utc_timestamp_for_path()
    for i in range(1000):
        suffix = stamp if i == 0 else f"{stamp}-{i:03d}"
        candidate = root / f"{run_prefix}-{suffix}"
        try:
            candidate.mkdir(parents=False, exist_ok=False)
            return candidate
        except FileExistsError:
            continue
    raise FileExistsError(f"could not create unique report directory under {root}")


def dependency_versions() -> dict[str, str]:
    versions: dict[str, str] = {}
    for dep in DEPENDENCIES:
        try:
            versions[dep] = metadata.version(dep)
        except metadata.PackageNotFoundError:
            versions[dep] = "not-installed"
    return versions


def project_hashes(root: str | Path = ".") -> dict[str, str | None]:
    root = Path(root)
    result: dict[str, str | None] = {}
    for name in ("pyproject.toml", "uv.lock"):
        path = root / name
        result[name] = sha256_file(path) if path.exists() else None
    return result


def build_manifest(
    *,
    bundle: MarketDataBundle | None,
    symbol: str,
    interval: str,
    months: tuple[str, ...],
    split_index: int | None,
    project_root: str | Path = ".",
) -> dict[str, Any]:
    manifest: dict[str, Any] = {
        "created_at": datetime.now(UTC).replace(microsecond=0).isoformat().replace("+00:00", "Z"),
        "symbol": symbol,
        "interval": interval,
        "months": list(months),
        "split_index": split_index,
        "source_urls": list(canonical_urls(symbol, interval, months)),
        "dependencies": dependency_versions(),
        "project_hashes": project_hashes(project_root),
    }
    if bundle is not None:
        manifest.update(
            {
                "source_urls": [record.url for record in bundle.downloads],
                "downloads": [asdict(record) for record in bundle.downloads],
                "normalized_ohlcv_sha256": bundle.normalized_sha256,
                "row_count": int(len(bundle.frame)),
                "first_timestamp": bundle.quality["first_timestamp"],
                "last_timestamp": bundle.quality["last_timestamp"],
                "data_quality": bundle.quality,
            }
        )
    return manifest


def _json_safe(value: Any) -> Any:
    if isinstance(value, dict):
        return {str(k): _json_safe(v) for k, v in value.items()}
    if isinstance(value, list | tuple):
        return [_json_safe(v) for v in value]
    if isinstance(value, pd.Timestamp):
        if value.tzinfo is None:
            value = value.tz_localize("UTC")
        else:
            value = value.tz_convert("UTC")
        return value.isoformat().replace("+00:00", "Z")
    if pd.isna(value) if not isinstance(value, (dict, list, tuple, str, bytes)) else False:
        return None
    return value


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.write_text(json.dumps(_json_safe(payload), indent=2, sort_keys=True, ensure_ascii=False) + "\n")


def _write_dataframe(path: Path, frame: pd.DataFrame) -> None:
    export = frame.copy()
    if isinstance(export.index, pd.DatetimeIndex):
        export.index = export.index.strftime("%Y-%m-%dT%H:%M:%SZ")
    export.to_csv(path, index=True, index_label="Timestamp")


def write_success_report(
    *,
    output_dir: str | Path,
    manifest: dict[str, Any],
    meaning: dict[str, Any],
    backtest: BacktestResult,
    holdout: BacktestResult,
) -> dict[str, Any]:
    out = Path(output_dir)
    metrics = {
        "status": "success",
        "strategy": strategy_settings_dict(),
        "broker": broker_settings_dict(),
        "funding_assumption": "funding fees excluded; limitation, not hidden alpha",
        "paper_semantics": "bar-based holdout simulation; not exchange matching, not live fill proof, not a profit guarantee",
        "meaning_gate": meaning,
        "data_quality": manifest.get("data_quality", {}),
        "backtest": backtest.metrics,
        "holdout_paper": holdout.metrics,
    }
    write_json(out / "manifest.json", manifest)
    write_json(out / "metrics.json", metrics)
    trades = pd.concat(
        [
            backtest.trades.assign(segment="backtest"),
            holdout.trades.assign(segment="holdout_paper"),
        ],
        ignore_index=True,
    )
    trades.to_csv(out / "trades.csv", index=False)
    equity = pd.concat(
        [
            backtest.equity.assign(segment="backtest"),
            holdout.equity.assign(segment="holdout_paper"),
        ]
    )
    _write_dataframe(out / "equity.csv", equity)
    report = render_report(manifest=manifest, metrics=metrics)
    (out / "report.md").write_text(report)
    return metrics


def write_risk_success_report(
    *,
    output_dir: str | Path,
    manifest: dict[str, Any],
    risk_result: RiskRunResult,
) -> dict[str, Any]:
    out = Path(output_dir)
    selected = risk_result.selected
    split = risk_result.split.metadata
    metrics = {
        "status": "success",
        "run_type": "risk-run",
        "product_family": risk_result.product_family.value,
        "data_venue": "binance_usds_futures_public_klines",
        "strategy_family": "sma_atr_bracket",
        "selected_candidate_id": selected.candidate_id,
        "selected_settings": strategy_settings_dict(selected.settings),
        "broker": broker_settings_dict(),
        "selection_objective": RISK_SELECTION_OBJECTIVE,
        "selection_formula": RISK_SELECTION_FORMULA,
        "candidate_count": len(risk_result.candidates),
        "holdout_used_for_selection": False,
        "claim_boundary": (
            "canonical BTCUSDT 1h Jan-Jun 2024 risk-run is a reproducible research "
            "harness and comparison report, not proof of durable stable live returns"
        ),
        "funding_assumption": "funding fees excluded; limitation, not hidden alpha",
        "paper_semantics": "bar-based holdout simulation; not exchange matching, not live fill proof, not a profit guarantee",
        "meaning_gate": risk_result.meaning,
        "split": split,
        "data_quality": manifest.get("data_quality", {}),
        "train": selected.train.metrics,
        "validation": selected.validation.metrics,
        "holdout_paper": risk_result.holdout.metrics,
        "risk_adjusted_research_gate": {
            "chronological_split_verified": bool(split["chronological_split_verified"]),
            "holdout_used_for_selection": False,
            "candidate_count": len(risk_result.candidates),
            "selection_objective": RISK_SELECTION_OBJECTIVE,
            "selection_formula": RISK_SELECTION_FORMULA,
            "selected_settings": strategy_settings_dict(selected.settings),
            "train_metrics_present": True,
            "validation_metrics_present": True,
            "holdout_metrics_present": True,
            "max_drawdown_present": all(
                "Max_Drawdown_pct" in segment
                for segment in (selected.train.metrics, selected.validation.metrics, risk_result.holdout.metrics)
            ),
            "sharpe_present": all(
                "Sharpe_Ratio" in segment
                for segment in (selected.train.metrics, selected.validation.metrics, risk_result.holdout.metrics)
            ),
            "minimum_trade_gates_passed": True,
            "no_profit_guarantee_disclaimer_present": True,
            "live_orders_submitted": False,
        },
    }
    manifest = {
        **manifest,
        "run_type": "risk-run",
        "product_family": risk_result.product_family.value,
        "selection_objective": RISK_SELECTION_OBJECTIVE,
        "selection_formula": RISK_SELECTION_FORMULA,
        "split": split,
        "selected_candidate_id": selected.candidate_id,
    }
    write_json(out / "manifest.json", manifest)
    write_json(out / "metrics.json", metrics)
    pd.DataFrame([candidate.to_record() for candidate in risk_result.candidates]).to_csv(out / "candidates.csv", index=False)
    trades = pd.concat(
        [
            selected.train.trades.assign(segment="train", candidate_id=selected.candidate_id),
            selected.validation.trades.assign(segment="validation", candidate_id=selected.candidate_id),
            risk_result.holdout.trades.assign(segment="holdout_paper", candidate_id=selected.candidate_id),
        ],
        ignore_index=True,
    )
    trades.to_csv(out / "trades.csv", index=False)
    equity = pd.concat(
        [
            selected.train.equity.assign(segment="train", candidate_id=selected.candidate_id),
            selected.validation.equity.assign(segment="validation", candidate_id=selected.candidate_id),
            risk_result.holdout.equity.assign(segment="holdout_paper", candidate_id=selected.candidate_id),
        ]
    )
    _write_dataframe(out / "equity.csv", equity)
    (out / "report.md").write_text(render_risk_report(manifest=manifest, metrics=metrics))
    return metrics


def render_report(*, manifest: dict[str, Any], metrics: dict[str, Any]) -> str:
    backtest = metrics["backtest"]
    holdout = metrics["holdout_paper"]
    meaning = metrics["meaning_gate"]
    source_urls = manifest.get("source_urls", [])
    lines = [
        "# Binance Vision Real-Run Report",
        "",
        "## Status",
        "- status: success",
        "- no profit guarantee: true",
        "- paper semantics: bar-based holdout simulation; not exchange matching, not live fill proof",
        "- funding fees: excluded and recorded as limitation",
        "",
        "## Data",
        f"- symbol: {manifest['symbol']}",
        f"- interval: {manifest['interval']}",
        f"- months: {', '.join(manifest['months'])}",
        f"- rows: {manifest.get('row_count')}",
        f"- first timestamp: {manifest.get('first_timestamp')}",
        f"- last timestamp: {manifest.get('last_timestamp')}",
        f"- normalized OHLCV sha256: {manifest.get('normalized_ohlcv_sha256')}",
        "",
        "## Source URLs",
    ]
    lines.extend(f"- {url}" for url in source_urls)
    lines.extend(
        [
            "",
            "## Canonical Parameters",
            f"- strategy: `{json.dumps(metrics['strategy'], sort_keys=True)}`",
            f"- broker/costs: `{json.dumps(metrics['broker'], sort_keys=True)}`",
            "- funding: excluded from canonical bar-based simulation",
            "",
            "## Meaning Gate",
            f"- status: {meaning['status']}",
            f"- total bars: {meaning['total_bars']}",
            f"- train bars: {meaning['train_bars']}",
            f"- holdout bars: {meaning['holdout_bars']}",
            f"- backtest closed trades: {meaning['backtest_closed_trades']}",
            f"- holdout closed trades: {meaning['holdout_closed_trades']}",
            "",
            "## Backtest Metrics",
            f"- closed trades: {backtest['closed_trades']}",
            f"- return pct: {backtest.get('Return_pct')}",
            f"- equity final usd: {backtest.get('Equity_Final_usd')}",
            f"- max drawdown pct: {backtest.get('Max_Drawdown_pct')}",
            "",
            "## Holdout / Paper Metrics",
            f"- closed trades: {holdout['closed_trades']}",
            f"- return pct: {holdout.get('Return_pct')}",
            f"- equity final usd: {holdout.get('Equity_Final_usd')}",
            f"- max drawdown pct: {holdout.get('Max_Drawdown_pct')}",
            "",
            "## Limitations",
            "- This is a historical bar-based simulation, not Binance exchange matching.",
            "- This is not live fill proof and not a profit guarantee.",
            "- No Binance API key was used; no testnet/live order was submitted.",
        ]
    )
    return "\n".join(lines) + "\n"


def render_risk_report(*, manifest: dict[str, Any], metrics: dict[str, Any]) -> str:
    train = metrics["train"]
    validation = metrics["validation"]
    holdout = metrics["holdout_paper"]
    meaning = metrics["meaning_gate"]
    source_urls = manifest.get("source_urls", [])
    lines = [
        "# Binance Vision Risk-Adjusted Research Run Report",
        "",
        "## Status",
        "- status: success",
        "- run type: risk-run",
        "- no profit guarantee: true",
        "- live orders submitted: false",
        "- holdout used for selection: false",
        f"- claim boundary: {metrics['claim_boundary']}",
        "",
        "## Data",
        f"- symbol: {manifest['symbol']}",
        f"- interval: {manifest['interval']}",
        f"- months: {', '.join(manifest['months'])}",
        f"- product family: {metrics['product_family']}",
        f"- data venue: {metrics['data_venue']}",
        f"- rows: {manifest.get('row_count')}",
        f"- first timestamp: {manifest.get('first_timestamp')}",
        f"- last timestamp: {manifest.get('last_timestamp')}",
        "",
        "## Source URLs",
    ]
    lines.extend(f"- {url}" for url in source_urls)
    lines.extend(
        [
            "",
            "## Selection",
            f"- selected candidate: {metrics['selected_candidate_id']}",
            f"- candidate count: {metrics['candidate_count']}",
            f"- objective: {metrics['selection_objective']}",
            f"- formula: `{metrics['selection_formula']}`",
            f"- selected settings: `{json.dumps(metrics['selected_settings'], sort_keys=True)}`",
            "",
            "## Split / Meaning Gate",
            f"- chronological split verified: {meaning['chronological_split_verified']}",
            f"- train bars: {meaning['train_bars']}",
            f"- validation bars: {meaning['validation_bars']}",
            f"- holdout bars: {meaning['holdout_bars']}",
            f"- train closed trades: {meaning['train_closed_trades']}",
            f"- validation closed trades: {meaning['validation_closed_trades']}",
            f"- holdout closed trades: {meaning['holdout_closed_trades']}",
            "",
            "## Risk Metrics",
            "| segment | return pct | Sharpe | max drawdown pct | closed trades |",
            "| --- | ---: | ---: | ---: | ---: |",
            f"| train | {train.get('Return_pct')} | {train.get('Sharpe_Ratio')} | {train.get('Max_Drawdown_pct')} | {train.get('closed_trades')} |",
            f"| validation | {validation.get('Return_pct')} | {validation.get('Sharpe_Ratio')} | {validation.get('Max_Drawdown_pct')} | {validation.get('closed_trades')} |",
            f"| holdout/paper | {holdout.get('Return_pct')} | {holdout.get('Sharpe_Ratio')} | {holdout.get('Max_Drawdown_pct')} | {holdout.get('closed_trades')} |",
            "",
            "## Limitations",
            "- This is a historical bar-based simulation, not Binance exchange matching.",
            "- This is not live fill proof and not a profit guarantee.",
            "- Sharpe annualization can be misleading under serial correlation or non-normal returns.",
            "- The canonical one-window BTCUSDT result is not proof of durable stable live returns.",
            "- Funding fees are excluded and recorded as a limitation.",
            "- No Binance API key was used; no testnet/live order was submitted.",
        ]
    )
    return "\n".join(lines) + "\n"


def write_failure_report(
    *,
    output_dir: str | Path,
    manifest: dict[str, Any],
    failed_gate: str,
    reason: str,
    observed: dict[str, Any] | None = None,
    run_type: str = "real-run",
) -> dict[str, Any]:
    out = Path(output_dir)
    failure = {
        "status": "failed",
        "run_type": run_type,
        "failed_gate": failed_gate,
        "reason": reason,
        "observed": observed or {},
        "strategy": strategy_settings_dict(),
        "broker": broker_settings_dict(),
        "canonical_window": {
            "symbol": manifest.get("symbol"),
            "interval": manifest.get("interval"),
            "months": manifest.get("months"),
        },
        "manifest_available": bool(manifest),
    }
    write_json(out / "manifest.json", manifest)
    write_json(out / "failure.json", failure)
    (out / "failure.md").write_text(
        f"# Binance Vision {run_type} Failure\n\n"
        f"- status: failed\n- failed gate: {failed_gate}\n- reason: {reason}\n\n"
        "No success report was created because the run did not meet the strict meaningful-result contract.\n"
    )
    for filename in report_file_set(run_type) - {"manifest.json"}:
        (out / filename).unlink(missing_ok=True)
    return failure
