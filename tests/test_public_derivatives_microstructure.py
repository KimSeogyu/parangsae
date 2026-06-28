from __future__ import annotations

import json
import os
import re
import zipfile
from argparse import Namespace
from pathlib import Path
from typing import Any, cast

import pandas as pd

from parangsae.apps.cli import quantctl
from parangsae.research.loop_artifacts import existing_loop_artifact_type
from parangsae.research.public_derivatives_microstructure import (
    DATA_FAMILY_COVERAGE_COLUMNS,
    PUBLIC_DERIVATIVES_MICROSTRUCTURE_DEFAULT_UNIVERSE,
    PUBLIC_DERIVATIVES_MICROSTRUCTURE_REQUIRED_FILES,
    PUBLIC_DERIVATIVES_MICROSTRUCTURE_TARGET_FAMILIES,
    PUBLIC_DERIVATIVES_MICROSTRUCTURE_RECENT_WINDOW_REQUIRED_FILES,
    RECENT_WINDOW_ENDPOINT_FEASIBILITY_COLUMNS,
    RECENT_WINDOW_KLINE_PROXY_COVERAGE_COLUMNS,
    READINESS_BY_FAMILY_COLUMNS,
    SYMBOL_DATA_COVERAGE_COLUMNS,
    evaluate_public_derivatives_microstructure_axis,
    evaluate_public_derivatives_microstructure_recent_window_connector,
    parse_public_derivatives_symbol_universe,
    write_public_derivatives_microstructure_axis_artifacts,
    write_public_derivatives_microstructure_recent_window_artifacts,
)


def v10_fixture(tmp_path: Path) -> Path:
    out = tmp_path / "v10"
    out.mkdir(exist_ok=True)
    payload = {
        "loop_namespace": "v9_falsification_lineage_negative_control_governance_v10_only_not_readiness",
        "suite_decision": "V9_KILL_STATE_GOVERNANCE_LOCKED",
        "readiness_baseline_verdict": "NOT_READY",
        "source_v9_suite_decision": "NO_FAMILY_SURVIVES_STRICT_FALSIFICATION",
        "source_v9_next_queue_empty": True,
        "source_v9_hash_manifest_digest": "v9-digest",
        "source_v8_hash_manifest_digest": "v8-digest",
        "negative_control_families": ["correlation_breakdown"],
        "disallowed_revival_entries": ["correlation_breakdown"],
        "data_download_performed": False,
        "alpha_generation_performed": False,
        "tsmom_state_changed": False,
        "live_or_testnet_path_created": False,
    }
    (out / "v9_falsification_lineage_governance.json").write_text(json.dumps(payload, indent=2) + "\n")
    pd.DataFrame(
        [
            {
                "signal_family": "correlation_breakdown",
                "source_v9_family_decision": "FALSIFIED_SIGN_FLIP",
                "source_v9_horizons": "8;24;72",
                "negative_control_status": "frozen_negative_control",
                "paper_only": True,
                "reason": "fixture negative control",
                "claim_boundary": "paper-only",
            },
            {
                "signal_family": "volatility_dispersion",
                "source_v9_family_decision": "FALSIFIED_SIGN_FLIP",
                "source_v9_horizons": "8;24;72",
                "negative_control_status": "frozen_negative_control",
                "paper_only": True,
                "reason": "fixture negative control",
                "claim_boundary": "paper-only",
            },
        ]
    ).to_csv(out / "negative_control_registry.csv", index=False)
    pd.DataFrame(
        [
            {
                "signal_family": "correlation_breakdown",
                "source_v9_family_decision": "FALSIFIED_SIGN_FLIP",
                "revival_allowed_without_new_preregistration": False,
                "required_reentry_boundary": "new hypothesis contract",
                "paper_only": True,
                "claim_boundary": "paper-only",
            },
            {
                "signal_family": "volatility_dispersion",
                "source_v9_family_decision": "FALSIFIED_SIGN_FLIP",
                "revival_allowed_without_new_preregistration": False,
                "required_reentry_boundary": "new hypothesis contract",
                "paper_only": True,
                "claim_boundary": "paper-only",
            },
        ]
    ).to_csv(out / "disallowed_revival_registry.csv", index=False)
    return out


def write_microstructure_cache(
    cache_root: Path,
    *,
    data_family: str,
    symbol: str,
    rows: int = 4,
    start: str = "2024-01-01",
    end: str | None = None,
    include_timestamp: bool = True,
) -> Path:
    path = cache_root / data_family / f"{symbol}.csv"
    path.parent.mkdir(parents=True, exist_ok=True)
    if rows == 0:
        columns = ["timestamp", "value"] if include_timestamp else ["value"]
        pd.DataFrame([], columns=columns).to_csv(path, index=False)
        return path
    if end is not None and rows == 2:
        index = pd.DatetimeIndex([pd.Timestamp(start, tz="UTC"), pd.Timestamp(end, tz="UTC")])
    else:
        index = pd.date_range(start, periods=rows, freq="1h", tz="UTC")
    payload: dict[str, Any] = {"value": [float(i) for i in range(rows)]}
    if include_timestamp:
        payload["timestamp"] = [ts.isoformat().replace("+00:00", "Z") for ts in index]
    pd.DataFrame(payload).to_csv(path, index=False)
    return path


def evaluate_fixture(tmp_path: Path, **kwargs: Any):
    source_v10 = kwargs.pop("source_v10_artifact_path", None)
    if source_v10 is None:
        source_v10 = v10_fixture(tmp_path)
    return evaluate_public_derivatives_microstructure_axis(
        source_v10_artifact_path=source_v10,
        cache_root=kwargs.pop("cache_root", tmp_path / "micro-cache"),
        funding_cache_dir=kwargs.pop("funding_cache_dir", tmp_path / "funding-cache"),
        commands=["uv run quantctl public-derivatives-microstructure-data-axis-run"],
        changed_files=[
            "src/parangsae/research/public_derivatives_microstructure.py",
            "data/raw-public-cache-file.csv",
        ],
        **kwargs,
    )


def test_v10_artifact_is_read_and_kill_state_preserved(tmp_path: Path) -> None:
    result = evaluate_fixture(tmp_path)
    assert result.payload["v10_kill_state_preserved"] is True
    assert result.payload["source_v10_artifact_path"].endswith("v10")
    assert result.payload["source_v9_suite_decision"] == "NO_FAMILY_SURVIVES_STRICT_FALSIFICATION"
    assert result.payload["source_v10_payload_hash"]
    assert result.payload["source_v9_hash_manifest_digest"] == "v9-digest"
    assert result.payload["source_v8_hash_manifest_digest"] == "v8-digest"


def test_v9_empty_queue_is_preserved(tmp_path: Path) -> None:
    result = evaluate_fixture(tmp_path)
    assert result.payload["v9_next_queue_empty"] is True
    assert result.payload["source_v9_next_queue_empty"] is True


def test_negative_control_families_are_carried_forward(tmp_path: Path) -> None:
    result = evaluate_fixture(tmp_path)
    assert result.payload["negative_control_families"] == [
        "correlation_breakdown",
        "volatility_dispersion",
    ]


def test_disallowed_revival_entries_are_carried_forward(tmp_path: Path) -> None:
    result = evaluate_fixture(tmp_path)
    assert result.payload["disallowed_revival_entries"] == [
        "correlation_breakdown",
        "volatility_dispersion",
    ]


def test_default_universe_excludes_tonusdt_with_reason(tmp_path: Path) -> None:
    assert "TONUSDT" not in PUBLIC_DERIVATIVES_MICROSTRUCTURE_DEFAULT_UNIVERSE
    result = evaluate_fixture(tmp_path)
    excluded = {row["symbol"]: row["reason"] for row in result.payload["excluded_symbols"]}
    assert "TONUSDT" in excluded
    assert "previous coverage" in excluded["TONUSDT"]


def test_data_family_coverage_schema_is_stable(tmp_path: Path) -> None:
    result = evaluate_fixture(tmp_path)
    frame = pd.DataFrame(result.data_family_coverage)
    assert list(frame.columns) == list(DATA_FAMILY_COVERAGE_COLUMNS)
    assert set(result.payload["data_families_requested"]) == set(PUBLIC_DERIVATIVES_MICROSTRUCTURE_TARGET_FAMILIES)


def test_unavailable_data_family_records_unavailable_with_reason(tmp_path: Path) -> None:
    result = evaluate_fixture(tmp_path, requested_symbols=("BTCUSDT",))
    row = next(
        row
        for row in result.data_family_coverage
        if row["symbol"] == "BTCUSDT" and row["data_family"] == "open_interest"
    )
    assert row["available"] is False
    assert row["unavailable_reason"]
    assert "missing" in row["unavailable_reason"]


def test_missing_cache_does_not_fabricate_data(tmp_path: Path) -> None:
    result = evaluate_fixture(tmp_path, requested_symbols=("BTCUSDT",))
    row = next(row for row in result.data_family_coverage if row["data_family"] == "open_interest")
    assert row["observation_count"] == 0
    assert row["first_timestamp"] == ""
    assert row["last_timestamp"] == ""
    assert result.payload["coverage_sufficient_for_future_v12"] is False


def test_existing_cache_is_skipped_idempotently(tmp_path: Path) -> None:
    cache_root = tmp_path / "micro-cache"
    cache_file = write_microstructure_cache(
        cache_root,
        data_family="open_interest",
        symbol="BTCUSDT",
        rows=2,
        start="2024-01-01",
        end="2025-12-31",
    )
    first = evaluate_fixture(tmp_path, cache_root=cache_root, requested_symbols=("BTCUSDT",))
    second = evaluate_fixture(tmp_path, cache_root=cache_root, requested_symbols=("BTCUSDT",))
    assert first.payload["skipped_existing_count"] == second.payload["skipped_existing_count"] == 1
    assert first.payload["successful_download_count"] == second.payload["successful_download_count"] == 0
    assert first.provenance_manifest[0]["source_location_or_cache_path"] == str(cache_file)


def test_raw_data_files_are_not_added_to_changed_files(tmp_path: Path) -> None:
    result = evaluate_fixture(tmp_path)
    assert "data/raw-public-cache-file.csv" not in result.payload["changed_files"]
    assert result.payload["raw_data_files_created"] == ["data/raw-public-cache-file.csv"]
    assert result.payload["raw_data_files_committed"] is False


def test_coverage_sufficient_requires_at_least_three_microstructure_symbols(tmp_path: Path) -> None:
    cache_root = tmp_path / "micro-cache"
    write_microstructure_cache(cache_root, data_family="open_interest", symbol="BTCUSDT")
    one_symbol = evaluate_fixture(tmp_path, cache_root=cache_root, requested_symbols=("BTCUSDT",))
    assert one_symbol.payload["coverage_sufficient_for_future_v12"] is False
    assert one_symbol.payload["suite_decision"] == "MICROSTRUCTURE_AXIS_NOT_READY_INSUFFICIENT_SYMBOLS"

    for symbol in ("ETHUSDT", "SOLUSDT"):
        write_microstructure_cache(
            cache_root,
            data_family="open_interest",
            symbol=symbol,
            rows=2,
            start="2024-01-01",
            end="2025-12-31",
        )
    write_microstructure_cache(
        cache_root,
        data_family="open_interest",
        symbol="BTCUSDT",
        rows=2,
        start="2024-01-01",
        end="2025-12-31",
    )
    three_symbols = evaluate_fixture(
        tmp_path,
        cache_root=cache_root,
        requested_symbols=("BTCUSDT", "ETHUSDT", "SOLUSDT"),
    )
    assert three_symbols.payload["coverage_sufficient_for_future_v12"] is True
    assert three_symbols.payload["suite_decision"] == "MICROSTRUCTURE_AXIS_READY_FOR_V12"


def test_no_private_account_api_key_order_or_fill_path_exists(tmp_path: Path) -> None:
    result = evaluate_fixture(tmp_path)
    assert result.payload["private_data_used"] is False
    assert result.payload["account_data_used"] is False
    assert result.payload["api_key_used"] is False
    safety = result.payload["safety_gates"]
    assert safety["no_account_data_used"] is True
    assert safety["no_api_key_used"] is True
    assert safety["no_private_order_or_fill_data_used"] is True


def test_alpha_generation_performed_remains_false(tmp_path: Path) -> None:
    result = evaluate_fixture(tmp_path)
    assert result.payload["alpha_generation_performed"] is False
    assert result.payload["strategy_optimization_performed"] is False


def test_artifact_writing_and_artifact_type_detection(tmp_path: Path) -> None:
    result = evaluate_fixture(tmp_path)
    out = write_public_derivatives_microstructure_axis_artifacts(tmp_path / "v11", result)
    for filename in PUBLIC_DERIVATIVES_MICROSTRUCTURE_REQUIRED_FILES:
        assert (out / filename).exists(), filename
    assert (out / "loop.json").exists()
    payload = json.loads((out / "public_derivatives_microstructure_axis.json").read_text())
    assert payload["readiness_baseline_verdict"] == "NOT_READY"
    assert existing_loop_artifact_type(out) == "public-derivatives-microstructure-data-axis-run"


def test_cli_no_credential_no_live_path(tmp_path: Path, capsys) -> None:
    args = Namespace(
        source_v10_artifact=str(v10_fixture(tmp_path)),
        symbols="BTCUSDT",
        cache_root=str(tmp_path / "micro-cache"),
        funding_cache_dir=str(tmp_path / "funding-cache"),
        reports_root=str(tmp_path / "reports"),
        output_dir=str(tmp_path / "v11"),
        overwrite=False,
        command=None,
        changed_file=None,
    )
    assert quantctl.cmd_public_derivatives_microstructure_data_axis_run(args) == 0
    output = json.loads(capsys.readouterr().out)
    assert output["run_type"] == "public-derivatives-microstructure-data-axis-run"
    assert output["live_orders_submitted"] is False
    assert output["testnet_orders_submitted"] is False
    assert output["api_keys_required_for_research"] is False


def test_readiness_baseline_verdict_remains_not_ready(tmp_path: Path) -> None:
    result = evaluate_fixture(tmp_path)
    assert result.payload["readiness_baseline_verdict"] == "NOT_READY"


def test_tsmom_state_unchanged(tmp_path: Path) -> None:
    result = evaluate_fixture(tmp_path)
    assert result.payload["tsmom_state_changed"] is False


def test_v7_v8_v9_v10_commands_remain_wired_importable() -> None:
    parser = quantctl.build_parser()
    commands = cast(Any, parser)._subparsers._group_actions[0].choices
    for command in (
        "public-multi-asset-data-backfill-run",
        "panel-ready-multi-asset-alpha-prefilter-rerun",
        "multi-asset-strict-oos-fdr-falsification-run",
        "multi-asset-v9-falsification-lineage-governance-run",
        "public-derivatives-microstructure-data-axis-run",
    ):
        assert command in commands


def test_parse_public_derivatives_symbol_universe_is_deterministic() -> None:
    assert parse_public_derivatives_symbol_universe("BTCUSDT,ETHUSDT,BTCUSDT") == ("BTCUSDT", "ETHUSDT")


def test_tonusdt_override_is_still_excluded_with_reason(tmp_path: Path) -> None:
    result = evaluate_fixture(tmp_path, requested_symbols=("TONUSDT", "BTCUSDT"))
    assert "TONUSDT" not in result.payload["requested_symbol_universe"]
    excluded = {row["symbol"]: row["reason"] for row in result.payload["excluded_symbols"]}
    assert "TONUSDT" in excluded
    assert "previous coverage" in excluded["TONUSDT"]


def test_tonusdt_only_request_writes_not_ready_artifact_with_exclusion(tmp_path: Path) -> None:
    result = evaluate_fixture(tmp_path, requested_symbols=("TONUSDT",))
    assert result.payload["requested_symbol_universe"] == []
    assert result.payload["coverage_sufficient_for_future_v12"] is False
    excluded = {row["symbol"]: row["reason"] for row in result.payload["excluded_symbols"]}
    assert "TONUSDT" in excluded
    assert "previous coverage" in excluded["TONUSDT"]


def test_empty_cache_file_cannot_open_future_v12_gate(tmp_path: Path) -> None:
    cache_root = tmp_path / "micro-cache"
    for symbol in ("BTCUSDT", "ETHUSDT", "SOLUSDT"):
        write_microstructure_cache(cache_root, data_family="open_interest", symbol=symbol, rows=0)
    result = evaluate_fixture(
        tmp_path,
        cache_root=cache_root,
        requested_symbols=("BTCUSDT", "ETHUSDT", "SOLUSDT"),
    )
    assert result.payload["coverage_sufficient_for_future_v12"] is False
    assert all(row["available"] is False for row in result.data_family_coverage if row["data_family"] == "open_interest")


def test_timestampless_cache_file_cannot_open_future_v12_gate(tmp_path: Path) -> None:
    cache_root = tmp_path / "micro-cache"
    for symbol in ("BTCUSDT", "ETHUSDT", "SOLUSDT"):
        write_microstructure_cache(
            cache_root,
            data_family="open_interest",
            symbol=symbol,
            rows=2,
            include_timestamp=False,
        )
    result = evaluate_fixture(
        tmp_path,
        cache_root=cache_root,
        requested_symbols=("BTCUSDT", "ETHUSDT", "SOLUSDT"),
    )
    assert result.payload["coverage_sufficient_for_future_v12"] is False
    assert all(
        "timestamp" in row["unavailable_reason"]
        for row in result.data_family_coverage
        if row["data_family"] == "open_interest"
    )


def test_out_of_range_cache_file_cannot_open_future_v12_gate(tmp_path: Path) -> None:
    cache_root = tmp_path / "micro-cache"
    for symbol in ("BTCUSDT", "ETHUSDT", "SOLUSDT"):
        write_microstructure_cache(cache_root, data_family="open_interest", symbol=symbol)
    result = evaluate_fixture(
        tmp_path,
        cache_root=cache_root,
        requested_symbols=("BTCUSDT", "ETHUSDT", "SOLUSDT"),
    )
    assert result.payload["coverage_sufficient_for_future_v12"] is False
    assert all(
        "target range" in row["unavailable_reason"]
        for row in result.data_family_coverage
        if row["data_family"] == "open_interest"
    )


def test_symbol_and_readiness_by_family_schemas_are_stable(tmp_path: Path) -> None:
    result = evaluate_fixture(tmp_path)
    assert list(pd.DataFrame(result.symbol_data_coverage).columns) == list(SYMBOL_DATA_COVERAGE_COLUMNS)
    assert list(pd.DataFrame(result.readiness_by_family).columns) == list(READINESS_BY_FAMILY_COLUMNS)
    row = result.readiness_by_family[0]
    assert row["coverage_gate"] == "future_v12_input_coverage_only"
    assert row["not_strategy_readiness"] is True


def test_v11_does_not_rerun_prior_loop_evaluators(tmp_path: Path, monkeypatch) -> None:
    def explode(*_args: Any, **_kwargs: Any) -> None:
        raise AssertionError("prior loop evaluator must not be called by V11")

    monkeypatch.setattr(quantctl, "evaluate_v9_falsification_lineage_governance", explode)
    monkeypatch.setattr(quantctl, "evaluate_multi_asset_strict_oos_fdr_falsification", explode)
    monkeypatch.setattr(quantctl, "evaluate_panel_ready_multi_asset_alpha_prefilter_rerun", explode)
    result = evaluate_fixture(tmp_path)
    assert result.payload["alpha_generation_performed"] is False


def test_source_v10_artifact_mtime_is_unchanged(tmp_path: Path) -> None:
    v10 = v10_fixture(tmp_path)
    before = {
        path.name: os.stat(path).st_mtime_ns
        for path in v10.iterdir()
        if path.is_file()
    }
    evaluate_fixture(tmp_path, source_v10_artifact_path=v10)
    after = {
        path.name: os.stat(path).st_mtime_ns
        for path in v10.iterdir()
        if path.is_file()
    }
    assert after == before


def test_written_outputs_do_not_emit_forbidden_positive_labels(tmp_path: Path) -> None:
    result = evaluate_fixture(tmp_path)
    out = write_public_derivatives_microstructure_axis_artifacts(tmp_path / "v11", result)
    forbidden = {
        "READY",
        "TRADE",
        "LIVE_READY",
        "PROFITABLE",
        "DEPLOY",
        "CAPITAL_ALLOCATE",
        "EDGE_CONFIRMED",
        "ALPHA_CONFIRMED",
        "EDGE_PROVEN",
    }
    for path in out.iterdir():
        if path.name == "readiness_by_family.csv":
            continue
        text = path.read_text()
        for label in forbidden:
            assert not re.search(rf"(?<![A-Z_]){re.escape(label)}(?![A-Z_])", text), (
                f"{label} leaked into {path}"
            )


def v11_fixture(tmp_path: Path) -> Path:
    out = tmp_path / "v11"
    out.mkdir(exist_ok=True)
    payload = {
        "loop_namespace": "public_derivatives_microstructure_data_axis_v11_only_not_alpha",
        "suite_decision": "MICROSTRUCTURE_AXIS_NOT_READY_INSUFFICIENT_SYMBOLS",
        "readiness_baseline_verdict": "NOT_READY",
        "coverage_sufficient_for_future_v12": False,
        "v10_kill_state_preserved": True,
        "v9_next_queue_empty": True,
        "data_families_available": ["funding_rate_extensions"],
        "usable_symbol_universe": [],
        "negative_control_families": ["correlation_breakdown"],
        "disallowed_revival_entries": ["correlation_breakdown"],
        "alpha_generation_performed": False,
        "tsmom_state_changed": False,
        "private_data_used": False,
        "account_data_used": False,
        "api_key_used": False,
        "live_or_testnet_path_created": False,
    }
    (out / "public_derivatives_microstructure_axis.json").write_text(json.dumps(payload, indent=2) + "\n")
    return out


def write_kline_proxy_zip(
    cache_dir: Path,
    *,
    symbol: str,
    month: str = "2024-01",
    include_microstructure_columns: bool = True,
) -> Path:
    path = cache_dir / "futures" / "um" / "monthly" / "klines" / symbol / "1h" / f"{symbol}-1h-{month}.zip"
    path.parent.mkdir(parents=True, exist_ok=True)
    start = pd.Timestamp(f"{month}-01 00:00:00", tz="UTC")
    end = (start + pd.offsets.MonthEnd(0)).replace(hour=23)
    header = [
        "open_time",
        "open",
        "high",
        "low",
        "close",
        "volume",
        "close_time",
    ]
    if include_microstructure_columns:
        header.extend(["quote_volume", "count", "taker_buy_volume", "taker_buy_quote_volume"])
    header.append("ignore")
    rows: list[list[str]] = []
    for ts in (start, end):
        open_ms = int(cast(pd.Timestamp, ts).timestamp() * 1000)
        row = [
            str(open_ms),
            "1",
            "2",
            "0.5",
            "1.5",
            "10",
            str(open_ms + 3_599_999),
        ]
        if include_microstructure_columns:
            row.extend(["100", "7", "4", "40"])
        row.append("0")
        rows.append(row)
    body = ",".join(header) + "\n" + "\n".join(",".join(row) for row in rows) + "\n"
    with zipfile.ZipFile(path, mode="w") as zf:
        zf.writestr(f"{symbol}-1h-{month}.csv", body)
    return path


def write_recent_endpoint_cache(
    cache_root: Path,
    *,
    symbol: str,
    data_family: str = "open_interest_statistics_recent",
    start_ms: int = 1_704_067_200_000,
    end_ms: int = 1_704_153_600_000,
) -> Path:
    path = (
        cache_root
        / data_family
        / symbol
        / "1h"
        / f"{symbol}-{data_family}-1h-{start_ms}-{end_ms}.json"
    )
    path.parent.mkdir(parents=True, exist_ok=True)
    payload = [
        {
            "symbol": symbol,
            "sumOpenInterest": "10",
            "sumOpenInterestValue": "100",
            "timestamp": start_ms,
        },
        {
            "symbol": symbol,
            "sumOpenInterest": "11",
            "sumOpenInterestValue": "101",
            "timestamp": end_ms,
        },
    ]
    path.write_text(json.dumps(payload) + "\n")
    return path


def recent_fixture(tmp_path: Path, **kwargs: Any):
    source_v11 = kwargs.pop("source_v11_artifact_path", None)
    if source_v11 is None:
        source_v11 = v11_fixture(tmp_path)
    return evaluate_public_derivatives_microstructure_recent_window_connector(
        source_v11_artifact_path=source_v11,
        binance_vision_cache_dir=kwargs.pop("binance_vision_cache_dir", tmp_path / "vision-cache"),
        recent_cache_root=kwargs.pop("recent_cache_root", tmp_path / "recent-cache"),
        commands=["uv run quantctl public-derivatives-microstructure-recent-window-run"],
        changed_files=[
            "src/parangsae/research/public_derivatives_microstructure.py",
            "data/recent-window-cache.json",
        ],
        download_recent=kwargs.pop("download_recent", False),
        **kwargs,
    )


def test_v12_reads_v11_artifact_and_preserves_state(tmp_path: Path) -> None:
    result = recent_fixture(tmp_path)
    assert result.payload["v11_state_preserved"] is True
    assert result.payload["source_v11_artifact_path"].endswith("v11")
    assert result.payload["source_v11_suite_decision"] == "MICROSTRUCTURE_AXIS_NOT_READY_INSUFFICIENT_SYMBOLS"
    assert result.payload["source_v11_coverage_sufficient_for_future_v12"] is False
    assert result.payload["source_v11_payload_hash"]


def test_v12_tonusdt_remains_excluded_with_reason(tmp_path: Path) -> None:
    result = recent_fixture(tmp_path, requested_symbols=("TONUSDT", "BTCUSDT"))
    assert "TONUSDT" not in result.payload["requested_symbol_universe"]
    excluded = {row["symbol"]: row["reason"] for row in result.payload["excluded_symbols"]}
    assert "TONUSDT" in excluded
    assert "previous coverage" in excluded["TONUSDT"]


def test_v12_endpoint_feasibility_schema_and_retention_boundary(tmp_path: Path) -> None:
    result = recent_fixture(tmp_path)
    frame = pd.DataFrame(result.endpoint_feasibility)
    assert list(frame.columns) == list(RECENT_WINDOW_ENDPOINT_FEASIBILITY_COLUMNS)
    boundaries = {row["data_family"]: row["retention_boundary"] for row in result.endpoint_feasibility}
    assert "latest_one_month" in boundaries["open_interest_statistics_recent"]
    assert "latest_30_days" in boundaries["global_long_short_ratio_recent"]


def test_v12_recent_window_not_treated_as_historical_coverage(tmp_path: Path) -> None:
    cache_root = tmp_path / "recent-cache"
    for symbol in ("BTCUSDT", "ETHUSDT", "SOLUSDT"):
        write_recent_endpoint_cache(
                cache_root,
                symbol=symbol,
                start_ms=1_780_012_800_000,
                end_ms=1_782_604_800_000,
            )
    result = recent_fixture(
        tmp_path,
        recent_cache_root=cache_root,
        requested_symbols=("BTCUSDT", "ETHUSDT", "SOLUSDT"),
        data_families=("open_interest_statistics_recent",),
        recent_start="2026-05-29",
        recent_end="2026-06-28",
    )
    assert result.payload["recent_window_microstructure_readiness"] is True
    assert result.payload["historical_microstructure_proxy_readiness"] is False
    assert result.payload["coverage_sufficient_for_future_v13"] is True
    assert result.payload["recent_window_only"] is True
    assert result.payload["temporal_mismatch_warning"]


def test_v12_kline_proxy_uses_existing_public_kline_columns(tmp_path: Path) -> None:
    cache_dir = tmp_path / "vision-cache"
    for symbol in ("BTCUSDT", "ETHUSDT", "SOLUSDT"):
        write_kline_proxy_zip(cache_dir, symbol=symbol)
    result = recent_fixture(
        tmp_path,
        binance_vision_cache_dir=cache_dir,
        requested_symbols=("BTCUSDT", "ETHUSDT", "SOLUSDT"),
        target_start="2024-01-01",
        target_end="2024-01-31",
        data_families=("kline_derived_microstructure_proxy",),
    )
    assert result.payload["historical_microstructure_proxy_readiness"] is True
    assert list(pd.DataFrame(result.kline_microstructure_proxy_coverage).columns) == list(
        RECENT_WINDOW_KLINE_PROXY_COVERAGE_COLUMNS
    )
    row = result.kline_microstructure_proxy_coverage[0]
    assert row["available"] is True
    assert "taker_buy_volume_ratio" in row["schema_columns"]


def test_v12_missing_kline_microstructure_columns_are_not_fabricated(tmp_path: Path) -> None:
    cache_dir = tmp_path / "vision-cache"
    write_kline_proxy_zip(cache_dir, symbol="BTCUSDT", include_microstructure_columns=False)
    result = recent_fixture(
        tmp_path,
        binance_vision_cache_dir=cache_dir,
        requested_symbols=("BTCUSDT",),
        target_start="2024-01-01",
        target_end="2024-01-31",
        data_families=("kline_derived_microstructure_proxy",),
    )
    row = result.kline_microstructure_proxy_coverage[0]
    assert row["available"] is False
    assert "missing required raw kline microstructure columns" in row["unavailable_reason"]
    assert "taker_buy_volume_ratio" not in row["schema_columns"]


def test_v12_unavailable_endpoint_records_reason(tmp_path: Path) -> None:
    result = recent_fixture(
        tmp_path,
        requested_symbols=("BTCUSDT",),
        data_families=("open_interest_statistics_recent",),
    )
    row = result.recent_window_coverage[0]
    assert row["available"] is False
    assert row["unavailable_reason"]
    assert row["requires_api_key"] is False


def test_v12_failed_download_is_recorded_safely(tmp_path: Path) -> None:
    def fail_fetch(_request: dict[str, Any]) -> list[dict[str, Any]]:
        raise RuntimeError("network unavailable")

    result = recent_fixture(
        tmp_path,
        requested_symbols=("BTCUSDT",),
        data_families=("open_interest_statistics_recent",),
        download_recent=True,
        endpoint_fetcher=fail_fetch,
    )
    assert result.payload["failed_download_count"] == 1
    assert result.failed_downloads[0]["error_type"] == "RuntimeError"
    assert result.recent_window_coverage[0]["available"] is False


def test_v12_idempotent_cache_skip_behavior(tmp_path: Path) -> None:
    calls: list[str] = []

    def fetch(request: dict[str, Any]) -> list[dict[str, Any]]:
        calls.append(str(request["symbol"]))
        return [{"symbol": request["symbol"], "timestamp": int(request["start_time_ms"]), "value": "1"}]

    cache_root = tmp_path / "recent-cache"
    first = recent_fixture(
        tmp_path,
        recent_cache_root=cache_root,
        requested_symbols=("BTCUSDT",),
        data_families=("open_interest_statistics_recent",),
        download_recent=True,
        endpoint_fetcher=fetch,
    )
    second = recent_fixture(
        tmp_path,
        recent_cache_root=cache_root,
        requested_symbols=("BTCUSDT",),
        data_families=("open_interest_statistics_recent",),
        download_recent=True,
        endpoint_fetcher=fetch,
    )
    assert first.payload["successful_download_count"] == 1
    assert second.payload["skipped_existing_count"] == 1
    assert calls == ["BTCUSDT"]


def test_v12_raw_data_files_are_not_added_to_changed_files(tmp_path: Path) -> None:
    result = recent_fixture(tmp_path)
    assert "data/recent-window-cache.json" not in result.payload["changed_files"]
    assert result.payload["raw_data_files_created"] == ["data/recent-window-cache.json"]
    assert result.payload["raw_data_files_committed"] is False


def test_v12_historical_proxy_readiness_requires_three_symbols(tmp_path: Path) -> None:
    cache_dir = tmp_path / "vision-cache"
    for symbol in ("BTCUSDT", "ETHUSDT"):
        write_kline_proxy_zip(cache_dir, symbol=symbol)
    two = recent_fixture(
        tmp_path,
        binance_vision_cache_dir=cache_dir,
        requested_symbols=("BTCUSDT", "ETHUSDT"),
        target_start="2024-01-01",
        target_end="2024-01-31",
        data_families=("kline_derived_microstructure_proxy",),
    )
    assert two.payload["historical_microstructure_proxy_readiness"] is False

    write_kline_proxy_zip(cache_dir, symbol="SOLUSDT")
    three = recent_fixture(
        tmp_path,
        binance_vision_cache_dir=cache_dir,
        requested_symbols=("BTCUSDT", "ETHUSDT", "SOLUSDT"),
        target_start="2024-01-01",
        target_end="2024-01-31",
        data_families=("kline_derived_microstructure_proxy",),
    )
    assert three.payload["historical_microstructure_proxy_readiness"] is True


def test_v12_recent_window_readiness_requires_three_symbols(tmp_path: Path) -> None:
    cache_root = tmp_path / "recent-cache"
    for symbol in ("BTCUSDT", "ETHUSDT"):
        write_recent_endpoint_cache(
                cache_root,
                symbol=symbol,
                start_ms=1_780_012_800_000,
                end_ms=1_782_604_800_000,
            )
    two = recent_fixture(
        tmp_path,
        recent_cache_root=cache_root,
        requested_symbols=("BTCUSDT", "ETHUSDT"),
        data_families=("open_interest_statistics_recent",),
        recent_start="2026-05-29",
        recent_end="2026-06-28",
    )
    assert two.payload["recent_window_microstructure_readiness"] is False

    write_recent_endpoint_cache(
        cache_root,
        symbol="SOLUSDT",
        start_ms=1_780_012_800_000,
        end_ms=1_782_604_800_000,
    )
    three = recent_fixture(
        tmp_path,
        recent_cache_root=cache_root,
        requested_symbols=("BTCUSDT", "ETHUSDT", "SOLUSDT"),
        data_families=("open_interest_statistics_recent",),
        recent_start="2026-05-29",
        recent_end="2026-06-28",
    )
    assert three.payload["recent_window_microstructure_readiness"] is True


def test_v12_stale_recent_endpoint_cache_does_not_open_recent_gate(tmp_path: Path) -> None:
    cache_root = tmp_path / "recent-cache"
    for symbol in ("BTCUSDT", "ETHUSDT", "SOLUSDT"):
        write_recent_endpoint_cache(cache_root, symbol=symbol)
    result = recent_fixture(
        tmp_path,
        recent_cache_root=cache_root,
        requested_symbols=("BTCUSDT", "ETHUSDT", "SOLUSDT"),
        data_families=("open_interest_statistics_recent",),
        recent_start="2026-05-29",
        recent_end="2026-06-28",
    )
    assert result.payload["recent_window_microstructure_readiness"] is False
    assert result.payload["coverage_sufficient_for_future_v13"] is False
    assert all(row["available"] is False for row in result.recent_window_coverage)
    assert all("does not cover requested recent window" in row["unavailable_reason"] for row in result.recent_window_coverage)


def test_v12_no_private_account_api_key_order_or_fill_path_exists(tmp_path: Path) -> None:
    result = recent_fixture(tmp_path)
    assert result.payload["private_data_used"] is False
    assert result.payload["account_data_used"] is False
    assert result.payload["api_key_used"] is False
    assert result.payload["live_or_testnet_path_created"] is False
    assert result.payload["safety_gates"]["no_private_order_or_fill_data_used"] is True


def test_v12_alpha_generation_remains_false(tmp_path: Path) -> None:
    result = recent_fixture(tmp_path)
    assert result.payload["alpha_generation_performed"] is False
    assert result.payload["strategy_optimization_performed"] is False


def test_v12_artifact_writing_and_artifact_type_detection(tmp_path: Path) -> None:
    result = recent_fixture(tmp_path)
    out = write_public_derivatives_microstructure_recent_window_artifacts(tmp_path / "v12", result)
    for filename in PUBLIC_DERIVATIVES_MICROSTRUCTURE_RECENT_WINDOW_REQUIRED_FILES:
        assert (out / filename).exists(), filename
    assert (out / "loop.json").exists()
    payload = json.loads((out / "public_derivatives_microstructure_recent_window.json").read_text())
    assert payload["readiness_baseline_verdict"] == "NOT_READY"
    assert existing_loop_artifact_type(out) == "public-derivatives-microstructure-recent-window-run"


def test_v12_cli_no_credential_no_live_path(tmp_path: Path, capsys) -> None:
    args = Namespace(
        source_v11_artifact=str(v11_fixture(tmp_path)),
        symbols="BTCUSDT",
        data_family=None,
        binance_vision_cache_dir=str(tmp_path / "vision-cache"),
        recent_cache_root=str(tmp_path / "recent-cache"),
        interval="1h",
        target_start="2024-01-01",
        target_end="2024-01-31",
        recent_start="2026-05-29",
        recent_end="2026-06-28",
        minimum_required_symbols=3,
        download_recent=False,
        force=False,
        reports_root=str(tmp_path / "reports"),
        output_dir=str(tmp_path / "v12"),
        overwrite=False,
        command=None,
        changed_file=None,
    )
    assert quantctl.cmd_public_derivatives_microstructure_recent_window_run(args) == 0
    output = json.loads(capsys.readouterr().out)
    assert output["run_type"] == "public-derivatives-microstructure-recent-window-run"
    assert output["live_orders_submitted"] is False
    assert output["testnet_orders_submitted"] is False
    assert output["api_keys_required_for_research"] is False


def test_v12_readiness_baseline_verdict_remains_not_ready(tmp_path: Path) -> None:
    result = recent_fixture(tmp_path)
    assert result.payload["readiness_baseline_verdict"] == "NOT_READY"


def test_v12_tsmom_state_unchanged(tmp_path: Path) -> None:
    result = recent_fixture(tmp_path)
    assert result.payload["tsmom_state_changed"] is False


def test_v8_v9_v10_v11_v12_commands_remain_wired_importable() -> None:
    parser = quantctl.build_parser()
    commands = cast(Any, parser)._subparsers._group_actions[0].choices
    for command in (
        "panel-ready-multi-asset-alpha-prefilter-rerun",
        "multi-asset-strict-oos-fdr-falsification-run",
        "multi-asset-v9-falsification-lineage-governance-run",
        "public-derivatives-microstructure-data-axis-run",
        "public-derivatives-microstructure-recent-window-run",
    ):
        assert command in commands


def test_v12_forbidden_positive_labels_cannot_be_emitted(tmp_path: Path) -> None:
    result = recent_fixture(tmp_path)
    out = write_public_derivatives_microstructure_recent_window_artifacts(tmp_path / "v12", result)
    for path in out.iterdir():
        if path.name == "readiness_by_family.csv":
            continue
        text = path.read_text()
        for label in {"READY", "TRADE", "LIVE_READY", "PROFITABLE", "DEPLOY", "CAPITAL_ALLOCATE", "EDGE_CONFIRMED"}:
            assert not re.search(rf"(?<![A-Z_]){re.escape(label)}(?![A-Z_])", text), (
                f"{label} leaked into {path}"
            )
