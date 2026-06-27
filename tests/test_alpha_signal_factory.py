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
from parangsae.research import alpha_signal_factory as alpha_signal_factory_module
from parangsae.research.alpha_signal_factory import (
    ALPHA_SIGNAL_FACTORY_DECISION,
    ALPHA_SIGNAL_DAG_DECISION,
    ALPHA_SIGNAL_DAG_NAMESPACE,
    ALPHA_SIGNAL_FACTORY_NAMESPACE,
    ALPHA_SIGNAL_OOS_FDR_DECISION,
    ALPHA_SIGNAL_OOS_FDR_NAMESPACE,
    ALPHA_SIGNAL_STRICT_FALSIFICATION_DECISION,
    ALPHA_SIGNAL_STRICT_FALSIFICATION_NAMESPACE,
    CLAIM_BOUNDARY,
    DEFAULT_SIGNAL_DAG_CANDIDATE_BUDGET,
    DEFAULT_SIGNAL_DAG_MAX_DEPTH,
    OOS_FDR_CLAIM_BOUNDARY,
    OOS_FORBIDDEN_OUTPUT_LABELS,
    READINESS_BASELINE_VERDICT,
    SIGNAL_DAG_CLAIM_BOUNDARY,
    STRICT_FALSIFICATION_CLAIM_BOUNDARY,
    STRICT_FORBIDDEN_OUTPUT_LABELS,
    STRICT_PURGE_EMBARGO_MULTIPLIER,
    SignalSpec,
    SignalDagSpec,
    apply_unary_operator,
    apply_binary_operator,
    apply_gate_operator,
    build_signal_dag_series,
    duplicate_correlation_clusters,
    duplicate_correlation_clusters_for_dags,
    evaluate_alpha_signal_factory,
    evaluate_alpha_signal_dag_expansion,
    evaluate_alpha_signal_oos_fdr_validation,
    evaluate_alpha_signal_strict_falsification,
    generate_signal_specs,
    generate_signal_dag_specs,
    inverse_signal_position,
    lag_signal_for_labels,
    lagged_signal_position,
    rolling_zscore,
    build_oos_chronological_folds,
    deterministic_random_sign_signal,
    shuffled_timestamp_signal,
    write_alpha_signal_factory_artifacts,
    write_alpha_signal_dag_expansion_artifacts,
    write_alpha_signal_oos_fdr_validation_artifacts,
    write_alpha_signal_strict_falsification_artifacts,
    _benjamini_hochberg,
    _moving_block_permutation_pvalue,
    _strict_falsification_decision,
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


def write_v2_fixture_artifact(tmp_path: Path, frame: pd.DataFrame, *, candidate_budget: int = 30) -> Path:
    result = evaluate_alpha_signal_dag_expansion(
        market_bundle=market_bundle(frame),
        funding_bundle=funding_bundle(frame),
        seed=2718,
        max_depth=4,
        candidate_budget=candidate_budget,
        commands=["uv run quantctl alpha-signal-dag-expansion-run"],
        changed_files=["src/parangsae/research/alpha_signal_factory.py"],
    )
    return write_alpha_signal_dag_expansion_artifacts(tmp_path / "v2-source", result)


def write_v3_fixture_artifact(
    tmp_path: Path,
    frame: pd.DataFrame,
    *,
    candidate_budget: int = 32,
    horizons: tuple[int, ...] = (2, 4),
    fold_count: int = 2,
    force_queue: bool = True,
) -> Path:
    v2_artifact = write_v2_fixture_artifact(tmp_path, frame, candidate_budget=candidate_budget)
    v3_result = evaluate_alpha_signal_oos_fdr_validation(
        source_v2_artifact_path=v2_artifact,
        market_bundle=market_bundle(frame),
        funding_bundle=funding_bundle(frame),
        horizons=horizons,
        fold_count=fold_count,
        commands=["uv run quantctl alpha-signal-oos-fdr-validation-run"],
        changed_files=["src/parangsae/research/alpha_signal_factory.py"],
    )
    out = write_alpha_signal_oos_fdr_validation_artifacts(tmp_path / "v3-source", v3_result)
    if force_queue and not v3_result.next_falsification_queue:
        seed_family = v3_result.family_oos_summary[0]
        forced_ic = seed_family["median_test_ic"]
        pd.DataFrame(
            [
                {
                    "signal_family": seed_family["signal_family"],
                    "horizon_bars": seed_family["horizon_bars"],
                    "queue_decision": "NEEDS_STRICTER_OOS_FALSIFICATION",
                    "median_test_ic": forced_ic if forced_ic is not None else 0.01,
                    "effective_signal_breadth": seed_family["effective_signal_breadth"],
                    "reason": "forced non-empty queue for strict falsification fixture",
                    "claim_boundary": OOS_FDR_CLAIM_BOUNDARY,
                }
            ]
        ).to_csv(out / "next_falsification_queue.csv", index=False)
    return out


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


def test_signal_dag_spec_json_serialization_and_future_data_rejection() -> None:
    payload = {
        "signal_id": "dag.test",
        "dag_id": "dag00001",
        "signal_family": "test",
        "root_operator": "zscore",
        "nodes": (
            {"node_id": "base_0", "type": "base", "input": "returns"},
            {"node_id": "op_1", "type": "unary", "op": "zscore", "window": 8},
        ),
        "edges": ({"source": "base_0", "target": "op_1"},),
        "base_inputs": ("returns",),
        "operator_sequence": ("zscore",),
        "depth": 1,
        "max_depth": 4,
        "lookback_bars": 8,
        "lag_bars": 1,
        "uses_price": True,
        "uses_volume": False,
        "uses_funding": False,
        "uses_regime_gate": False,
        "uses_future_data": False,
        "generation_seed": 7,
        "generation_rule_id": "unit",
        "complexity_score": 1.25,
    }
    spec = SignalDagSpec.from_dict(payload)
    assert json.loads(json.dumps(spec.to_dict()))["dag_id"] == "dag00001"
    assert spec.claim_boundary == SIGNAL_DAG_CLAIM_BOUNDARY
    with pytest.raises(ValueError, match="future data"):
        SignalDagSpec.from_dict({**payload, "uses_future_data": True})


def test_signal_dag_generation_is_deterministic_budgeted_and_depth_bounded() -> None:
    first = generate_signal_dag_specs(seed=42, max_depth=4, candidate_budget=75)
    second = generate_signal_dag_specs(seed=42, max_depth=4, candidate_budget=75)
    assert [spec.to_dict() for spec in first] == [spec.to_dict() for spec in second]
    assert len(first) == 75
    assert max(spec.depth for spec in first) <= DEFAULT_SIGNAL_DAG_MAX_DEPTH
    assert all(spec.lag_bars >= 1 for spec in first)
    assert all("forward" not in " ".join(spec.base_inputs + spec.operator_sequence) for spec in first)
    with pytest.raises(ValueError, match="candidate_budget"):
        generate_signal_dag_specs(candidate_budget=10001)
    with pytest.raises(ValueError, match="max_depth"):
        generate_signal_dag_specs(max_depth=5)
    assert DEFAULT_SIGNAL_DAG_CANDIDATE_BUDGET == 2000


def test_signal_dag_operator_grammar_causality_and_safe_math() -> None:
    frame = synthetic_frame(rows=120)
    funding = cast(
        pd.Series,
        funding_bundle(frame).frame["funding_rate"].reindex(frame.index, method="ffill").fillna(0.0).shift(1).fillna(0.0),
    )
    spec = next(
        item
        for item in generate_signal_dag_specs(seed=11, max_depth=4, candidate_budget=1200)
        if "template_" in item.generation_rule_id and item.uses_regime_gate
    )
    signal = build_signal_dag_series(spec, frame=frame, funding=funding)
    assert len(signal) == len(frame)
    assert spec.depth <= 4
    with pytest.raises(ValueError, match="full-sample normalization"):
        apply_unary_operator(pd.Series([1.0, 2.0, 3.0]), "zscore", lookback_bars=0)
    left = pd.Series([1.0, 2.0, 3.0])
    right = pd.Series([0.0, 1.0, 0.0])
    ratio = apply_binary_operator(left, right, "ratio_safe", lookback_bars=2)
    assert not ratio.replace([float("inf"), float("-inf")], pd.NA).dropna().isin([float("inf"), float("-inf")]).any()
    gated = apply_gate_operator(pd.Series(1.0, index=frame.index), "gate_low_volatility", frame=frame, funding=funding, lookback_bars=24)
    shocked = frame.copy()
    shocked.iloc[-1, shocked.columns.get_loc("Close")] = 100000.0
    gated_prefix = apply_gate_operator(pd.Series(1.0, index=shocked.index), "gate_low_volatility", frame=shocked, funding=funding, lookback_bars=24).iloc[:50]
    pd.testing.assert_series_equal(gated.iloc[:50], gated_prefix)


def test_signal_dag_duplicate_representative_uses_non_performance_rule() -> None:
    index = pd.RangeIndex(6)
    signals = {
        "z_complex": pd.Series([1, 2, 3, 4, 5, 6], index=index, dtype=float),
        "a_simple": pd.Series([2, 4, 6, 8, 10, 12], index=index, dtype=float),
        "m_medium": pd.Series([6, 5, 4, 3, 2, 1], index=index, dtype=float),
    }
    specs = [
        SignalDagSpec(
            signal_id="z_complex",
            dag_id="dagz",
            signal_family="test",
            root_operator="zscore",
            nodes=({"node_id": "base_0", "type": "base", "input": "returns"}, {"node_id": "op_1", "type": "unary", "op": "zscore", "window": 2}),
            edges=({"source": "base_0", "target": "op_1"},),
            base_inputs=("returns",),
            operator_sequence=("zscore",),
            depth=1,
            max_depth=4,
            lookback_bars=2,
            lag_bars=1,
            uses_price=True,
            uses_volume=False,
            uses_funding=False,
            uses_regime_gate=False,
            uses_future_data=False,
            generation_seed=1,
            generation_rule_id="unit",
            complexity_score=3.0,
        ),
        SignalDagSpec(
            signal_id="a_simple",
            dag_id="daga",
            signal_family="test",
            root_operator="zscore",
            nodes=({"node_id": "base_0", "type": "base", "input": "returns"}, {"node_id": "op_1", "type": "unary", "op": "zscore", "window": 2}),
            edges=({"source": "base_0", "target": "op_1"},),
            base_inputs=("returns",),
            operator_sequence=("zscore",),
            depth=1,
            max_depth=4,
            lookback_bars=2,
            lag_bars=1,
            uses_price=True,
            uses_volume=False,
            uses_funding=False,
            uses_regime_gate=False,
            uses_future_data=False,
            generation_seed=2,
            generation_rule_id="unit",
            complexity_score=1.0,
        ),
        SignalDagSpec(
            signal_id="m_medium",
            dag_id="dagm",
            signal_family="test",
            root_operator="zscore",
            nodes=({"node_id": "base_0", "type": "base", "input": "returns"}, {"node_id": "op_1", "type": "unary", "op": "zscore", "window": 2}),
            edges=({"source": "base_0", "target": "op_1"},),
            base_inputs=("returns",),
            operator_sequence=("zscore",),
            depth=1,
            max_depth=4,
            lookback_bars=2,
            lag_bars=1,
            uses_price=True,
            uses_volume=False,
            uses_funding=False,
            uses_regime_gate=False,
            uses_future_data=False,
            generation_seed=3,
            generation_rule_id="unit",
            complexity_score=2.0,
        ),
    ]
    clusters = duplicate_correlation_clusters_for_dags(signals, specs, threshold=0.95)
    duplicate_cluster = [row for row in clusters if row["duplicate_cluster_id"] == clusters[0]["duplicate_cluster_id"]]
    assert any(row["representative_signal_id"] == "a_simple" for row in duplicate_cluster)
    assert sum(1 for row in clusters if not row["is_redundant_duplicate"]) < len(signals)


def test_signal_dag_baselines_are_deterministic_and_no_lookahead() -> None:
    signal = pd.Series([1.0, -2.0, 3.0, -4.0, 5.0])
    random_a, seed_a = deterministic_random_sign_signal(signal, "dag", seed_base=99)
    random_b, seed_b = deterministic_random_sign_signal(signal, "dag", seed_base=99)
    shuffled_a, shuffle_seed_a = shuffled_timestamp_signal(signal, "dag", seed_base=99)
    shuffled_b, shuffle_seed_b = shuffled_timestamp_signal(signal, "dag", seed_base=99)
    assert seed_a == seed_b
    assert shuffle_seed_a == shuffle_seed_b
    pd.testing.assert_series_equal(random_a, random_b)
    pd.testing.assert_series_equal(shuffled_a, shuffled_b)
    assert lagged_signal_position(signal, lag_bars=2).tolist() == [0.0, 0.0, 1.0, -2.0, 3.0]
    pd.testing.assert_series_equal(inverse_signal_position(signal), -signal)
    assert pd.isna(lag_signal_for_labels(signal, lag_bars=1).iloc[0])


def test_signal_dag_expansion_payload_artifacts_and_multiple_testing_warning(tmp_path: Path) -> None:
    frame = synthetic_frame(rows=240)
    result = evaluate_alpha_signal_dag_expansion(
        market_bundle=market_bundle(frame),
        funding_bundle=funding_bundle(frame),
        seed=321,
        max_depth=4,
        candidate_budget=160,
        commands=["uv run quantctl alpha-signal-dag-expansion-run"],
        changed_files=["src/parangsae/research/alpha_signal_factory.py"],
    )
    payload = result.payload
    assert payload["suite_decision"] == ALPHA_SIGNAL_DAG_DECISION
    assert payload["loop_namespace"] == ALPHA_SIGNAL_DAG_NAMESPACE
    assert payload["readiness_baseline_verdict"] == READINESS_BASELINE_VERDICT
    assert payload["tsmom_state_changed"] is False
    assert payload["holdout_retune_used"] is False
    assert payload["generated_signal_count"] <= payload["candidate_budget"]
    assert payload["effective_signal_breadth"] <= payload["evaluated_signal_count"]
    assert payload["summary"]["cannot_claim_edge_due_to_multiple_testing"] is True
    assert any(row["cannot_claim_edge_due_to_multiple_testing"] for row in result.multiple_testing_summary)
    assert all(row["baseline_uses_current_bar_signal"] is False for row in result.null_comparison)
    out = write_alpha_signal_dag_expansion_artifacts(tmp_path / "dag", result)
    assert {path.name for path in out.iterdir()} == set(payload["evidence_artifacts"])
    saved = json.loads((out / "alpha_signal_dag_expansion.json").read_text())
    assert saved["claim_boundary"] == SIGNAL_DAG_CLAIM_BOUNDARY
    assert saved["readiness_baseline_verdict"] == "NOT_READY"
    assert (out / "multiple_testing_summary.csv").read_text().count("cannot_claim_edge_due_to_multiple_testing") >= 1


def test_cli_signal_dag_expansion_uses_public_no_credential_no_live_path(monkeypatch, tmp_path: Path) -> None:
    frame = synthetic_frame(rows=220)
    args = Namespace(
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
        max_depth=4,
        candidate_budget=120,
        command=None,
        changed_file=None,
    )
    monkeypatch.setattr(quantctl, "load_market_data_bundle", lambda **_: market_bundle(frame))
    monkeypatch.setattr(quantctl, "fetch_funding_rate_history", lambda **_: funding_bundle(frame))
    monkeypatch.setenv("BINANCE_API_KEY", "SHOULD_NOT_BE_READ")
    monkeypatch.setenv("BINANCE_API_SECRET", "SHOULD_NOT_BE_READ")
    real_getenv = os.getenv

    def guarded_getenv(key, default=None):
        if key in {"BINANCE_API_KEY", "BINANCE_API_SECRET"}:
            raise AssertionError(f"alpha signal DAG expansion must not read {key}")
        return real_getenv(key, default)

    monkeypatch.setattr(os, "getenv", guarded_getenv)
    assert quantctl.cmd_alpha_signal_dag_expansion_run(args) == 0
    report_dirs = list((tmp_path / "reports" / "loops").iterdir())
    assert len(report_dirs) == 1
    payload = json.loads((report_dirs[0] / "alpha_signal_dag_expansion.json").read_text())
    assert payload["suite_decision"] == "SIGNAL_DAG_INFRA_ONLY"
    assert payload["readiness_baseline_verdict"] == "NOT_READY"
    assert payload["tsmom_state_changed"] is False
    assert payload["safety_gates"]["no_private_order_or_fill_data_used"] is True


def test_v1_alpha_signal_factory_command_still_works(monkeypatch, tmp_path: Path) -> None:
    frame = synthetic_frame(rows=180)
    args = cli_args(tmp_path)
    monkeypatch.setattr(quantctl, "load_market_data_bundle", lambda **_: market_bundle(frame))
    monkeypatch.setattr(quantctl, "fetch_funding_rate_history", lambda **_: funding_bundle(frame))
    assert quantctl.cmd_alpha_signal_factory_composition_run(args) == 0
    report_dirs = list((tmp_path / "reports" / "loops").iterdir())
    assert len(report_dirs) == 1
    payload = json.loads((report_dirs[0] / "alpha_signal_factory.json").read_text())
    assert payload["suite_decision"] == "SIGNAL_FACTORY_INFRA_ONLY"
    assert payload["readiness_baseline_verdict"] == "NOT_READY"


def test_oos_fdr_chronological_folds_are_ordered_purged_and_embargoed() -> None:
    index = pd.date_range("2024-01-01", periods=220, freq="1h", tz="UTC")
    folds = build_oos_chronological_folds(index, horizons=(2, 4), fold_count=2)
    assert folds
    for fold in folds:
        assert fold["train_start_pos"] < fold["train_end_pos"] < fold["validation_start_pos"]
        assert fold["validation_start_pos"] < fold["validation_end_pos"] < fold["test_start_pos"]
        assert fold["test_start_pos"] < fold["test_end_pos"]
        assert fold["validation_start_pos"] - fold["train_end_pos"] >= fold["purge_bars"] + fold["embargo_bars"]
        assert fold["test_start_pos"] - fold["validation_end_pos"] >= fold["purge_bars"] + fold["embargo_bars"]
        assert fold["purged_observation_count"] > 0
        assert fold["label_overlap_allowed"] is False


def test_oos_fdr_freezes_v2_candidate_universe_and_preserves_signal_ids(monkeypatch, tmp_path: Path) -> None:
    frame = synthetic_frame(rows=220)
    v2_artifact = write_v2_fixture_artifact(tmp_path, frame, candidate_budget=28)

    def forbidden_generation(*_args, **_kwargs):
        raise AssertionError("V3 must not generate new Signal DAG candidates")

    monkeypatch.setattr(alpha_signal_factory_module, "generate_signal_dag_specs", forbidden_generation)
    result = evaluate_alpha_signal_oos_fdr_validation(
        source_v2_artifact_path=v2_artifact,
        market_bundle=market_bundle(frame),
        funding_bundle=funding_bundle(frame),
        horizons=(2, 4),
        fold_count=2,
        commands=["uv run quantctl alpha-signal-oos-fdr-validation-run"],
        changed_files=["src/parangsae/research/alpha_signal_factory.py"],
    )
    source_signal_ids = [json.loads(line)["signal_id"] for line in (v2_artifact / "signal_dag_catalog.jsonl").read_text().splitlines()]
    frozen_signal_ids = [row["signal_id"] for row in result.frozen_candidate_universe]
    assert frozen_signal_ids == source_signal_ids
    assert result.payload["suite_decision"] == ALPHA_SIGNAL_OOS_FDR_DECISION
    assert result.payload["loop_namespace"] == ALPHA_SIGNAL_OOS_FDR_NAMESPACE
    assert result.payload["readiness_baseline_verdict"] == READINESS_BASELINE_VERDICT
    assert result.payload["candidate_universe_freeze"]["new_signal_dags_generated"] is False
    assert result.payload["candidate_universe_freeze"]["signal_ids_changed"] is False
    assert result.payload["strategy_optimization_performed"] is False
    assert result.payload["tsmom_state_changed"] is False
    assert result.payload["safety_gates"]["no_private_order_or_fill_data_used"] is True
    assert all(row["leakage_check_status"] == "passed_shifted_by_decision_lag" for row in result.signal_oos_diagnostics)


def test_oos_fdr_artifacts_boundaries_baselines_and_nonperformance_representatives(tmp_path: Path) -> None:
    frame = synthetic_frame(rows=220)
    v2_artifact = write_v2_fixture_artifact(tmp_path, frame, candidate_budget=32)
    result = evaluate_alpha_signal_oos_fdr_validation(
        source_v2_artifact_path=v2_artifact,
        market_bundle=market_bundle(frame),
        funding_bundle=funding_bundle(frame),
        horizons=(2, 4),
        fold_count=2,
        commands=["uv run pytest tests/test_alpha_signal_factory.py -q"],
        changed_files=["src/parangsae/research/alpha_signal_factory.py"],
    )
    payload = result.payload
    assert payload["claim_boundary"] == OOS_FDR_CLAIM_BOUNDARY
    assert payload["summary"]["cannot_claim_edge_due_to_multiple_testing"] is True
    assert payload["suite_decision"] not in OOS_FORBIDDEN_OUTPUT_LABELS
    assert all(row["family_decision"] not in OOS_FORBIDDEN_OUTPUT_LABELS for row in result.family_oos_summary)
    assert any(row["cannot_claim_edge_due_to_multiple_testing"] for row in result.multiple_testing_fdr_summary)
    assert all(row["baseline_uses_current_bar_signal"] is False for row in result.oos_null_baseline_comparison)
    assert {"deterministic_random_sign", "lagged_signal", "inverse_signal", "shuffled_timestamp_signal", "family_permuted_signal"}.issubset(
        {row["baseline_name"] for row in result.oos_null_baseline_comparison}
    )
    assert all(
        row["selection_rule"] == "v2_cluster_representative_lowest_complexity_then_depth_then_lexicographic_signal_id"
        for row in result.representative_signal_summary
    )
    assert all(
        "corresponding_test_ic_for_best_validation_signal" in row and "best_validation_signal_id" in row
        for row in result.family_oos_summary
    )
    out = write_alpha_signal_oos_fdr_validation_artifacts(tmp_path / "v3", result)
    assert {path.name for path in out.iterdir()} == set(payload["evidence_artifacts"])
    saved = json.loads((out / "alpha_signal_oos_fdr_validation.json").read_text())
    assert saved["readiness_baseline_verdict"] == "NOT_READY"
    assert saved["candidate_universe_freeze"]["new_signal_dags_generated"] is False
    assert len((out / "frozen_candidate_universe.jsonl").read_text().splitlines()) == payload["frozen_candidate_count"]


def test_cli_oos_fdr_validation_uses_public_no_credential_no_live_path(monkeypatch, tmp_path: Path) -> None:
    source_frame = synthetic_frame(rows=220)
    v2_artifact = write_v2_fixture_artifact(tmp_path, source_frame, candidate_budget=24)
    frame = synthetic_frame(rows=1300)
    args = Namespace(
        source_v2_artifact=str(v2_artifact),
        symbol="BTCUSDT",
        interval="1h",
        months="2024-01,2024-02",
        cache_dir="unused-cache",
        funding_source="public-binance",
        funding_cache_dir="unused-funding-cache",
        reports_root=str(tmp_path / "reports"),
        output_dir=None,
        overwrite=False,
        command=None,
        changed_file=None,
    )
    monkeypatch.setattr(quantctl, "load_market_data_bundle", lambda **_: market_bundle(frame))
    monkeypatch.setattr(quantctl, "fetch_funding_rate_history", lambda **_: funding_bundle(frame))
    monkeypatch.setenv("BINANCE_API_KEY", "SHOULD_NOT_BE_READ")
    monkeypatch.setenv("BINANCE_API_SECRET", "SHOULD_NOT_BE_READ")
    real_getenv = os.getenv

    def guarded_getenv(key, default=None):
        if key in {"BINANCE_API_KEY", "BINANCE_API_SECRET"}:
            raise AssertionError(f"alpha signal OOS/FDR validation must not read {key}")
        return real_getenv(key, default)

    monkeypatch.setattr(os, "getenv", guarded_getenv)
    assert quantctl.cmd_alpha_signal_oos_fdr_validation_run(args) == 0
    report_dirs = list((tmp_path / "reports" / "loops").iterdir())
    assert len(report_dirs) == 1
    payload = json.loads((report_dirs[0] / "alpha_signal_oos_fdr_validation.json").read_text())
    assert payload["suite_decision"] == "OOS_FDR_PROTOCOL_INFRA_ONLY"
    assert payload["readiness_baseline_verdict"] == "NOT_READY"
    assert payload["tsmom_state_changed"] is False
    assert payload["safety_gates"]["no_api_key_used"] is True
    assert payload["live_or_testnet_path_created"] is False


def test_v1_v2_v3_alpha_commands_are_wired() -> None:
    parser = quantctl.build_parser()
    assert parser.parse_args(["alpha-signal-factory-composition-run"]).func is quantctl.cmd_alpha_signal_factory_composition_run
    assert parser.parse_args(["alpha-signal-dag-expansion-run"]).func is quantctl.cmd_alpha_signal_dag_expansion_run
    assert parser.parse_args(["alpha-signal-oos-fdr-validation-run"]).func is quantctl.cmd_alpha_signal_oos_fdr_validation_run


def test_v4_strict_falsification_command_is_wired() -> None:
    parser = quantctl.build_parser()
    parsed = parser.parse_args(["alpha-signal-strict-falsification-run"])
    assert parsed.func is quantctl.cmd_alpha_signal_strict_falsification_run
    assert parsed.source_v3_artifact.endswith("alpha-signal-oos-fdr-validation-protocol-v3")


def test_moving_block_permutation_pvalue_separates_signal_from_null() -> None:
    rng = pd.Series(range(600), dtype=float)
    base = (rng % 17 - 8.0).to_numpy(dtype=float)
    dependent_label = 0.35 * base + ((rng % 5 - 2.0).to_numpy(dtype=float))
    dependent = _moving_block_permutation_pvalue(
        base, dependent_label, block_length=8, n_resamples=400, expected_sign=1, seed=4242
    )
    assert dependent["observed_ic"] is not None and dependent["observed_ic"] > 0
    assert dependent["pvalue"] is not None and dependent["pvalue"] < 0.05
    assert dependent["block_length"] == 8
    assert dependent["test_direction"] == "greater"

    unrelated_label = ((rng * 7 + 3) % 13 - 6.0).to_numpy(dtype=float)
    unrelated = _moving_block_permutation_pvalue(
        base, unrelated_label, block_length=8, n_resamples=400, expected_sign=1, seed=4242
    )
    assert unrelated["pvalue"] is not None and unrelated["pvalue"] > dependent["pvalue"]

    short = _moving_block_permutation_pvalue(
        base[:10], dependent_label[:10], block_length=8, n_resamples=400, expected_sign=1, seed=1
    )
    assert short["pvalue"] is None and short["observed_ic"] is None


def test_moving_block_permutation_pvalue_is_deterministic() -> None:
    series = pd.Series(range(300), dtype=float)
    signal = (series % 11 - 5.0).to_numpy(dtype=float)
    label = 0.2 * signal + ((series % 7 - 3.0).to_numpy(dtype=float))
    first = _moving_block_permutation_pvalue(signal, label, block_length=4, n_resamples=300, expected_sign=1, seed=99)
    second = _moving_block_permutation_pvalue(signal, label, block_length=4, n_resamples=300, expected_sign=1, seed=99)
    assert first == second


def test_benjamini_hochberg_controls_false_discovery() -> None:
    result = _benjamini_hochberg([0.001, 0.04, 0.2, None, 0.5], alpha=0.10, m=5)
    assert result["flags"] == [True, True, False, False, False]
    assert result["cutoff_pvalue"] == 0.04
    assert result["m"] == 5
    none_result = _benjamini_hochberg([None, None], alpha=0.10, m=2)
    assert none_result["flags"] == [False, False]
    assert none_result["cutoff_pvalue"] is None


def test_strict_falsification_decision_defaults_to_falsified() -> None:
    survived = _strict_falsification_decision(
        expected_sign=1,
        observed_test_sign=1,
        bootstrap_pvalue=0.001,
        bh_significant=True,
        sign_consistency_rate=0.75,
        fold_consistency_rate=0.7,
        null_robustness_rate=0.7,
        degradation_ratio=1.0,
    )
    assert survived == "SURVIVED_STRICT_FALSIFICATION"
    assert (
        _strict_falsification_decision(
            expected_sign=1, observed_test_sign=None, bootstrap_pvalue=None, bh_significant=False,
            sign_consistency_rate=None, fold_consistency_rate=None, null_robustness_rate=0.0, degradation_ratio=None,
        )
        == "FALSIFIED_INSUFFICIENT_EVIDENCE"
    )
    assert (
        _strict_falsification_decision(
            expected_sign=1, observed_test_sign=-1, bootstrap_pvalue=0.01, bh_significant=True,
            sign_consistency_rate=0.9, fold_consistency_rate=0.9, null_robustness_rate=0.9, degradation_ratio=1.0,
        )
        == "FALSIFIED_SIGN_FLIP"
    )
    assert (
        _strict_falsification_decision(
            expected_sign=1, observed_test_sign=1, bootstrap_pvalue=0.5, bh_significant=False,
            sign_consistency_rate=0.9, fold_consistency_rate=0.9, null_robustness_rate=0.9, degradation_ratio=1.0,
        )
        == "FALSIFIED_NO_BOOTSTRAP_SIGNIFICANCE"
    )


def test_strict_falsification_preregisters_and_freezes_v3_queue(monkeypatch, tmp_path: Path) -> None:
    frame = synthetic_frame(rows=420)
    v3_artifact = write_v3_fixture_artifact(tmp_path, frame, candidate_budget=30)

    def forbidden_generation(*_args, **_kwargs):
        raise AssertionError("V4 must not generate new Signal DAG candidates")

    monkeypatch.setattr(alpha_signal_factory_module, "generate_signal_dag_specs", forbidden_generation)
    result = evaluate_alpha_signal_strict_falsification(
        source_v3_artifact_path=v3_artifact,
        market_bundle=market_bundle(frame),
        funding_bundle=funding_bundle(frame),
        fold_count=2,
        bootstrap_resamples=200,
        commands=["uv run quantctl alpha-signal-strict-falsification-run"],
        changed_files=["src/parangsae/research/alpha_signal_factory.py"],
    )
    payload = result.payload
    assert payload["suite_decision"] == ALPHA_SIGNAL_STRICT_FALSIFICATION_DECISION
    assert payload["loop_namespace"] == ALPHA_SIGNAL_STRICT_FALSIFICATION_NAMESPACE
    assert payload["readiness_baseline_verdict"] == READINESS_BASELINE_VERDICT
    assert payload["candidate_universe_freeze"]["new_signal_dags_generated"] is False
    assert payload["candidate_universe_freeze"]["signal_ids_changed"] is False
    assert payload["strategy_optimization_performed"] is False
    assert payload["tsmom_state_changed"] is False
    assert payload["preregistered_hypothesis_count"] >= 1

    queue_rows = pd.read_csv(v3_artifact / "next_falsification_queue.csv").to_dict(orient="records")
    queued_families = {str(row["signal_family"]) for row in queue_rows}
    assert {row["signal_family"] for row in result.preregistered_hypotheses} == queued_families
    # frozen universe must be a sign-id-preserving subset of the V3 frozen universe, queued families only
    v3_signal_ids = {json.loads(line)["signal_id"] for line in (v3_artifact / "frozen_candidate_universe.jsonl").read_text().splitlines() if line.strip()}
    v4_signal_ids = {row["signal_id"] for row in result.frozen_candidate_universe}
    assert v4_signal_ids.issubset(v3_signal_ids)
    assert all(str(row["signal_family"]) in queued_families for row in result.frozen_candidate_universe)
    # stricter purge/embargo than V3 (2x max queued horizon)
    queued_horizons = {int(row["horizon_bars"]) for row in queue_rows}
    assert payload["purge_bars"] == STRICT_PURGE_EMBARGO_MULTIPLIER * max(queued_horizons)
    assert payload["embargo_bars"] == payload["purge_bars"]


def test_strict_falsification_defaults_to_falsified_and_writes_bounded_artifacts(tmp_path: Path) -> None:
    frame = synthetic_frame(rows=420)
    v3_artifact = write_v3_fixture_artifact(tmp_path, frame, candidate_budget=34)
    result = evaluate_alpha_signal_strict_falsification(
        source_v3_artifact_path=v3_artifact,
        market_bundle=market_bundle(frame),
        funding_bundle=funding_bundle(frame),
        fold_count=2,
        bootstrap_resamples=200,
        commands=["uv run pytest tests/test_alpha_signal_factory.py -q"],
        changed_files=["src/parangsae/research/alpha_signal_factory.py"],
    )
    payload = result.payload
    valid_decisions = {
        "SURVIVED_STRICT_FALSIFICATION",
        "FALSIFIED_SIGN_FLIP",
        "FALSIFIED_OOS_INCONSISTENT",
        "FALSIFIED_NULL_FRAGILE",
        "FALSIFIED_DEGRADATION",
        "FALSIFIED_NO_BOOTSTRAP_SIGNIFICANCE",
        "FALSIFIED_INSUFFICIENT_EVIDENCE",
    }
    assert payload["claim_boundary"] == STRICT_FALSIFICATION_CLAIM_BOUNDARY
    assert payload["summary"]["default_outcome_is_falsified"] is True
    assert payload["summary"]["survivors_are_not_alpha_confirmation"] is True
    assert payload["suite_decision"] not in STRICT_FORBIDDEN_OUTPUT_LABELS
    assert result.family_strict_falsification_summary, "fixture must yield at least one hypothesis"
    for row in result.family_strict_falsification_summary:
        assert row["strict_decision"] in valid_decisions
        assert row["strict_decision"] not in STRICT_FORBIDDEN_OUTPUT_LABELS
        assert row["decision_is_alpha_confirmation"] is False
    # bootstrap block length must equal the pre-registered horizon (autocorrelation-aware)
    for row in result.bootstrap_significance_summary:
        assert row["block_length"] == int(row["horizon_bars"])
    assert any(row["cannot_claim_edge_due_to_multiple_testing"] for row in result.multiple_testing_fdr_summary)
    # survivors only ever flow into a paper-only next protocol queue
    for row in result.next_protocol_queue:
        assert row["queue_decision"] == "NEEDS_EXECUTION_AWARE_PROTOCOL"

    out = write_alpha_signal_strict_falsification_artifacts(tmp_path / "v4", result)
    assert {path.name for path in out.iterdir()} == set(payload["evidence_artifacts"])
    saved = json.loads((out / "alpha_signal_strict_falsification.json").read_text())
    assert saved["readiness_baseline_verdict"] == "NOT_READY"
    assert saved["candidate_universe_freeze"]["new_signal_dags_generated"] is False


def test_cli_strict_falsification_uses_public_no_credential_no_live_path(monkeypatch, tmp_path: Path) -> None:
    source_frame = synthetic_frame(rows=420)
    v3_artifact = write_v3_fixture_artifact(tmp_path, source_frame, candidate_budget=26)
    frame = synthetic_frame(rows=1300)
    args = Namespace(
        source_v3_artifact=str(v3_artifact),
        symbol="BTCUSDT",
        interval="1h",
        months="2024-01,2024-02",
        cache_dir="unused-cache",
        funding_source="public-binance",
        funding_cache_dir="unused-funding-cache",
        reports_root=str(tmp_path / "reports"),
        output_dir=None,
        overwrite=False,
        command=None,
        changed_file=None,
    )
    monkeypatch.setattr(quantctl, "load_market_data_bundle", lambda **_: market_bundle(frame))
    monkeypatch.setattr(quantctl, "fetch_funding_rate_history", lambda **_: funding_bundle(frame))
    monkeypatch.setenv("BINANCE_API_KEY", "SHOULD_NOT_BE_READ")
    monkeypatch.setenv("BINANCE_API_SECRET", "SHOULD_NOT_BE_READ")
    real_getenv = os.getenv

    def guarded_getenv(key, default=None):
        if key in {"BINANCE_API_KEY", "BINANCE_API_SECRET"}:
            raise AssertionError(f"strict falsification must not read {key}")
        return real_getenv(key, default)

    monkeypatch.setattr(os, "getenv", guarded_getenv)
    assert quantctl.cmd_alpha_signal_strict_falsification_run(args) == 0
    report_dirs = list((tmp_path / "reports" / "loops").iterdir())
    assert len(report_dirs) == 1
    payload = json.loads((report_dirs[0] / "alpha_signal_strict_falsification.json").read_text())
    assert payload["suite_decision"] == "STRICT_FALSIFICATION_PROTOCOL_INFRA_ONLY"
    assert payload["readiness_baseline_verdict"] == "NOT_READY"
    assert payload["tsmom_state_changed"] is False
    assert payload["safety_gates"]["no_api_key_used"] is True
    assert payload["live_or_testnet_path_created"] is False
