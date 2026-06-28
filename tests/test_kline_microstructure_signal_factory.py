from __future__ import annotations

import json
import zipfile
from pathlib import Path
from typing import Any

import pandas as pd
import pytest

from parangsae.apps.cli import quantctl
from parangsae.marketdata.binance_vision import canonical_cache_path
from parangsae.research.kline_microstructure_signal_factory import (
    HISTORICAL_KLINE_MICROSTRUCTURE_ALLOWED_SIGNAL_LABELS,
    HISTORICAL_KLINE_MICROSTRUCTURE_DEFAULT_UNIVERSE,
    HISTORICAL_KLINE_MICROSTRUCTURE_DERIVED_COLUMNS,
    HISTORICAL_KLINE_MICROSTRUCTURE_FORBIDDEN_OUTPUT_LABELS,
    HISTORICAL_KLINE_MICROSTRUCTURE_PANEL_COLUMNS,
    build_historical_kline_microstructure_panel,
    build_historical_kline_microstructure_signal_series,
    default_historical_kline_microstructure_signal_factory_output_dir,
    duplicate_correlation_clusters_for_historical_kline_microstructure,
    evaluate_historical_kline_microstructure_signal_factory,
    generate_historical_kline_microstructure_signal_specs,
    lag_multi_asset_signal_for_labels,
    read_v12_historical_kline_microstructure_source_state,
    write_historical_kline_microstructure_signal_factory_artifacts,
)
from parangsae.research.loop_artifacts import existing_loop_artifact_type


def write_raw_kline_zip(
    cache_root: Path,
    *,
    symbol: str,
    month: str = "2024-01",
    rows: int = 96,
    include_microstructure_columns: bool = True,
) -> Path:
    path = canonical_cache_path(cache_root, symbol, "1h", month)
    path.parent.mkdir(parents=True, exist_ok=True)
    ts = pd.date_range(f"{month}-01", periods=rows, freq="1h", tz="UTC")
    frame = pd.DataFrame(
        {
            "open_time": [int(item.timestamp() * 1000) for item in ts],
            "open": [100 + i * 0.1 for i in range(rows)],
            "high": [101 + i * 0.1 for i in range(rows)],
            "low": [99 + i * 0.1 for i in range(rows)],
            "close": [100.5 + i * 0.1 for i in range(rows)],
            "volume": [1000 + i for i in range(rows)],
            "close_time": [int((item + pd.Timedelta(hours=1)).timestamp() * 1000 - 1) for item in ts],
            "quote_volume": [100000 + i * 10 for i in range(rows)],
            "count": [100 + i % 7 for i in range(rows)],
            "taker_buy_volume": [500 + i * 0.3 for i in range(rows)],
            "taker_buy_quote_volume": [50000 + i * 5 for i in range(rows)],
            "ignore": [0 for _ in range(rows)],
        }
    )
    if not include_microstructure_columns:
        frame = frame.drop(columns=["quote_volume", "count", "taker_buy_volume", "taker_buy_quote_volume"])
    with zipfile.ZipFile(path, "w", compression=zipfile.ZIP_DEFLATED) as zf:
        zf.writestr(path.with_suffix(".csv").name, frame.to_csv(index=False))
    return path


def v12_fixture(
    tmp_path: Path,
    *,
    ready: bool = True,
    symbols: tuple[str, ...] = HISTORICAL_KLINE_MICROSTRUCTURE_DEFAULT_UNIVERSE,
) -> Path:
    out = tmp_path / "v12"
    out.mkdir()
    payload: dict[str, Any] = {
        "loop_namespace": "public_derivatives_microstructure_recent_window_v12_only_not_alpha",
        "suite_decision": "MICROSTRUCTURE_PROXY_READY_FOR_V13" if ready else "MICROSTRUCTURE_AXIS_NOT_READY_INSUFFICIENT_SYMBOLS",
        "readiness_baseline_verdict": "NOT_READY",
        "coverage_sufficient_for_future_v13": ready,
        "historical_microstructure_proxy_readiness": ready,
        "recent_window_microstructure_readiness": False,
        "requested_symbol_universe": list(symbols),
        "usable_symbol_universe": list(symbols if ready else symbols[:1]),
        "excluded_symbols": [
            {"symbol": "TONUSDT", "reason": "previous coverage left TONUSDT missing in V7/V8 public panel artifacts"}
        ],
        "private_data_used": False,
        "account_data_used": False,
        "api_key_used": False,
        "live_or_testnet_path_created": False,
        "raw_data_files_committed": False,
    }
    (out / "public_derivatives_microstructure_recent_window.json").write_text(json.dumps(payload, indent=2) + "\n")
    coverage_rows = [
        {
            "symbol": symbol,
            "data_family": "kline_derived_microstructure_proxy",
            "source_endpoint_or_cache": "cache",
            "public_source_only": True,
            "requires_api_key": False,
            "available": ready,
            "retention_boundary": "archive",
            "historical_backfill_supported": True,
            "recent_window_supported": False,
            "first_timestamp": "2024-01-01T00:00:00Z",
            "last_timestamp": "2024-01-04T23:00:00Z",
            "observation_count": 96,
            "expected_observation_count_if_known": 96,
            "coverage_ratio_if_known": 1.0,
            "missing_ranges": "",
            "unavailable_reason": "" if ready else "missing",
            "schema_columns": "open_time,open,high,low,close,volume,quote_volume,count,taker_buy_volume,taker_buy_quote_volume",
            "provenance_hash_if_available": f"hash-{symbol}",
            "raw_cache_files_created": "",
            "raw_cache_files_committed": False,
            "included_for_future_v13": ready,
            "exclusion_reason": "",
        }
        for symbol in symbols
    ]
    pd.DataFrame(coverage_rows).to_csv(out / "kline_microstructure_proxy_coverage.csv", index=False)
    pd.DataFrame(
        [
            {
                "data_family": "kline_derived_microstructure_proxy",
                "available_symbol_count": len(symbols) if ready else 1,
                "included_symbol_count_for_future_v13": len(symbols) if ready else 1,
                "common_start_by_family": "2024-01-01T00:00:00Z",
                "common_end_by_family": "2024-01-04T23:00:00Z",
                "historical_microstructure_proxy_readiness": ready,
                "recent_window_microstructure_readiness": False,
                "coverage_sufficient_for_future_v13": ready,
                "recent_window_only": False,
                "claim_boundary": "paper-only",
            }
        ]
    ).to_csv(out / "readiness_by_family.csv", index=False)
    pd.DataFrame(coverage_rows).to_csv(out / "provenance_manifest.csv", index=False)
    pd.DataFrame([{"reference_type": "v11_payload", "preserved": True}]).to_csv(out / "v11_reference.csv", index=False)
    return out


def ready_panel(tmp_path: Path, *, include_microstructure_columns: bool = True) -> tuple[Path, Path]:
    source_v12 = v12_fixture(tmp_path)
    cache = tmp_path / "binance-vision"
    for symbol in HISTORICAL_KLINE_MICROSTRUCTURE_DEFAULT_UNIVERSE:
        write_raw_kline_zip(cache, symbol=symbol, include_microstructure_columns=include_microstructure_columns)
    return source_v12, cache


def test_v12_artifact_is_read(tmp_path: Path) -> None:
    source = v12_fixture(tmp_path)
    state = read_v12_historical_kline_microstructure_source_state(source)
    assert state["usable_symbols"] == HISTORICAL_KLINE_MICROSTRUCTURE_DEFAULT_UNIVERSE
    assert state["payload"]["coverage_sufficient_for_future_v13"] is True


def test_v13_refuses_to_run_if_v12_not_ready(tmp_path: Path) -> None:
    source = v12_fixture(tmp_path, ready=False)
    with pytest.raises(ValueError, match="PRECONDITION_FAILED_SOURCE_V12_NOT_READY"):
        read_v12_historical_kline_microstructure_source_state(source)


def test_v13_uses_historical_not_recent_readiness(tmp_path: Path) -> None:
    source, cache = ready_panel(tmp_path)
    result = evaluate_historical_kline_microstructure_signal_factory(
        source_v12_artifact_path=source,
        binance_vision_cache_dir=cache,
        candidate_budget=20,
        target_end="2024-01-04",
    )
    assert result.payload["source_v12_historical_microstructure_proxy_readiness"] is True
    assert result.payload["source_v12_recent_window_microstructure_readiness"] is False
    assert result.payload["suite_decision"] in {
        "NO_SURVIVING_SIGNAL_FAMILY",
        "WATCH_FAMILY_ONLY_PAPER_RESEARCH",
        "NEEDS_STRICT_OOS_FALSIFICATION",
        "FAILS_NULL_ROBUSTNESS",
    }


def test_v13_rejects_symbols_not_approved_by_v12(tmp_path: Path) -> None:
    source = v12_fixture(tmp_path, symbols=("BTCUSDT", "ETHUSDT", "SOLUSDT"))
    cache = tmp_path / "cache"
    write_raw_kline_zip(cache, symbol="ADAUSDT")
    state = read_v12_historical_kline_microstructure_source_state(source)

    with pytest.raises(ValueError, match="approved by V12 historical kline proxy readiness"):
        build_historical_kline_microstructure_panel(
            source_v12_state=state,
            binance_vision_cache_dir=cache,
            target_end="2024-01-04",
            requested_symbols=("ADAUSDT",),
        )


def test_recent_window_data_used_remains_false(tmp_path: Path) -> None:
    source, cache = ready_panel(tmp_path)
    result = evaluate_historical_kline_microstructure_signal_factory(
        source_v12_artifact_path=source,
        binance_vision_cache_dir=cache,
        candidate_budget=10,
        target_end="2024-01-04",
    )
    assert result.payload["recent_window_data_used"] is False
    assert result.payload["data_download_performed"] is False


def test_temporal_mismatch_boundary_is_preserved(tmp_path: Path) -> None:
    source, cache = ready_panel(tmp_path)
    result = evaluate_historical_kline_microstructure_signal_factory(
        source_v12_artifact_path=source,
        binance_vision_cache_dir=cache,
        candidate_budget=10,
        target_end="2024-01-04",
    )
    assert "Recent-window endpoint data was not used" in result.payload["temporal_mismatch_warning"]


def test_tonusdt_remains_excluded() -> None:
    assert "TONUSDT" not in HISTORICAL_KLINE_MICROSTRUCTURE_DEFAULT_UNIVERSE


def test_no_extra_download_backfill_occurs(tmp_path: Path) -> None:
    source, cache = ready_panel(tmp_path)
    before = sorted(path.relative_to(cache) for path in cache.rglob("*") if path.is_file())
    result = evaluate_historical_kline_microstructure_signal_factory(
        source_v12_artifact_path=source,
        binance_vision_cache_dir=cache,
        candidate_budget=10,
        target_end="2024-01-04",
    )
    after = sorted(path.relative_to(cache) for path in cache.rglob("*") if path.is_file())
    assert before == after
    assert result.payload["data_download_performed"] is False


def test_kline_microstructure_panel_schema_is_stable(tmp_path: Path) -> None:
    source, cache = ready_panel(tmp_path)
    state = read_v12_historical_kline_microstructure_source_state(source)
    panel, summary, excluded = build_historical_kline_microstructure_panel(
        source_v12_state=state,
        binance_vision_cache_dir=cache,
        target_end="2024-01-04",
    )
    assert excluded == []
    for column in HISTORICAL_KLINE_MICROSTRUCTURE_DERIVED_COLUMNS:
        assert column in panel.columns
    for column in HISTORICAL_KLINE_MICROSTRUCTURE_PANEL_COLUMNS:
        assert column in panel.columns
    assert len(summary) == len(HISTORICAL_KLINE_MICROSTRUCTURE_DEFAULT_UNIVERSE)


def test_missing_raw_kline_microstructure_columns_are_not_fabricated(tmp_path: Path) -> None:
    source, cache = ready_panel(tmp_path, include_microstructure_columns=False)
    state = read_v12_historical_kline_microstructure_source_state(source)
    panel, summary, excluded = build_historical_kline_microstructure_panel(
        source_v12_state=state,
        binance_vision_cache_dir=cache,
        target_end="2024-01-04",
    )
    assert panel.empty
    assert summary == []
    assert all("missing required raw kline microstructure columns" in row["reason"] for row in excluded)


def test_deterministic_candidate_generation() -> None:
    first = generate_historical_kline_microstructure_signal_specs(("BTCUSDT", "ETHUSDT"), candidate_budget=50)
    second = generate_historical_kline_microstructure_signal_specs(("ETHUSDT", "BTCUSDT"), candidate_budget=50)
    assert [spec.signal_id for spec in first] == [spec.signal_id for spec in second]


def test_candidate_budget_and_max_depth_enforced() -> None:
    specs = generate_historical_kline_microstructure_signal_specs(("BTCUSDT", "ETHUSDT", "SOLUSDT"), candidate_budget=25, max_depth=2)
    assert len(specs) <= 25
    assert all(spec.depth <= 2 for spec in specs)
    with pytest.raises(ValueError):
        generate_historical_kline_microstructure_signal_specs(("BTCUSDT",), candidate_budget=20001)
    with pytest.raises(ValueError):
        generate_historical_kline_microstructure_signal_specs(("BTCUSDT",), max_depth=5)


def test_cross_sectional_ranks_use_only_same_timestamp_available_data(tmp_path: Path) -> None:
    source, cache = ready_panel(tmp_path)
    state = read_v12_historical_kline_microstructure_source_state(source)
    panel, _, _ = build_historical_kline_microstructure_panel(
        source_v12_state=state,
        binance_vision_cache_dir=cache,
        requested_symbols=("BTCUSDT", "ETHUSDT"),
        target_end="2024-01-04",
    )
    timestamp = panel["timestamp"].iloc[30]
    rank_count = panel.loc[panel["timestamp"] == timestamp, "cross_sectional_trade_count_rank"].notna().sum()
    assert rank_count == 2


def test_no_future_timestamps_or_symbols_used(tmp_path: Path) -> None:
    source, cache = ready_panel(tmp_path)
    result = evaluate_historical_kline_microstructure_signal_factory(
        source_v12_artifact_path=source,
        binance_vision_cache_dir=cache,
        requested_symbols=("BTCUSDT", "ETHUSDT"),
        candidate_budget=5,
        target_end="2024-01-04",
    )
    assert result.payload["symbol_universe_used"] == ["BTCUSDT", "ETHUSDT"]
    assert result.payload["common_end"] == "2024-01-04T00:00:00Z"


def test_decision_lag_prevents_same_bar_leakage(tmp_path: Path) -> None:
    source, cache = ready_panel(tmp_path)
    state = read_v12_historical_kline_microstructure_source_state(source)
    panel, _, _ = build_historical_kline_microstructure_panel(
        source_v12_state=state,
        binance_vision_cache_dir=cache,
        target_end="2024-01-04",
    )
    spec = generate_historical_kline_microstructure_signal_specs(("BTCUSDT",), candidate_budget=1)[0]
    raw = build_historical_kline_microstructure_signal_series(spec, panel)
    decision = lag_multi_asset_signal_for_labels(raw, lag_bars=spec.lag_bars)
    expected = raw.groupby(level="symbol").shift(spec.lag_bars)
    pd.testing.assert_series_equal(decision, expected)


def test_symbol_and_family_permuted_baselines_are_deterministic(tmp_path: Path) -> None:
    source, cache = ready_panel(tmp_path)
    first = evaluate_historical_kline_microstructure_signal_factory(
        source_v12_artifact_path=source,
        binance_vision_cache_dir=cache,
        candidate_budget=20,
        target_end="2024-01-04",
    )
    second = evaluate_historical_kline_microstructure_signal_factory(
        source_v12_artifact_path=source,
        binance_vision_cache_dir=cache,
        candidate_budget=20,
        target_end="2024-01-04",
    )
    assert first.null_baseline_comparison == second.null_baseline_comparison


def test_duplicate_representative_selection_is_non_performance_based(tmp_path: Path) -> None:
    source, cache = ready_panel(tmp_path)
    state = read_v12_historical_kline_microstructure_source_state(source)
    panel, _, _ = build_historical_kline_microstructure_panel(
        source_v12_state=state,
        binance_vision_cache_dir=cache,
        target_end="2024-01-04",
    )
    specs = generate_historical_kline_microstructure_signal_specs(("BTCUSDT",), candidate_budget=2)
    signals = {
        spec.signal_id: build_historical_kline_microstructure_signal_series(spec, panel)
        for spec in specs
    }
    clusters = duplicate_correlation_clusters_for_historical_kline_microstructure(signals, specs, threshold=0.0)
    representative = clusters[0]["representative_signal_id"]
    expected = sorted(specs, key=lambda spec: (spec.complexity_score, spec.depth, spec.signal_id))[0].signal_id
    assert representative == expected
    assert clusters[0]["representative_rule"] == "lowest_complexity_then_depth_then_lexicographic_signal_id"


def test_vectorized_correlation_path_is_used(tmp_path: Path) -> None:
    source, cache = ready_panel(tmp_path)
    result = evaluate_historical_kline_microstructure_signal_factory(
        source_v12_artifact_path=source,
        binance_vision_cache_dir=cache,
        candidate_budget=5,
        target_end="2024-01-04",
    )
    assert {row["correlation_engine"] for row in result.clusters} == {
        "vectorized_matrix_correlation_no_pairwise_pandas_corr_loop"
    }


def test_multiple_testing_warning_exists(tmp_path: Path) -> None:
    source, cache = ready_panel(tmp_path)
    result = evaluate_historical_kline_microstructure_signal_factory(
        source_v12_artifact_path=source,
        binance_vision_cache_dir=cache,
        candidate_budget=5,
        target_end="2024-01-04",
    )
    assert result.payload["cannot_claim_edge_due_to_multiple_testing"] is True
    assert result.multiple_testing_summary[0]["approximate_false_discovery_warning"] == "high"


def test_forbidden_labels_cannot_be_emitted() -> None:
    assert not (set(HISTORICAL_KLINE_MICROSTRUCTURE_ALLOWED_SIGNAL_LABELS) & HISTORICAL_KLINE_MICROSTRUCTURE_FORBIDDEN_OUTPUT_LABELS)


def test_artifact_writing(tmp_path: Path) -> None:
    source, cache = ready_panel(tmp_path)
    result = evaluate_historical_kline_microstructure_signal_factory(
        source_v12_artifact_path=source,
        binance_vision_cache_dir=cache,
        candidate_budget=5,
        target_end="2024-01-04",
        changed_files=["src/parangsae/research/kline_microstructure_signal_factory.py", "data/raw.csv"],
    )
    out = write_historical_kline_microstructure_signal_factory_artifacts(tmp_path / "out", result)
    assert (out / "historical_kline_microstructure_signal_factory.json").exists()
    assert (out / "next_strict_falsification_queue.csv").exists()
    assert existing_loop_artifact_type(out) == "historical-kline-microstructure-proxy-signal-factory-run"
    loop_payload = json.loads((out / "loop.json").read_text())
    failed_gate_rollup = pd.read_csv(out / "failed_gate_rollup.csv")
    assert loop_payload["failed_gates"]
    assert {row["gate"] for row in loop_payload["failed_gates"]} == set(failed_gate_rollup["gate"])
    changed_text = (out / "changed_files.txt").read_text()
    assert "src/parangsae/research/kline_microstructure_signal_factory.py" in changed_text
    assert "data/raw.csv" not in changed_text


def test_cli_no_credential_no_live_path(tmp_path: Path, capsys: pytest.CaptureFixture[str]) -> None:
    source, cache = ready_panel(tmp_path)
    args = quantctl.build_parser().parse_args(
        [
            "historical-kline-microstructure-proxy-signal-factory-run",
            "--source-v12-artifact",
            str(source),
            "--binance-vision-cache-dir",
            str(cache),
            "--target-end",
            "2024-01-04",
            "--candidate-budget",
            "5",
            "--output-dir",
            str(tmp_path / "cli-out"),
        ]
    )
    assert args.func(args) == 0
    payload = json.loads(capsys.readouterr().out)
    assert payload["live_orders_submitted"] is False
    assert payload["api_keys_required_for_research"] is False
    assert payload["private_data_used"] is False


def test_readiness_baseline_verdict_remains_not_ready(tmp_path: Path) -> None:
    source, cache = ready_panel(tmp_path)
    result = evaluate_historical_kline_microstructure_signal_factory(
        source_v12_artifact_path=source,
        binance_vision_cache_dir=cache,
        candidate_budget=5,
        target_end="2024-01-04",
    )
    assert result.payload["readiness_baseline_verdict"] == "NOT_READY"


def test_tsmom_state_unchanged(tmp_path: Path) -> None:
    source, cache = ready_panel(tmp_path)
    result = evaluate_historical_kline_microstructure_signal_factory(
        source_v12_artifact_path=source,
        binance_vision_cache_dir=cache,
        candidate_budget=5,
        target_end="2024-01-04",
    )
    assert result.payload["tsmom_state_changed"] is False


def test_v10_negative_controls_are_not_revived(tmp_path: Path) -> None:
    source, cache = ready_panel(tmp_path)
    result = evaluate_historical_kline_microstructure_signal_factory(
        source_v12_artifact_path=source,
        binance_vision_cache_dir=cache,
        candidate_budget=20,
        target_end="2024-01-04",
    )
    assert "correlation_breakdown" not in result.payload["families_evaluated"]
    assert "volatility_dispersion" not in result.payload["families_evaluated"]


def test_v11_v12_v13_commands_remain_wired() -> None:
    parser = quantctl.build_parser()
    for command in (
        "public-derivatives-microstructure-data-axis-run",
        "public-derivatives-microstructure-recent-window-run",
        "historical-kline-microstructure-proxy-signal-factory-run",
    ):
        args = parser.parse_args([command, "--help"]) if False else parser.parse_args([command])
        assert args.func is not None


def test_default_v13_output_dir() -> None:
    assert str(default_historical_kline_microstructure_signal_factory_output_dir(today="20260629")).endswith(
        "20260629-historical-kline-microstructure-proxy-signal-factory-v13"
    )
