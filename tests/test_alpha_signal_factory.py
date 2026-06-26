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
from parangsae.research.alpha_signal_factory import (
    ALPHA_SIGNAL_FACTORY_DECISION,
    ALPHA_SIGNAL_FACTORY_NAMESPACE,
    CLAIM_BOUNDARY,
    READINESS_BASELINE_VERDICT,
    SignalSpec,
    apply_unary_operator,
    duplicate_correlation_clusters,
    evaluate_alpha_signal_factory,
    generate_signal_specs,
    inverse_signal_position,
    lag_signal_for_labels,
    lagged_signal_position,
    rolling_zscore,
    deterministic_random_sign_signal,
    write_alpha_signal_factory_artifacts,
)


def timestamp_at(index: pd.Index, position: int) -> pd.Timestamp:
    return cast(pd.Timestamp, pd.Timestamp(cast(Any, index[position])))


def synthetic_frame(rows: int = 180) -> pd.DataFrame:
    index = pd.date_range("2024-01-01", periods=rows, freq="1h", tz="UTC")
    trend = pd.Series(range(rows), index=index, dtype=float) * 0.05
    cycle = pd.Series([((i % 24) - 12) * 0.08 for i in range(rows)], index=index)
    close = 100.0 + trend + cycle
    return pd.DataFrame(
        {
            "Open": close.shift(1).fillna(close.iloc[0]),
            "High": close + 1.0,
            "Low": close - 1.0,
            "Close": close,
            "Volume": 1000.0 + (pd.Series(range(rows), index=index) % 19) * 17.0,
        },
        index=index,
    )


def market_bundle(frame: pd.DataFrame) -> MarketDataBundle:
    return MarketDataBundle(
        symbol="BTCUSDT",
        interval="1h",
        months=("2024-01",),
        frame=frame,
        downloads=(),
        normalized_sha256="ohlcv-sha",
        quality={
            "closed_bar_only": True,
            "first_timestamp": timestamp_at(frame.index, 0).isoformat().replace("+00:00", "Z"),
            "last_timestamp": timestamp_at(frame.index, -1).isoformat().replace("+00:00", "Z"),
        },
    )


def funding_bundle(frame: pd.DataFrame) -> FundingRateBundle:
    funding_index = pd.DatetimeIndex(frame.index)[::8]
    funding = pd.DataFrame(
        {
            "symbol": ["BTCUSDT" for _ in funding_index],
            "funding_time_ms": [int(timestamp_at(funding_index, offset).timestamp() * 1000) for offset in range(len(funding_index))],
            "funding_rate": [0.0001 if offset % 2 == 0 else -0.00005 for offset in range(len(funding_index))],
            "mark_price": [100.0 for _ in funding_index],
        },
        index=funding_index,
    )
    return FundingRateBundle(
        symbol="BTCUSDT",
        frame=funding,
        downloads=(),
        normalized_sha256="funding-sha",
        quality={"rows": len(funding), "symbol": "BTCUSDT"},
    )


def cli_args(tmp_path: Path) -> Namespace:
    return Namespace(
        symbol="BTCUSDT",
        interval="1h",
        months="2024-01",
        cache_dir="unused-cache",
        funding_source="public-binance",
        funding_cache_dir="unused-funding-cache",
        reports_root=str(tmp_path / "reports"),
        output_dir=None,
        overwrite=False,
        seed=1234,
        max_depth=3,
        command=None,
        changed_file=None,
    )


def test_signal_spec_schema_serializes_and_forbids_future_data() -> None:
    spec = SignalSpec(
        signal_id="x",
        signal_family="test",
        base_inputs=("Close",),
        operators=({"op": "rolling_mean", "window": 3},),
        lookback_bars=3,
        lag_bars=1,
        max_depth=1,
        uses_funding=False,
        uses_volume=False,
        uses_future_data=False,
        deterministic_seed=7,
    )
    payload = spec.to_dict()
    assert json.loads(json.dumps(payload))["signal_id"] == "x"
    assert SignalSpec.from_dict(payload) == spec
    with pytest.raises(ValueError, match="future data"):
        SignalSpec.from_dict({**payload, "uses_future_data": True})


def test_deterministic_signal_generation_and_max_depth() -> None:
    first = generate_signal_specs(seed=99, max_depth=3)
    second = generate_signal_specs(seed=99, max_depth=3)
    assert [spec.to_dict() for spec in first] == [spec.to_dict() for spec in second]
    assert max(len(spec.operators) for spec in first) <= 3
    assert all(spec.lag_bars >= 1 for spec in first)
    with pytest.raises(ValueError, match="max_depth"):
        generate_signal_specs(seed=99, max_depth=4)


def test_rolling_zscore_is_historical_and_full_sample_normalization_forbidden() -> None:
    data = pd.Series([1.0, 2.0, 3.0, 100.0, 101.0, 102.0])
    shocked = data.copy()
    shocked.iloc[-1] = 10000.0
    expected_prefix = rolling_zscore(data, window=3).iloc[:4]
    shocked_prefix = rolling_zscore(shocked, window=3).iloc[:4]
    pd.testing.assert_series_equal(expected_prefix, shocked_prefix)
    with pytest.raises(ValueError, match="full-sample normalization"):
        apply_unary_operator(data, "zscore", lookback_bars=0)


def test_signal_lag_random_lagged_and_inverse_baselines_are_causal_and_deterministic() -> None:
    signal = pd.Series([1.0, -2.0, 3.0, -4.0])
    lagged_for_label = lag_signal_for_labels(signal, lag_bars=1)
    assert pd.isna(lagged_for_label.iloc[0])
    assert lagged_for_label.iloc[1] == signal.iloc[0]
    random_a, seed_a = deterministic_random_sign_signal(signal, "abc", seed_base=10)
    random_b, seed_b = deterministic_random_sign_signal(signal, "abc", seed_base=10)
    assert seed_a == seed_b
    pd.testing.assert_series_equal(random_a, random_b)
    causal_lag = lagged_signal_position(signal, lag_bars=2)
    assert causal_lag.tolist() == [0.0, 0.0, 1.0, -2.0]
    pd.testing.assert_series_equal(inverse_signal_position(signal), -signal)


def test_duplicate_clustering_and_effective_breadth_exclude_duplicates() -> None:
    index = pd.RangeIndex(5)
    signals = {
        "a": pd.Series([1, 2, 3, 4, 5], index=index, dtype=float),
        "b": pd.Series([2, 4, 6, 8, 10], index=index, dtype=float),
        "c": pd.Series([5, 4, 3, 2, 1], index=index, dtype=float),
    }
    clusters = duplicate_correlation_clusters(signals, threshold=0.95)
    assert sum(1 for row in clusters if row["is_redundant_duplicate"]) >= 1
    effective_breadth = len({row["duplicate_cluster_id"] for row in clusters if not row["is_redundant_duplicate"]})
    assert effective_breadth < len(signals)


def test_factory_payload_preserves_claim_boundaries_and_artifact_schema(tmp_path: Path) -> None:
    frame = synthetic_frame()
    result = evaluate_alpha_signal_factory(
        market_bundle=market_bundle(frame),
        funding_bundle=funding_bundle(frame),
        seed=123,
        max_depth=3,
        commands=["uv run quantctl alpha-signal-factory-composition-run"],
        changed_files=["src/parangsae/research/alpha_signal_factory.py"],
    )
    payload = result.payload
    assert payload["suite_decision"] == ALPHA_SIGNAL_FACTORY_DECISION
    assert payload["loop_namespace"] == ALPHA_SIGNAL_FACTORY_NAMESPACE
    assert payload["readiness_baseline_verdict"] == READINESS_BASELINE_VERDICT
    assert payload["tsmom_state_changed"] is False
    assert payload["holdout_retune_used"] is False
    assert payload["safety_gates"]["no_live_orders_submitted"] is True
    assert payload["safety_gates"]["no_testnet_orders_submitted"] is True
    assert payload["safety_gates"]["no_account_data_used"] is True
    assert payload["safety_gates"]["no_api_key_used"] is True
    assert payload["generated_signal_count"] == len(result.catalog)
    assert payload["effective_signal_breadth"] <= payload["evaluated_signal_count"]
    assert {"KILL_SIGNAL", "DUPLICATE_SIGNAL", "WATCH_DIAGNOSTIC_ONLY"}.intersection(
        {row["shortlist_category"] for row in result.shortlist}
    )
    assert all(
        row["baseline_uses_current_bar_signal"] is False
        for row in result.null_comparison
        if row["baseline_name"] in {"lagged_signal", "inverse_signal", "random_sign_signal", "shuffled_timestamp_signal"}
    )
    out = write_alpha_signal_factory_artifacts(tmp_path / "alpha", result)
    assert {path.name for path in out.iterdir()} == set(payload["evidence_artifacts"])
    saved = json.loads((out / "alpha_signal_factory.json").read_text())
    assert saved["claim_boundary"] == CLAIM_BOUNDARY
    assert saved["readiness_baseline_verdict"] == "NOT_READY"
    assert len((out / "signal_catalog.jsonl").read_text().splitlines()) == payload["generated_signal_count"]


def test_cli_alpha_signal_factory_uses_public_no_credential_no_live_path(monkeypatch, tmp_path: Path) -> None:
    frame = synthetic_frame()
    args = cli_args(tmp_path)
    monkeypatch.setattr(quantctl, "load_market_data_bundle", lambda **_: market_bundle(frame))
    monkeypatch.setattr(quantctl, "fetch_funding_rate_history", lambda **_: funding_bundle(frame))
    monkeypatch.setenv("BINANCE_API_KEY", "SHOULD_NOT_BE_READ")
    monkeypatch.setenv("BINANCE_API_SECRET", "SHOULD_NOT_BE_READ")
    real_getenv = os.getenv

    def guarded_getenv(key, default=None):
        if key in {"BINANCE_API_KEY", "BINANCE_API_SECRET"}:
            raise AssertionError(f"alpha signal factory must not read {key}")
        return real_getenv(key, default)

    monkeypatch.setattr(os, "getenv", guarded_getenv)
    assert quantctl.cmd_alpha_signal_factory_composition_run(args) == 0
    report_dirs = list((tmp_path / "reports" / "loops").iterdir())
    assert len(report_dirs) == 1
    payload = json.loads((report_dirs[0] / "alpha_signal_factory.json").read_text())
    assert payload["suite_decision"] == "SIGNAL_FACTORY_INFRA_ONLY"
    assert payload["readiness_baseline_verdict"] == "NOT_READY"
    assert payload["tsmom_state_changed"] is False
    assert payload["safety_gates"]["no_private_order_or_fill_data_used"] is True
