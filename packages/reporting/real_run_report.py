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
from packages.research.regime import RegimeReport
from packages.research.readiness import READINESS_SCHEMA_VERSION, READINESS_SUCCESS_FILES, ReadinessReport
from packages.research.forward_paper import (
    EQUITY_CSV_COLUMNS,
    FORWARD_PAPER_SCHEMA_VERSION,
    POSITIONS_CSV_COLUMNS,
    TRADES_COLUMNS,
    ForwardPaperResult,
)
from packages.research.signals import SignalReport

DEPENDENCIES = ("backtesting", "ccxt", "pandas", "numpy", "requests")
SUCCESS_FILES = {
    "report.md",
    "metrics.json",
    "trades.csv",
    "equity.csv",
    "manifest.json",
    "regime.csv",
    "regime.json",
    "signals.csv",
    "signals.json",
}
RISK_SUCCESS_FILES = SUCCESS_FILES | {"candidates.csv"}
FORWARD_PAPER_SUCCESS_FILES = {
    "report.md",
    "metrics.json",
    "equity.csv",
    "trades.csv",
    "positions.csv",
    "manifest.json",
    "regime.csv",
    "regime.json",
    "signals.csv",
    "signals.json",
}
FAILURE_FILES = {"failure.md", "failure.json", "manifest.json"}
KNOWN_RUN_TYPES = {"real-run", "risk-run", "readiness-run", "forward-paper-run"}


def utc_timestamp_for_path() -> str:
    return datetime.now(UTC).strftime("%Y%m%dT%H%M%SZ")


def report_file_set(run_type: str) -> set[str]:
    if run_type == "risk-run":
        return RISK_SUCCESS_FILES
    if run_type == "readiness-run":
        return READINESS_SUCCESS_FILES
    if run_type == "forward-paper-run":
        return FORWARD_PAPER_SUCCESS_FILES
    return SUCCESS_FILES


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
    if path.name.startswith("readiness-run-"):
        return "readiness-run"
    if path.name.startswith("forward-paper-run-"):
        return "forward-paper-run"
    if path.name.startswith("real-run-"):
        return "real-run"
    names = {child.name for child in path.iterdir()} if path.exists() and path.is_dir() else set()
    if names & READINESS_SUCCESS_FILES and "readiness.json" in names:
        return "readiness-run"
    if names & FORWARD_PAPER_SUCCESS_FILES and "positions.csv" in names:
        return "forward-paper-run"
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


def _write_plain_dataframe(path: Path, frame: pd.DataFrame) -> None:
    frame.to_csv(path, index=False)


def _write_regime_artifacts(out: Path, regime_report: RegimeReport) -> None:
    _write_dataframe(out / "regime.csv", regime_report.rows)
    write_json(out / "regime.json", regime_report.to_json_payload())


def _write_signal_artifacts(out: Path, signal_report: SignalReport) -> None:
    _write_dataframe(out / "signals.csv", signal_report.rows)
    write_json(out / "signals.json", signal_report.to_json_payload())


def write_success_report(
    *,
    output_dir: str | Path,
    manifest: dict[str, Any],
    meaning: dict[str, Any],
    backtest: BacktestResult,
    holdout: BacktestResult,
    regime_report: RegimeReport,
    signal_report: SignalReport,
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
        "regime_summary": regime_report.summary,
        "signal_summary": signal_report.summary,
    }
    write_json(out / "manifest.json", manifest)
    write_json(out / "metrics.json", metrics)
    _write_regime_artifacts(out, regime_report)
    _write_signal_artifacts(out, signal_report)
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
    regime_report: RegimeReport,
    signal_report: SignalReport,
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
        "regime_summary": regime_report.summary,
        "signal_summary": signal_report.summary,
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
    _write_regime_artifacts(out, regime_report)
    _write_signal_artifacts(out, signal_report)
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


def write_readiness_success_report(
    *,
    output_dir: str | Path,
    manifest: dict[str, Any],
    readiness_report: ReadinessReport,
) -> dict[str, Any]:
    out = Path(output_dir)
    payload = readiness_report.to_json_payload()
    summary = payload["summary"]
    manifest = {
        **manifest,
        "run_type": "readiness-run",
        "readiness_schema_version": READINESS_SCHEMA_VERSION,
        "readiness_verdict": summary["verdict"],
        "readiness_failed_gates": summary["failed_gates"],
        "live_orders_submitted": False,
        "api_keys_required": False,
        "claim_boundary": payload["claim_boundary"],
    }
    write_json(out / "manifest.json", manifest)
    write_json(out / "readiness.json", payload)
    _write_plain_dataframe(out / "readiness_windows.csv", readiness_report.windows)
    _write_plain_dataframe(out / "readiness_ablation.csv", readiness_report.ablation)
    _write_plain_dataframe(out / "readiness_regime.csv", readiness_report.regime)
    (out / "report.md").write_text(render_readiness_report(manifest=manifest, readiness=payload))
    return {
        "status": "success",
        "run_type": "readiness-run",
        "schema_version": READINESS_SCHEMA_VERSION,
        "verdict": summary["verdict"],
        "failed_gates": summary["failed_gates"],
        "window_count": summary["window_count"],
        "live_orders_submitted": False,
        "claim_boundary": payload["claim_boundary"],
    }


def build_forward_paper_manifest(
    *,
    kline_bundle: Any,
    funding_metadata: dict[str, Any] | None,
    symbol: str,
    interval: str,
    project_root: str | Path = ".",
) -> dict[str, Any]:
    download = kline_bundle.download
    manifest: dict[str, Any] = {
        "created_at": datetime.now(UTC).replace(microsecond=0).isoformat().replace("+00:00", "Z"),
        "run_type": "forward-paper-run",
        "symbol": symbol,
        "interval": interval,
        "source_urls": [kline_bundle.source_url],
        "source_docs_urls": [kline_bundle.docs_url],
        "dependencies": dependency_versions(),
        "project_hashes": project_hashes(project_root),
        "live_orders_submitted": False,
        "api_keys_required": False,
        "data_quality": kline_bundle.quality,
        "row_count": int(len(kline_bundle.frame)),
        "first_timestamp": kline_bundle.quality["first_timestamp"],
        "last_timestamp": kline_bundle.quality["last_timestamp"],
        "normalized_ohlcv_sha256": kline_bundle.normalized_sha256,
        "kline_request": {
            "url": download.url,
            "docs_url": download.source_docs_url,
            "params": download.params,
            "http_status": download.http_status,
            "requested_output_rows": download.requested_output_rows,
            "exchange_request_limit": download.exchange_request_limit,
            "rows_received": download.rows_received,
            "rows_after_closed_bar_filter": download.rows_after_closed_bar_filter,
            "closed_bar_only": download.closed_bar_only,
            "include_open_bar": download.include_open_bar,
            "response_sha256": download.response_sha256,
            "normalized_ohlcv_sha256": kline_bundle.normalized_sha256,
        },
        "funding_metadata": funding_metadata or {"source": "none", "rows": 0},
    }
    return manifest


def write_forward_paper_success_report(
    *,
    output_dir: str | Path,
    manifest: dict[str, Any],
    result: ForwardPaperResult,
) -> dict[str, Any]:
    out = Path(output_dir)
    metrics = {
        **result.metrics,
        "run_type": "forward-paper-run",
        "schema_version": FORWARD_PAPER_SCHEMA_VERSION,
        "artifact_schemas": {
            "positions.csv": POSITIONS_CSV_COLUMNS,
            "equity.csv": EQUITY_CSV_COLUMNS,
            "trades.csv": TRADES_COLUMNS,
        },
        "data_quality": manifest.get("data_quality", {}),
        "kline_request": manifest.get("kline_request", {}),
        "funding_metadata": manifest.get("funding_metadata", {}),
        "regime_summary": result.regime_report.summary,
        "signal_summary": result.signal_report.summary,
    }
    manifest = {
        **manifest,
        "forward_paper_schema_version": FORWARD_PAPER_SCHEMA_VERSION,
        "claim_boundary": metrics["claim_boundary"],
    }
    write_json(out / "manifest.json", manifest)
    write_json(out / "metrics.json", metrics)
    _write_regime_artifacts(out, result.regime_report)
    _write_signal_artifacts(out, result.signal_report)
    _write_dataframe(out / "positions.csv", result.positions)
    _write_dataframe(out / "equity.csv", result.equity)
    result.trades.to_csv(out / "trades.csv", index=False)
    (out / "report.md").write_text(render_forward_paper_report(manifest=manifest, metrics=metrics))
    return metrics


def _regime_protocol_lines(metrics: dict[str, Any]) -> list[str]:
    summary = metrics.get("regime_summary", {})
    latest = summary.get("latest") or {}
    return [
        "",
        "## Regime Protocol",
        "- role: causal risk/permission overlay; not an alpha signal, live-fill proof, or profit guarantee",
        f"- latest regime: {latest.get('regime_label')}",
        f"- latest action: {latest.get('protocol_action')}",
        f"- latest risk multiplier: {latest.get('risk_multiplier')}",
        f"- latest reason: {latest.get('protocol_reason')}",
        f"- label counts: `{json.dumps(summary.get('label_counts', {}), sort_keys=True)}`",
        f"- protocol counts: `{json.dumps(summary.get('protocol_counts', {}), sort_keys=True)}`",
        f"- claim boundary: {summary.get('claim_boundary')}",
    ]


def _alpha_signal_factory_lines(metrics: dict[str, Any]) -> list[str]:
    summary = metrics.get("signal_summary", {})
    latest = summary.get("latest") or {}
    return [
        "",
        "## Alpha Signal Factory",
        "- role: causal OHLCV research diagnostics; not trading advice, live-fill proof, model alpha proof, or a profit guarantee",
        f"- latest regime: {latest.get('regime')}",
        f"- diagnostic decision (not executable): {latest.get('decision')}",
        f"- direction score: {latest.get('direction_score')}",
        f"- entry score: {latest.get('entry_score')}",
        f"- risk score: {latest.get('risk_score')}",
        f"- exit score: {latest.get('exit_score')}",
        f"- final score: {latest.get('final_score')}",
        f"- signal count: {latest.get('signal_count')}",
        f"- family count: {latest.get('family_count')}",
        f"- category counts: `{json.dumps(summary.get('category_counts', {}), sort_keys=True)}`",
        f"- duplicate family max counts: `{json.dumps(summary.get('duplicate_family_counts', {}), sort_keys=True)}`",
        f"- claim boundary: {summary.get('claim_boundary')}",
        f"- execution boundary: {summary.get('execution_boundary')}",
    ]


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
        ]
    )
    lines.extend(_regime_protocol_lines(metrics))
    lines.extend(_alpha_signal_factory_lines(metrics))
    lines.extend(
        [
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
        ]
    )
    lines.extend(_regime_protocol_lines(metrics))
    lines.extend(_alpha_signal_factory_lines(metrics))
    lines.extend(
        [
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


def render_readiness_report(*, manifest: dict[str, Any], readiness: dict[str, Any]) -> str:
    summary = readiness["summary"]
    stress = readiness["stress"]
    baselines = readiness["baselines"]
    duplicate = readiness["duplicate_risk"]
    lookahead = readiness["lookahead_checks"]
    overfit = readiness["overfit_checks"]
    funding = readiness["funding_scenarios"]
    execution_proxy = readiness.get("stress", {}).get("execution_proxy", {})
    data_coverage = readiness.get("method", {}).get("data_coverage", {})
    compute_profile = readiness.get("compute_profile", {})
    validation_depth = readiness.get("validation_depth", {})
    source_urls = manifest.get("source_urls", [])
    failed_gates = summary.get("failed_gates", [])
    passed_gates = [key for key in summary.get("gate_thresholds", {}) if key not in failed_gates]
    lines = [
        "# Destructive Readiness Validation Report",
        "",
        "## Status",
        "- status: success",
        "- run type: readiness-run",
        f"- readiness verdict: {summary['verdict']}",
        f"- failed gates: {', '.join(failed_gates) if failed_gates else 'none'}",
        f"- gates without failure flags: {', '.join(passed_gates) if passed_gates else 'none'}",
        "- live orders submitted: false",
        "- API keys required: false",
        "- no profit guarantee: true",
        f"- claim boundary: {readiness['claim_boundary']}",
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
            "## Walk-forward / OOS",
            f"- window count: {summary['window_count']}",
            f"- median base holdout return pct: {summary.get('median_base_holdout_return_pct')}",
            f"- median stress holdout return pct: {summary.get('median_stress_holdout_return_pct')}",
            f"- all holdouts have trades: {summary.get('all_holdouts_have_trades')}",
            f"- selected candidate switch rate: {summary.get('selected_candidate_switch_rate')}",
            f"- overfit status: {overfit.get('status')}",
            "",
            "## Validation depth / compute profile",
            f"- validation depth grade: {validation_depth.get('grade')}",
            f"- research-grade workload passed: {validation_depth.get('research_grade')}",
            f"- failed depth checks: {', '.join(validation_depth.get('failed_depth_checks', [])) or 'none'}",
            f"- wall-clock policy: {validation_depth.get('wall_clock_policy')}",
            f"- elapsed seconds: {compute_profile.get('elapsed_seconds')}",
            f"- elapsed seconds role: {compute_profile.get('elapsed_seconds_role')}",
            f"- total input bars: {compute_profile.get('total_input_bars')}",
            f"- input span days: {compute_profile.get('input_span_days')}",
            f"- window count: {compute_profile.get('window_count')}",
            f"- candidate count: {compute_profile.get('candidate_count')}",
            f"- candidate-window evaluations: {compute_profile.get('candidate_window_evaluations')}",
            f"- total strategy evaluations: {compute_profile.get('total_strategy_evaluations')}",
            f"- bars processed estimate: {compute_profile.get('bars_processed_estimate')}",
            f"- bars processed formula: `{compute_profile.get('bars_processed_formula')}`",
            "",
            "## Baselines / Costs / Funding",
            f"- cash null return pct: {baselines['cash_null_return_pct']}",
            f"- buy-hold source: {baselines['buy_hold_source']}",
            f"- stress commission: {readiness['cost_scenarios']['stress']['commission']}",
            f"- stress spread: {readiness['cost_scenarios']['stress']['spread']}",
            f"- stress slippage proxy bps: {readiness['cost_scenarios']['stress']['slippage_proxy_bps']}",
            f"- slippage formula: {readiness['cost_scenarios']['stress']['slippage_formula']}",
            f"- execution proxy source: {execution_proxy.get('source', 'fixed_bps_proxy')}",
            f"- execution proxy live-fill proof: {not bool(execution_proxy.get('not_live_fill_proof', True))}",
            f"- stress funding rate per 8h: {funding['stress_funding_rate_per_8h']}",
            f"- funding formula: {funding['funding_formula']}",
            f"- funding source: {funding.get('source')}",
            f"- funding rows: {funding.get('rows', 0)}",
            f"- funding rate selection: {funding.get('rate_selection')}",
            "",
            "## Data-source coverage",
            f"- OHLCV: {data_coverage.get('ohlcv', {}).get('status')}",
            f"- derivatives funding: {data_coverage.get('derivatives_funding', {}).get('status')}",
            f"- orderbook/orderflow: {data_coverage.get('orderbook_orderflow', {}).get('status')}",
            f"- on-chain: {data_coverage.get('on_chain', {}).get('status')}",
            f"- macro/events: {data_coverage.get('macro_events', {}).get('status')}",
            f"- sentiment/news: {data_coverage.get('sentiment_news', {}).get('status')}",
            "",
            "## Regime split",
            f"- observed regime labels: {readiness['regime_split']['regime_label_count']}",
            f"- min regime labels gate: {summary['gate_thresholds']['min_regime_labels']}",
            "",
            "## Signal ablation / duplicate-risk checks",
            f"- ablation method: {readiness['ablation']['method']}",
            f"- high correlation family pairs: {duplicate['high_corr_family_pair_count']}",
            f"- high correlation threshold: {duplicate['high_corr_threshold']}",
            f"- max raw signals per family/timestamp: {duplicate['max_raw_signals_per_family_per_timestamp']}",
            "",
            "## Stress / lookahead",
            f"- stress return floor pct: {stress['stress_return_floor_pct']}",
            f"- all windows survived stress floor: {stress['all_windows_survived']}",
            f"- lookahead probe status: {lookahead.get('status')}",
            f"- lookahead probe passed: {lookahead.get('passed')}",
            "",
            "## What failed / what is still needed before real trading",
        ]
    )
    if failed_gates:
        lines.extend(f"- failed gate: {gate}" for gate in failed_gates)
    else:
        lines.append("- no configured readiness gate failed, but this only supports further paper-only validation")
    lines.extend(
        [
            "",
            "## Live blockers",
        ]
    )
    lines.extend(f"- {blocker}" for blocker in readiness["live_blockers"])
    lines.extend(
        [
            "",
            "## Limitations",
            "- This report tries to falsify a historical research workflow; it is not exchange matching.",
            "- Passing readiness-run can at most justify additional paper-only validation.",
            "- It is not trading advice, not live-fill proof, and not a profit guarantee.",
            "- No Binance API key was used; no testnet/live order was submitted.",
        ]
    )
    return "\n".join(lines) + "\n"


def render_forward_paper_report(*, manifest: dict[str, Any], metrics: dict[str, Any]) -> str:
    source_urls = manifest.get("source_urls", [])
    docs_urls = manifest.get("source_docs_urls", [])
    kline_request = metrics.get("kline_request", {})
    funding = metrics.get("funding_metadata", {})
    lines = [
        "# Live-market Forward Paper Report",
        "",
        "## Status",
        "- status: success",
        "- run type: forward-paper-run",
        "- live orders submitted: false",
        "- API keys required: false",
        "- no profit guarantee: true",
        f"- claim boundary: {metrics['claim_boundary']}",
        f"- paper semantics: {metrics['paper_semantics']}",
        "",
        "## Data",
        f"- symbol: {manifest['symbol']}",
        f"- interval: {manifest['interval']}",
        f"- rows: {manifest.get('row_count')}",
        f"- first timestamp: {manifest.get('first_timestamp')}",
        f"- last timestamp: {manifest.get('last_timestamp')}",
        f"- normalized OHLCV sha256: {manifest.get('normalized_ohlcv_sha256')}",
        f"- closed bar only: {kline_request.get('closed_bar_only')}",
        f"- include open bar: {kline_request.get('include_open_bar')}",
        "",
        "## Source URLs",
    ]
    lines.extend(f"- {url}" for url in source_urls)
    lines.extend(["", "## Source Docs"])
    lines.extend(f"- {url}" for url in docs_urls)
    lines.extend(
        [
            "",
            "## Request Metadata",
            f"- kline URL: {kline_request.get('url')}",
            f"- params: `{json.dumps(kline_request.get('params', {}), sort_keys=True)}`",
            f"- HTTP status: {kline_request.get('http_status')}",
            f"- requested output rows: {kline_request.get('requested_output_rows')}",
            f"- exchange request limit: {kline_request.get('exchange_request_limit')}",
            f"- rows received: {kline_request.get('rows_received')}",
            f"- rows after closed-bar filter: {kline_request.get('rows_after_closed_bar_filter')}",
            "",
            "## Paper Performance",
            f"- initial cash: {metrics.get('initial_cash')}",
            f"- final equity: {metrics.get('final_equity')}",
            f"- return pct: {metrics.get('return_pct')}",
            f"- max drawdown pct: {metrics.get('max_drawdown_pct')}",
            f"- exposure time pct: {metrics.get('exposure_time_pct')}",
            f"- closed trades: {metrics.get('closed_trades')}",
            f"- open position: {metrics.get('open_position')}",
            "",
            "## Costs / Funding",
            f"- turnover notional: {metrics.get('turnover_notional')}",
            f"- fee cost total: {metrics.get('fee_cost_total')}",
            f"- slippage cost total: {metrics.get('slippage_cost_total')}",
            f"- funding cost total: {metrics.get('funding_cost_total')}",
            f"- funding source: {metrics.get('funding_source')}",
            f"- funding rows: {metrics.get('funding_rows')}",
            f"- funding metadata: `{json.dumps(funding, sort_keys=True)}`",
        ]
    )
    lines.extend(_regime_protocol_lines(metrics))
    lines.extend(_alpha_signal_factory_lines(metrics))
    lines.extend(
        [
            "",
            "## Limitations",
            "- This is public-market-data paper accounting, not Binance exchange matching.",
            "- It uses prior-bar exposure on close-to-close bars; intra-bar fills are not proven.",
            "- Public funding rows are approximated at bar-level event timestamps; account cashflows are not validated.",
            "- No exchange order was submitted.",
            "- This is not trading advice, not live-fill proof, not live readiness, and not a profit guarantee.",
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
