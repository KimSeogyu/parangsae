from __future__ import annotations

import json
import os
from argparse import Namespace
from pathlib import Path
from typing import Any, cast

import pandas as pd
import pytest

from parangsae.apps.cli import quantctl
from parangsae.marketdata.binance_funding import FundingRateBundle
from parangsae.marketdata.binance_vision import MarketDataBundle
from parangsae.research.loop_artifacts import existing_loop_artifact_type
from parangsae.research.multi_asset_alpha_factory import (
    DEFAULT_SYMBOL_UNIVERSE,
    MULTI_ASSET_ALLOWED_SIGNAL_LABELS,
    MULTI_ASSET_FORBIDDEN_OUTPUT_LABELS,
    MULTI_ASSET_NEXT_QUEUE_COLUMNS,
    MULTI_ASSET_REQUIRED_FILES,
    MULTI_ASSET_SIGNAL_FAMILIES,
    MultiAssetSignalSpec,
    _multi_asset_signal_diagnostics,
    build_multi_asset_oos_folds,
    build_multi_asset_panel,
    default_multi_asset_alpha_prefilter_output_dir,
    deterministic_random_sign_signal,
    duplicate_correlation_clusters_for_multi_asset,
    evaluate_multi_asset_alpha_prefilter,
    generate_multi_asset_signal_specs,
    lag_multi_asset_signal_for_labels,
    prepare_multi_asset_alpha_prefilter_output_dir,
    shuffled_timestamp_signal,
    symbol_permuted_signal,
    write_multi_asset_alpha_prefilter_artifacts,
)


def timestamp_at(index: pd.Index, position: int) -> pd.Timestamp:
    return cast(pd.Timestamp, pd.Timestamp(cast(Any, index[position])))


def synthetic_asset_frame(
    symbol: str,
    *,
    rows: int = 220,
    start: str = "2024-01-01",
    drop_first: int = 0,
) -> pd.DataFrame:
    index = pd.date_range(start, periods=rows, freq="1h", tz="UTC")
    symbol_offset = float(sum(ord(char) for char in symbol) % 17)
    trend = pd.Series(range(rows), index=index, dtype=float) * (0.02 + symbol_offset / 5000.0)
    cycle = pd.Series([((i + int(symbol_offset)) % 24 - 12) * 0.04 for i in range(rows)], index=index)
    close = 100.0 + symbol_offset + trend + cycle
    frame = pd.DataFrame(
        {
            "Open": close.shift(1).fillna(close.iloc[0]),
            "High": close + 1.0,
            "Low": close - 1.0,
            "Close": close,
            "Volume": 1000.0 + symbol_offset * 10.0 + (pd.Series(range(rows), index=index) % 11) * 13.0,
        },
        index=index,
    )
    return frame.iloc[drop_first:].copy()


def market_bundle(symbol: str, frame: pd.DataFrame) -> MarketDataBundle:
    return MarketDataBundle(
        symbol=symbol,
        interval="1h",
        months=("2024-01",),
        frame=frame,
        downloads=(),
        normalized_sha256=f"ohlcv-{symbol}",
        quality={
            "closed_bar_only": True,
            "first_timestamp": timestamp_at(frame.index, 0).isoformat().replace("+00:00", "Z"),
            "last_timestamp": timestamp_at(frame.index, -1).isoformat().replace("+00:00", "Z"),
        },
    )


def funding_bundle(symbol: str, frame: pd.DataFrame) -> FundingRateBundle:
    funding_index = pd.DatetimeIndex(frame.index)[::8]
    funding = pd.DataFrame(
        {
            "symbol": [symbol for _ in funding_index],
            "funding_time_ms": [
                int(timestamp_at(funding_index, offset).timestamp() * 1000)
                for offset in range(len(funding_index))
            ],
            "funding_rate": [0.0001 if offset % 2 == 0 else -0.00005 for offset in range(len(funding_index))],
            "mark_price": [100.0 for _ in funding_index],
        },
        index=funding_index,
    )
    return FundingRateBundle(
        symbol=symbol,
        frame=funding,
        downloads=(),
        normalized_sha256=f"funding-{symbol}",
        quality={"rows": len(funding), "symbol": symbol},
    )


def v4_artifact(tmp_path: Path, *, non_empty_queue: bool = False) -> Path:
    out = tmp_path / "v4"
    out.mkdir()
    if non_empty_queue:
        (out / "next_protocol_queue.csv").write_text(
            "signal_family,horizon_bars,queue_decision\nmomentum,24,NEEDS_EXECUTION_AWARE_PROTOCOL\n"
        )
    else:
        (out / "next_protocol_queue.csv").write_text("signal_family,horizon_bars,queue_decision\n")
    return out


def test_panel_schema_exclusions_and_same_timestamp_cross_sectional_ranks() -> None:
    btc = market_bundle("BTCUSDT", synthetic_asset_frame("BTCUSDT", rows=80))
    eth = market_bundle("ETHUSDT", synthetic_asset_frame("ETHUSDT", rows=80, drop_first=10))
    panel_with_late_eth, summary = build_multi_asset_panel(
        {"BTCUSDT": btc, "ETHUSDT": eth},
        funding_bundles={"BTCUSDT": funding_bundle("BTCUSDT", btc.frame), "ETHUSDT": None},
        funding_status_by_symbol={"BTCUSDT": "available", "ETHUSDT": "unavailable_missing_cache"},
    )
    required = {
        "symbol",
        "timestamp",
        "open",
        "high",
        "low",
        "close",
        "volume",
        "return",
        "log_return",
        "realized_volatility",
        "rolling_range",
        "volume_zscore",
        "funding_rate",
        "funding_change",
        "funding_zscore",
        "funding_rank",
        "cross_sectional_return_rank",
        "cross_sectional_volume_rank",
        "cross_sectional_volatility_rank",
    }
    assert required.issubset(set(panel_with_late_eth.columns))
    assert {row["symbol"]: row["funding_status"] for row in summary}["ETHUSDT"] == "unavailable_missing_cache"

    early_ts = timestamp_at(btc.frame.index, 3)
    early_rows = panel_with_late_eth[panel_with_late_eth["timestamp"] == early_ts]
    assert set(early_rows["symbol"]) == {"BTCUSDT"}

    btc_only_panel, _ = build_multi_asset_panel({"BTCUSDT": btc})
    late_rank = float(
        cast(
            pd.Series,
            panel_with_late_eth[
                (panel_with_late_eth["timestamp"] == early_ts) & (panel_with_late_eth["symbol"] == "BTCUSDT")
            ]["cross_sectional_return_rank"],
        ).iloc[0]
    )
    btc_only_rank = float(
        cast(
            pd.Series,
            btc_only_panel[
                (btc_only_panel["timestamp"] == early_ts) & (btc_only_panel["symbol"] == "BTCUSDT")
            ]["cross_sectional_return_rank"],
        ).iloc[0]
    )
    assert late_rank == btc_only_rank


def test_candidate_generation_is_deterministic_budgeted_depth_bounded_and_not_v4_revival() -> None:
    first = generate_multi_asset_signal_specs(("BTCUSDT", "ETHUSDT", "SOLUSDT"), seed=7, candidate_budget=50, max_depth=4)
    second = generate_multi_asset_signal_specs(("BTCUSDT", "ETHUSDT", "SOLUSDT"), seed=7, candidate_budget=50, max_depth=4)
    assert [spec.to_dict() for spec in first] == [spec.to_dict() for spec in second]
    assert len(first) == 50
    assert max(spec.depth for spec in first) <= 4
    assert {spec.signal_family for spec in first}.issubset(set(MULTI_ASSET_SIGNAL_FAMILIES))
    assert all("forward" not in spec.signal_id and "label" not in spec.signal_id for spec in first)
    assert all(spec.signal_id.startswith("v5.") for spec in first)
    with pytest.raises(ValueError, match="candidate_budget"):
        generate_multi_asset_signal_specs(("BTCUSDT",), candidate_budget=20001)
    with pytest.raises(ValueError, match="max_depth"):
        generate_multi_asset_signal_specs(("BTCUSDT",), max_depth=5)
    payload = first[0].to_dict()
    with pytest.raises(ValueError, match="future data"):
        MultiAssetSignalSpec.from_dict({**payload, "uses_future_data": True})


def test_deterministic_baselines_and_decision_lag_are_no_lookahead() -> None:
    index = pd.MultiIndex.from_product(
        [pd.date_range("2024-01-01", periods=4, freq="1h", tz="UTC"), ["BTCUSDT", "ETHUSDT"]],
        names=["timestamp", "symbol"],
    )
    signal = pd.Series([1.0, 2.0, -3.0, -4.0, 5.0, 6.0, -7.0, -8.0], index=index)
    lagged = lag_multi_asset_signal_for_labels(signal, lag_bars=1)
    assert pd.isna(lagged.xs("BTCUSDT", level="symbol").iloc[0])
    assert lagged.xs("BTCUSDT", level="symbol").iloc[1] == signal.xs("BTCUSDT", level="symbol").iloc[0]

    random_a, seed_a = deterministic_random_sign_signal(signal, "v5-test", seed_base=99)
    random_b, seed_b = deterministic_random_sign_signal(signal, "v5-test", seed_base=99)
    shuffled_a, shuffle_seed_a = shuffled_timestamp_signal(signal, "v5-test", seed_base=99)
    shuffled_b, shuffle_seed_b = shuffled_timestamp_signal(signal, "v5-test", seed_base=99)
    symbol_a = symbol_permuted_signal(signal, "v5-test", seed_base=99)
    symbol_b = symbol_permuted_signal(signal, "v5-test", seed_base=99)
    assert seed_a == seed_b
    assert shuffle_seed_a == shuffle_seed_b
    pd.testing.assert_series_equal(random_a, random_b)
    pd.testing.assert_series_equal(shuffled_a, shuffled_b)
    pd.testing.assert_series_equal(symbol_a, symbol_b)
    assert symbol_a.index.equals(signal.index)


def test_leakage_check_ignores_all_nan_signals_but_fails_unshifted_signal() -> None:
    index = pd.MultiIndex.from_product(
        [pd.date_range("2024-01-01", periods=4, freq="1h", tz="UTC"), ["BTCUSDT"]],
        names=["timestamp", "symbol"],
    )
    spec = MultiAssetSignalSpec(
        signal_id="v5.cross_sectional_momentum.unit",
        signal_family="cross_sectional_momentum",
        feature_name="return_rank",
        symbol_scope=("BTCUSDT",),
        pair_scope=None,
        depth=1,
        max_depth=4,
        complexity_score=1.0,
        lag_bars=1,
        uses_funding=False,
        uses_future_data=False,
        generation_seed=1,
        generation_rule_id="unit",
    )
    labels = {1: pd.Series([0.1, -0.1, 0.2, -0.2], index=index)}

    all_nan = pd.Series([float("nan")] * len(index), index=index, dtype=float)
    all_nan_diag = _multi_asset_signal_diagnostics(
        spec,
        raw_signal=all_nan,
        decision_signal=lag_multi_asset_signal_for_labels(all_nan, lag_bars=1),
        forward_returns=labels,
        folds=[],
        funding_available=False,
    )
    assert all_nan_diag["leakage_check_status"] == "passed_shifted_by_decision_lag"
    assert all_nan_diag["failure_reason"] == "insufficient_non_null_observations"

    raw = pd.Series([1.0, 2.0, 3.0, 4.0], index=index)
    unshifted_diag = _multi_asset_signal_diagnostics(
        spec,
        raw_signal=raw,
        decision_signal=raw,
        forward_returns=labels,
        folds=[],
        funding_available=False,
    )
    assert unshifted_diag["leakage_check_status"] == "failed_decision_lag_mismatch"
    assert unshifted_diag["failure_reason"] == "decision_signal_not_shifted_by_expected_lag"


def test_duplicate_clustering_uses_vectorized_matrix_and_nonperformance_representative() -> None:
    index = pd.RangeIndex(6)
    signals = {
        "v5.cross_sectional_momentum.b_complex": pd.Series([1, 2, 3, 4, 5, 6], index=index, dtype=float),
        "v5.cross_sectional_momentum.a_simple": pd.Series([2, 4, 6, 8, 10, 12], index=index, dtype=float),
        "v5.cross_sectional_momentum.c_other": pd.Series([6, 5, 4, 3, 2, 1], index=index, dtype=float),
    }
    specs = [
        MultiAssetSignalSpec(
            signal_id=signal_id,
            signal_family="cross_sectional_momentum",
            feature_name="return_rank",
            symbol_scope=("BTCUSDT",),
            pair_scope=None,
            depth=1,
            max_depth=4,
            complexity_score=3.0 if "b_complex" in signal_id else 1.0 if "a_simple" in signal_id else 2.0,
            lag_bars=1,
            uses_funding=False,
            uses_future_data=False,
            generation_seed=1,
            generation_rule_id="unit",
        )
        for signal_id in signals
    ]
    clusters = duplicate_correlation_clusters_for_multi_asset(signals, specs, threshold=0.95)
    assert any(row["representative_signal_id"] == "v5.cross_sectional_momentum.a_simple" for row in clusters)
    assert all(row["correlation_engine"] == "vectorized_matrix_correlation_no_pairwise_pandas_corr_loop" for row in clusters)
    assert sum(1 for row in clusters if row["is_redundant_duplicate"]) >= 1


def test_chronological_oos_folds_are_ordered_purged_and_embargoed() -> None:
    bundles = {
        symbol: market_bundle(symbol, synthetic_asset_frame(symbol, rows=180))
        for symbol in ("BTCUSDT", "ETHUSDT")
    }
    panel, _ = build_multi_asset_panel(bundles)
    folds = build_multi_asset_oos_folds(panel, horizons=(2, 4), fold_count=2)
    assert folds
    for fold in folds:
        assert fold["train_start_pos"] < fold["train_end_pos"] < fold["validation_start_pos"]
        assert fold["validation_start_pos"] < fold["validation_end_pos"] < fold["test_start_pos"]
        assert fold["test_start_pos"] < fold["test_end_pos"]
        assert fold["validation_start_pos"] - fold["train_end_pos"] >= fold["purge_bars"] + fold["embargo_bars"]
        assert fold["test_start_pos"] - fold["validation_end_pos"] >= fold["purge_bars"] + fold["embargo_bars"]
        assert fold["label_overlap_allowed"] is False
        assert fold["holdout_retune_used"] is False


def test_v5_payload_artifacts_lineage_funding_missing_and_multiple_testing(tmp_path: Path) -> None:
    frames = {symbol: synthetic_asset_frame(symbol, rows=180) for symbol in ("BTCUSDT", "ETHUSDT", "SOLUSDT")}
    markets = {symbol: market_bundle(symbol, frame) for symbol, frame in frames.items()}
    result = evaluate_multi_asset_alpha_prefilter(
        market_bundles=markets,
        funding_bundles={"BTCUSDT": funding_bundle("BTCUSDT", frames["BTCUSDT"]), "ETHUSDT": None, "SOLUSDT": None},
        requested_symbols=("BTCUSDT", "ETHUSDT", "SOLUSDT", "DOGEUSDT"),
        excluded_symbols=({"symbol": "DOGEUSDT", "stage": "ohlcv_cache_availability", "reason": "missing cached OHLCV"},),
        funding_status_by_symbol={"BTCUSDT": "available", "ETHUSDT": "unavailable_missing_cache", "SOLUSDT": "invalid_unavailable"},
        source_v4_artifact_path=v4_artifact(tmp_path),
        seed=123,
        candidate_budget=90,
        max_depth=4,
        min_bars_per_symbol=50,
        horizons=(2, 4),
        fold_count=2,
        commands=["uv run quantctl multi-asset-alpha-generator-strict-prefilter-run"],
        changed_files=["src/parangsae/research/multi_asset_alpha_factory.py"],
    )
    payload = result.payload
    assert payload["readiness_baseline_verdict"] == "NOT_READY"
    assert payload["v4_queue_empty"] is True
    assert payload["v4_queue_status"] == "present_empty"
    assert payload["v4_families_revived"] is False
    assert payload["v4_candidates_reused"] is False
    assert payload["symbol_universe_used"] == ["BTCUSDT", "ETHUSDT", "SOLUSDT"]
    assert payload["families_evaluated"] == list(MULTI_ASSET_SIGNAL_FAMILIES)
    assert {row["signal_family"] for row in result.family_summary} == set(MULTI_ASSET_SIGNAL_FAMILIES)
    assert "ETHUSDT" not in {row["symbol"] for row in payload["excluded_symbols"]}
    funding_status = {row["symbol"]: row["funding_status"] for row in result.panel_summary}
    assert funding_status["ETHUSDT"] == "unavailable_missing_cache"
    assert payload["cannot_claim_edge_due_to_multiple_testing"] is True
    assert payload["multiple_testing_summary"]["cannot_claim_edge_due_to_multiple_testing"] is True
    assert "forbidden_outputs" not in payload["summary"]
    assert any(row["cannot_claim_edge_due_to_multiple_testing"] for row in result.multiple_testing_summary)
    assert payload["tsmom_state_changed"] is False
    assert payload["strategy_optimization_performed"] is False
    assert payload["live_or_testnet_path_created"] is False
    assert all(row["signal_label"] in MULTI_ASSET_ALLOWED_SIGNAL_LABELS for row in result.shortlist)
    assert not any(row["signal_label"] in MULTI_ASSET_FORBIDDEN_OUTPUT_LABELS for row in result.shortlist)

    out = write_multi_asset_alpha_prefilter_artifacts(tmp_path / "v5", result)
    assert {path.name for path in out.iterdir()} == set(MULTI_ASSET_REQUIRED_FILES)
    saved = json.loads((out / "multi_asset_alpha_factory.json").read_text())
    assert saved["readiness_baseline_verdict"] == "NOT_READY"
    assert saved["source_v4_artifact_path"].endswith("v4")
    assert (out / "next_strict_falsification_queue.csv").exists()
    assert (out / "next_strict_falsification_queue.csv").read_text().splitlines()[0] == ",".join(
        MULTI_ASSET_NEXT_QUEUE_COLUMNS
    )

    single_root = tmp_path / "single"
    single_root.mkdir()
    single_symbol = evaluate_multi_asset_alpha_prefilter(
        market_bundles={"BTCUSDT": markets["BTCUSDT"]},
        requested_symbols=("BTCUSDT",),
        source_v4_artifact_path=v4_artifact(single_root),
        seed=123,
        candidate_budget=40,
        max_depth=4,
        min_bars_per_symbol=50,
        horizons=(2, 4),
        fold_count=2,
    )
    relative_value = next(row for row in single_symbol.family_summary if row["signal_family"] == "relative_value_interaction")
    assert relative_value["signal_count"] == 0
    assert relative_value["unavailable_reason"] == "requires_at_least_two_symbols"

    missing_v4 = evaluate_multi_asset_alpha_prefilter(
        market_bundles=markets,
        requested_symbols=("BTCUSDT", "ETHUSDT"),
        source_v4_artifact_path=tmp_path / "missing-v4",
        seed=123,
        candidate_budget=20,
        max_depth=4,
        min_bars_per_symbol=50,
        horizons=(2, 4),
        fold_count=2,
    )
    assert missing_v4.payload["v4_queue_status"] == "missing"
    assert missing_v4.payload["v4_queue_empty"] is False
    assert any(
        row["gate"] == "v4_queue_audit_only" and row["status"] == "failed"
        for row in missing_v4.failed_gates
    )


def test_cli_v5_uses_public_no_credential_no_live_path(monkeypatch, tmp_path: Path) -> None:
    frames = {
        "BTCUSDT": synthetic_asset_frame("BTCUSDT", rows=180),
        "ETHUSDT": synthetic_asset_frame("ETHUSDT", rows=180),
    }

    def fake_market_loader(**kwargs):
        symbol = str(kwargs["symbol"])
        return market_bundle(symbol, frames[symbol])

    def fake_funding_loader(**kwargs):
        symbol = str(kwargs["symbol"])
        if symbol == "ETHUSDT":
            raise ValueError("funding intentionally unavailable")
        return funding_bundle(symbol, frames[symbol])

    args = Namespace(
        source_v4_artifact=str(v4_artifact(tmp_path)),
        symbols="BTCUSDT,ETHUSDT",
        interval="1h",
        months="2024-01",
        cache_dir="unused-cache",
        funding_source="public-binance",
        funding_cache_dir="unused-funding-cache",
        cache_mode="download-missing",
        seed=123,
        candidate_budget=60,
        max_depth=4,
        min_bars_per_symbol=50,
        reports_root=str(tmp_path / "reports"),
        output_dir=None,
        overwrite=False,
        command=None,
        changed_file=None,
    )
    monkeypatch.setattr(quantctl, "load_market_data_bundle", fake_market_loader)
    monkeypatch.setattr(quantctl, "fetch_funding_rate_history", fake_funding_loader)
    monkeypatch.setenv("BINANCE_API_KEY", "SHOULD_NOT_BE_READ")
    monkeypatch.setenv("BINANCE_API_SECRET", "SHOULD_NOT_BE_READ")
    real_getenv = os.getenv

    def guarded_getenv(key, default=None):
        if key in {"BINANCE_API_KEY", "BINANCE_API_SECRET"}:
            raise AssertionError(f"V5 must not read {key}")
        return real_getenv(key, default)

    monkeypatch.setattr(os, "getenv", guarded_getenv)
    assert quantctl.cmd_multi_asset_alpha_generator_strict_prefilter_run(args) == 0
    report_dirs = list((tmp_path / "reports" / "loops").iterdir())
    assert len(report_dirs) == 1
    payload = json.loads((report_dirs[0] / "multi_asset_alpha_factory.json").read_text())
    assert payload["readiness_baseline_verdict"] == "NOT_READY"
    assert payload["safety_gates"]["no_api_key_used"] is True
    assert payload["safety_gates"]["no_private_order_or_fill_data_used"] is True
    assert payload["live_or_testnet_path_created"] is False
    assert "ETHUSDT" in payload["symbol_universe_used"]
    assert not payload["excluded_symbols"]


def test_artifact_type_detection_and_overwrite_guards_for_v1_to_v5(tmp_path: Path) -> None:
    artifact_files = {
        "readiness-run-20260625T000000Z": ("readiness.json", "{}", "readiness-run"),
        "v1": ("alpha_signal_factory.json", '{"loop_namespace":"alpha_signal_factory_composition_harness_only_not_readiness"}', "alpha-signal-factory-composition-run"),
        "v2": ("alpha_signal_dag_expansion.json", '{"loop_namespace":"alpha_signal_dag_combinatorial_expansion_v2_only_not_readiness"}', "alpha-signal-dag-expansion-run"),
        "v3": ("alpha_signal_oos_fdr_validation.json", '{"loop_namespace":"alpha_signal_oos_fdr_validation_protocol_v3_only_not_readiness"}', "alpha-signal-oos-fdr-validation-run"),
        "v4": ("alpha_signal_strict_falsification.json", '{"loop_namespace":"alpha_signal_strict_falsification_protocol_v4_only_not_readiness"}', "alpha-signal-strict-falsification-run"),
        "v5": ("multi_asset_alpha_factory.json", '{"loop_namespace":"multi_asset_alpha_generator_strict_prefilter_v5_only_not_readiness"}', "multi-asset-alpha-generator-strict-prefilter-run"),
    }
    for dirname, (filename, body, expected) in artifact_files.items():
        path = tmp_path / dirname
        path.mkdir()
        (path / filename).write_text(body + "\n")
        assert existing_loop_artifact_type(path) == expected

    v4_path = tmp_path / "v4"
    with pytest.raises(FileExistsError, match="refusing to overwrite"):
        prepare_multi_asset_alpha_prefilter_output_dir(v4_path, overwrite=True)


def test_v1_v2_v3_v4_and_v5_commands_are_wired() -> None:
    parser = quantctl.build_parser()
    assert parser.parse_args(["alpha-signal-factory-composition-run"]).func is quantctl.cmd_alpha_signal_factory_composition_run
    assert parser.parse_args(["alpha-signal-dag-expansion-run"]).func is quantctl.cmd_alpha_signal_dag_expansion_run
    assert parser.parse_args(["alpha-signal-oos-fdr-validation-run"]).func is quantctl.cmd_alpha_signal_oos_fdr_validation_run
    assert parser.parse_args(["alpha-signal-strict-falsification-run"]).func is quantctl.cmd_alpha_signal_strict_falsification_run
    parsed = parser.parse_args(["multi-asset-alpha-generator-strict-prefilter-run"])
    assert parsed.func is quantctl.cmd_multi_asset_alpha_generator_strict_prefilter_run
    assert parsed.source_v4_artifact.endswith("alpha-signal-strict-falsification-protocol-v4")
    assert parsed.symbols == ",".join(DEFAULT_SYMBOL_UNIVERSE)
    assert default_multi_asset_alpha_prefilter_output_dir(today="20260627").name.endswith(
        "multi-asset-alpha-generator-strict-prefilter-v5"
    )
