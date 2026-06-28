from __future__ import annotations

import json
import os
import shutil
from argparse import Namespace
from pathlib import Path
from typing import Any, cast

import pandas as pd
import pytest

from parangsae.apps.cli import quantctl
from parangsae.marketdata.binance_funding import FundingRateBundle
from parangsae.marketdata.binance_vision import MarketDataBundle
from parangsae.research.loop_artifacts import existing_loop_artifact_type
from parangsae.research.multi_asset_alpha_factory import MultiAssetSignalSpec
from parangsae.research.multi_asset_strict_falsification import (
    MULTI_ASSET_STRICT_FALSIFICATION_FORBIDDEN_OUTPUTS,
    MULTI_ASSET_STRICT_FALSIFICATION_REQUIRED_FILES,
    V9_FALSIFICATION_GOVERNANCE_REQUIRED_FILES,
    benjamini_hochberg_q_values,
    block_permutation_indices,
    build_artifact_hash_manifest,
    build_strict_chronological_folds,
    default_multi_asset_strict_oos_fdr_falsification_output_dir,
    evaluate_multi_asset_strict_oos_fdr_falsification,
    evaluate_v9_falsification_lineage_governance,
    family_passes_all_horizons,
    materialize_frozen_v8_queue,
    moving_block_permutation_null,
    read_v8_strict_falsification_source_state,
    strict_symbol_concentration_status,
    validate_v9_falsification_artifact_invariants,
    write_v9_falsification_lineage_governance_artifacts,
    write_multi_asset_strict_oos_fdr_falsification_artifacts,
)


def timestamp_at(index: pd.Index, position: int) -> pd.Timestamp:
    return cast(pd.Timestamp, pd.Timestamp(cast(Any, index[position])))


def synthetic_asset_frame(symbol: str, *, rows: int = 520, start: str = "2024-01-01") -> pd.DataFrame:
    index = pd.date_range(start, periods=rows, freq="1h", tz="UTC")
    offset = float(sum(ord(char) for char in symbol) % 17)
    trend = pd.Series(range(rows), index=index, dtype=float) * (0.03 + offset / 5000.0)
    cycle = pd.Series([((idx + int(offset)) % 24 - 12) * 0.04 for idx in range(rows)], index=index)
    close = 100.0 + offset + trend + cycle
    return pd.DataFrame(
        {
            "Open": close.shift(1).fillna(close.iloc[0]),
            "High": close + 1.0,
            "Low": close - 1.0,
            "Close": close,
            "Volume": 1000.0 + offset * 5.0 + (pd.Series(range(rows), index=index) % 13) * 7.0,
        },
        index=index,
    )


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


def spec(
    signal_id: str,
    family: str,
    feature: str,
    symbol: str,
    *,
    depth: int,
    complexity: float,
) -> dict[str, Any]:
    return MultiAssetSignalSpec(
        signal_id=signal_id,
        signal_family=family,
        feature_name=feature,
        symbol_scope=(symbol,),
        pair_scope=None,
        depth=depth,
        max_depth=4,
        complexity_score=complexity,
        lag_bars=1,
        uses_funding=False,
        uses_future_data=False,
        generation_seed=123,
        generation_rule_id=f"{family}:{feature}",
    ).to_dict()


def v8_artifact(
    tmp_path: Path,
    *,
    queue_families: tuple[str, ...] = ("correlation_breakdown", "volatility_dispersion"),
) -> Path:
    out = tmp_path / "v8"
    out.mkdir(parents=True, exist_ok=True)
    payload = {
        "schema_version": 8,
        "loop_id": "20260628-panel-ready-multi-asset-alpha-prefilter-rerun-v8",
        "loop_namespace": "panel_ready_multi_asset_alpha_prefilter_rerun_v8_only_not_readiness",
        "suite_decision": "NEEDS_STRICT_OOS_FALSIFICATION" if queue_families else "NO_SURVIVING_SIGNAL_FAMILY",
        "readiness_baseline_verdict": "NOT_READY",
        "source_v7_artifact_path": "reports/loops/20260628-public-multi-asset-data-backfill-v7",
        "source_v7_panel_ready": True,
        "symbol_universe_requested": ["BTCUSDT", "ETHUSDT", "SOLUSDT"],
        "symbol_universe_used": ["BTCUSDT", "ETHUSDT", "SOLUSDT"],
        "excluded_symbols": [],
        "common_start": "2024-01-01T00:00:00Z",
        "common_end": "2024-01-22T15:00:00Z",
        "common_bar_count": 520,
        "interval": "1h",
        "target_months": ["2024-01"],
        "generated_signal_count": 4,
        "evaluated_signal_count": 4,
        "effective_signal_breadth": 2,
        "duplicate_ratio": 0.0,
        "families_evaluated": list(queue_families),
        "horizons_evaluated": [8, 24, 72],
        "candidate_budget": 5000,
        "max_depth": 4,
        "multiple_testing_boundary": "V8 diagnostic only",
        "leakage_boundary": "signals shifted before labels",
        "claim_boundary": "V8 paper-only hypotheses; not edge/readiness/trading permission",
        "safety_gates": {
            "no_live_orders_submitted": True,
            "no_testnet_orders_submitted": True,
            "no_account_data_used": True,
            "no_api_key_used": True,
            "no_private_order_or_fill_data_used": True,
        },
        "no_live_orders_submitted": True,
        "no_testnet_orders_submitted": True,
        "no_account_data_used": True,
        "no_api_key_used": True,
        "no_private_order_or_fill_data_used": True,
        "holdout_retune_used": False,
        "strategy_optimization_performed": False,
        "live_or_testnet_path_created": False,
        "data_download_performed": False,
        "tsmom_state_changed": False,
    }
    (out / "panel_ready_multi_asset_alpha_prefilter.json").write_text(json.dumps(payload, sort_keys=True) + "\n")
    catalog = [
        spec(
            "v5.correlation_breakdown.rolling_correlation_to_btc.ETHUSDT.d2",
            "correlation_breakdown",
            "rolling_correlation_to_btc",
            "ETHUSDT",
            depth=2,
            complexity=1.5,
        ),
        spec(
            "v5.correlation_breakdown.decorrelation_shock.ETHUSDT.d3",
            "correlation_breakdown",
            "decorrelation_shock",
            "ETHUSDT",
            depth=3,
            complexity=2.0,
        ),
        spec(
            "v5.volatility_dispersion.high_volatility_rank.BTCUSDT.d1",
            "volatility_dispersion",
            "high_volatility_rank",
            "BTCUSDT",
            depth=1,
            complexity=1.0,
        ),
        spec(
            "v5.volatility_dispersion.volatility_change_rank.BTCUSDT.d2",
            "volatility_dispersion",
            "volatility_change_rank",
            "BTCUSDT",
            depth=2,
            complexity=1.4,
        ),
    ]
    (out / "signal_catalog.jsonl").write_text("".join(json.dumps(row, sort_keys=True) + "\n" for row in catalog))
    clusters = pd.DataFrame(
        [
            {
                "signal_id": row["signal_id"],
                "signal_family": row["signal_family"],
                "duplicate_cluster_id": f"MC{idx:04d}",
                "cluster_size": 1,
                "cluster_members": row["signal_id"],
                "representative_signal_id": row["signal_id"],
                "representative_rule": "lowest_complexity_then_depth_then_lexicographic_signal_id",
                "is_redundant_duplicate": False,
                "max_abs_correlation_to_existing_signal": 0.0,
                "complexity_score": row["complexity_score"],
                "depth": row["depth"],
            }
            for idx, row in enumerate(catalog, start=1)
        ]
    )
    clusters.to_csv(out / "signal_correlation_clusters.csv", index=False)
    pd.DataFrame(catalog).to_csv(out / "signal_diagnostics.csv", index=False)
    pd.DataFrame([], columns=["signal_family"]).to_csv(out / "signal_family_summary.csv", index=False)
    pd.DataFrame([], columns=["signal_family"]).to_csv(out / "oos_prefilter_summary.csv", index=False)
    pd.DataFrame([], columns=["signal_family"]).to_csv(out / "null_baseline_comparison.csv", index=False)
    pd.DataFrame([], columns=["scope"]).to_csv(out / "multiple_testing_summary.csv", index=False)
    pd.DataFrame([], columns=["signal_family"]).to_csv(out / "shortlist.csv", index=False)
    pd.DataFrame([], columns=["gate"]).to_csv(out / "failed_gate_rollup.csv", index=False)
    queue_rows = [
        {
            "signal_family": family,
            "queue_decision": "NEEDS_STRICT_OOS_FALSIFICATION",
            "paper_only": True,
            "effective_signal_breadth": 1,
            "corresponding_oos_metric": -0.03,
            "reason": "strict prefilter diagnostic only",
            "claim_boundary": "paper-only",
        }
        for family in queue_families
    ]
    pd.DataFrame(
        queue_rows,
        columns=[
            "signal_family",
            "queue_decision",
            "paper_only",
            "effective_signal_breadth",
            "corresponding_oos_metric",
            "reason",
            "claim_boundary",
        ],
    ).to_csv(out / "next_strict_falsification_queue.csv", index=False)
    return out


def test_v8_queue_is_read_and_frozen_with_signal_ids_preserved(tmp_path: Path) -> None:
    state = read_v8_strict_falsification_source_state(v8_artifact(tmp_path))
    frozen = materialize_frozen_v8_queue(state, horizons=(8, 24, 72))
    assert state["payload"]["source_v7_panel_ready"] is True
    assert len(state["queue_rows"]) == 2
    assert len(frozen) == 6
    assert {row["signal_family"] for row in frozen} == {"correlation_breakdown", "volatility_dispersion"}
    assert {row["horizon_bars"] for row in frozen} == {8, 24, 72}
    assert all(row["signal_id"].startswith("v5.") for row in frozen)
    assert all(row["frozen_before_evaluation"] is True for row in frozen)
    assert all(row["no_best_horizon_selection"] is True for row in frozen)
    assert all(row["horizon_aggregation_rule"] == "all_fixed_horizons_must_clear" for row in frozen)
    assert all(row["permutation_block_size_rule"] == "block_size_equals_horizon_bars" for row in frozen)
    assert all(row["block_size"] == row["horizon_bars"] for row in frozen)


def test_non_empty_v8_queue_with_missing_catalog_family_fails_closed(tmp_path: Path) -> None:
    artifact = v8_artifact(tmp_path)
    queue = pd.read_csv(artifact / "next_strict_falsification_queue.csv")
    queue["signal_family"] = "missing_family_from_catalog"
    queue.to_csv(artifact / "next_strict_falsification_queue.csv", index=False)
    state = read_v8_strict_falsification_source_state(artifact)
    with pytest.raises(ValueError, match="no matching signal_catalog entries"):
        materialize_frozen_v8_queue(state, horizons=(8, 24, 72))
    with pytest.raises(ValueError, match="no matching signal_catalog entries"):
        evaluate_multi_asset_strict_oos_fdr_falsification(
            market_bundles={},
            funding_bundles={},
            source_v8_artifact_path=artifact,
            horizons=(8, 24, 72),
            permutation_count=5,
        )


def test_empty_v8_queue_fails_safe_schema_only_artifact(tmp_path: Path) -> None:
    result = evaluate_multi_asset_strict_oos_fdr_falsification(
        market_bundles={},
        funding_bundles={},
        source_v8_artifact_path=v8_artifact(tmp_path, queue_families=()),
        horizons=(8, 24, 72),
        permutation_count=5,
        commands=["uv run quantctl multi-asset-strict-oos-fdr-falsification-run"],
        changed_files=["src/parangsae/research/multi_asset_strict_falsification.py"],
    )
    assert result.payload["suite_decision"] == "NO_V8_QUEUE_TO_FALSIFY"
    assert result.payload["frozen_queue_count"] == 0
    assert result.payload["data_download_performed"] is False
    out = write_multi_asset_strict_oos_fdr_falsification_artifacts(tmp_path / "v9-empty", result)
    assert {path.name for path in out.iterdir()} == set(MULTI_ASSET_STRICT_FALSIFICATION_REQUIRED_FILES)
    assert existing_loop_artifact_type(out) == "multi-asset-strict-oos-fdr-falsification-run"
    assert list(pd.read_csv(out / "next_mechanism_research_queue.csv").columns) == [
        "signal_family",
        "queue_decision",
        "paper_only",
        "reason",
        "claim_boundary",
    ]


def test_chronological_folds_are_ordered_purged_embargoed_and_non_overlapping() -> None:
    timestamps = pd.date_range("2024-01-01", periods=420, freq="1h", tz="UTC")
    folds = build_strict_chronological_folds(timestamps, horizons=(8, 24, 72), fold_count=2)
    assert folds
    for fold in folds:
        assert fold["train_start"] < fold["train_end"] < fold["validation_start"] < fold["validation_end"]
        assert fold["validation_end"] < fold["test_start"] < fold["test_end"]
        assert fold["purge_bars"] == 72
        assert fold["embargo_bars"] == 72
        assert fold["horizon_bars"] in {8, 24, 72}
        assert fold["purged_observation_count"] >= fold["horizon_bars"]
        assert fold["embargoed_observation_count"] >= 72
        assert fold["fold_sample_count"] > 0


def test_block_permutation_and_bh_fdr_are_deterministic_and_predeclared() -> None:
    first_indices = block_permutation_indices(length=10, block_size=4, seed=11)
    second_indices = block_permutation_indices(length=10, block_size=4, seed=11)
    assert first_indices == second_indices
    blocks = [first_indices[idx : idx + 4] for idx in range(0, len(first_indices), 4)]
    assert sorted(len(block) for block in blocks) == [2, 4, 4]
    assert all(block == list(range(block[0], block[0] + len(block))) for block in blocks)

    signal = pd.Series([1.0, 2.0, 3.0, 4.0, 5.0, 6.0])
    label = pd.Series([1.0, 1.5, 2.5, 3.5, 4.5, 5.5])
    first_null = moving_block_permutation_null(
        signal,
        label,
        horizon_bars=3,
        permutation_count=20,
        seed=123,
    )
    second_null = moving_block_permutation_null(
        signal,
        label,
        horizon_bars=3,
        permutation_count=20,
        seed=123,
    )
    assert first_null == second_null
    assert first_null["block_size"] == 3
    assert first_null["permutation_block_size_rule"] == "block_size_equals_horizon_bars"

    q_values, significant = benjamini_hochberg_q_values([0.001, 0.02, 0.03, 0.2], alpha=0.05)
    assert q_values == [0.004, 0.04, 0.04, 0.2]
    assert significant == [True, True, True, False]
    generator_q_values, generator_significant = benjamini_hochberg_q_values(
        iter([0.001, 0.02, 0.03, 0.2]),
        alpha=0.05,
    )
    assert generator_q_values == q_values
    assert generator_significant == significant


def test_all_horizon_family_gate_prevents_best_horizon_promotion() -> None:
    rows = [
        {"horizon_bars": 8, "horizon_survived": True},
        {"horizon_bars": 24, "horizon_survived": False},
        {"horizon_bars": 72, "horizon_survived": True},
    ]
    assert family_passes_all_horizons(rows, horizons=(8, 24, 72)) is False
    assert family_passes_all_horizons(
        [
            {"horizon_bars": 8, "horizon_survived": True},
            {"horizon_bars": 24, "horizon_survived": True},
            {"horizon_bars": 72, "horizon_survived": True},
        ],
        horizons=(8, 24, 72),
    ) is True


def test_symbol_concentration_detection() -> None:
    concentrated = strict_symbol_concentration_status(
        [{"symbol": "BTCUSDT", "test_ic": 0.2}, {"symbol": "ETHUSDT", "test_ic": 0.0}, {"symbol": "SOLUSDT", "test_ic": 0.0}]
    )
    balanced = strict_symbol_concentration_status(
        [{"symbol": "BTCUSDT", "test_ic": 0.1}, {"symbol": "ETHUSDT", "test_ic": -0.08}, {"symbol": "SOLUSDT", "test_ic": 0.07}]
    )
    assert concentrated["symbol_concentration"] == "high"
    assert concentrated["symbol_consistency_rate"] < 0.5
    assert balanced["symbol_concentration"] == "normal"


def test_v9_evaluation_is_frozen_deterministic_safe_and_uses_no_signal_generation(monkeypatch, tmp_path: Path) -> None:
    frames = {symbol: synthetic_asset_frame(symbol) for symbol in ("BTCUSDT", "ETHUSDT", "SOLUSDT")}
    markets = {symbol: market_bundle(symbol, frame) for symbol, frame in frames.items()}

    def fail_generation(*args: Any, **kwargs: Any) -> None:
        raise AssertionError("V9 must not call V5 signal generation")

    monkeypatch.setattr("parangsae.research.multi_asset_alpha_factory.generate_multi_asset_signal_specs", fail_generation)
    v8 = v8_artifact(tmp_path)
    with pytest.raises(ValueError, match="complete frozen V8 symbol universe"):
        evaluate_multi_asset_strict_oos_fdr_falsification(
            market_bundles={"BTCUSDT": markets["BTCUSDT"]},
            funding_bundles={"BTCUSDT": funding_bundle("BTCUSDT", frames["BTCUSDT"])},
            source_v8_artifact_path=v8,
            horizons=(8, 24, 72),
            fold_count=1,
            permutation_count=20,
            seed=101,
        )
    first = evaluate_multi_asset_strict_oos_fdr_falsification(
        market_bundles=markets,
        funding_bundles={symbol: funding_bundle(symbol, frames[symbol]) for symbol in frames},
        source_v8_artifact_path=v8,
        horizons=(8, 24, 72),
        fold_count=1,
        permutation_count=20,
        seed=101,
        commands=["uv run quantctl multi-asset-strict-oos-fdr-falsification-run"],
        changed_files=["src/parangsae/research/multi_asset_strict_falsification.py"],
    )
    second = evaluate_multi_asset_strict_oos_fdr_falsification(
        market_bundles=markets,
        funding_bundles={symbol: funding_bundle(symbol, frames[symbol]) for symbol in frames},
        source_v8_artifact_path=v8,
        horizons=(8, 24, 72),
        fold_count=1,
        permutation_count=20,
        seed=101,
    )
    assert first.frozen_queue == second.frozen_queue
    assert first.permutation_null_results == second.permutation_null_results
    assert first.null_baseline_stress == second.null_baseline_stress
    assert first.payload["source_v8_queue_count"] == 2
    assert first.payload["frozen_queue_count"] == 6
    assert first.payload["evaluated_signal_count"] == 6
    assert first.payload["readiness_baseline_verdict"] == "NOT_READY"
    assert first.payload["data_download_performed"] is False
    assert first.payload["safety_gates"]["no_api_key_used"] is True
    assert first.payload["holdout_retune_used"] is False
    assert first.payload["strategy_optimization_performed"] is False
    assert first.payload["live_or_testnet_path_created"] is False
    assert first.payload["tsmom_state_changed"] is False
    assert {row["signal_id"] for row in first.frozen_queue}.issubset(
        {json.loads(line)["signal_id"] for line in (v8 / "signal_catalog.jsonl").read_text().splitlines()}
    )
    assert all(row["lag_bars"] >= 1 for row in first.signal_strict_oos_diagnostics)
    assert all(row["leakage_check_status"] == "passed_shifted_by_decision_lag" for row in first.signal_strict_oos_diagnostics)
    assert all(row["regime_uses_forward_labels"] is False for row in first.regime_robustness)
    assert all(row["baseline_uses_current_bar_label"] is False for row in first.null_baseline_stress)
    assert all(row["block_size"] == row["horizon_bars"] for row in first.permutation_null_results)
    assert not any(first.payload["suite_decision"] == forbidden for forbidden in MULTI_ASSET_STRICT_FALSIFICATION_FORBIDDEN_OUTPUTS)


def test_v9_artifact_writing_and_required_fields(tmp_path: Path) -> None:
    frames = {symbol: synthetic_asset_frame(symbol) for symbol in ("BTCUSDT", "ETHUSDT", "SOLUSDT")}
    markets = {symbol: market_bundle(symbol, frame) for symbol, frame in frames.items()}
    result = evaluate_multi_asset_strict_oos_fdr_falsification(
        market_bundles=markets,
        funding_bundles={symbol: None for symbol in frames},
        source_v8_artifact_path=v8_artifact(tmp_path),
        horizons=(8, 24, 72),
        fold_count=1,
        permutation_count=5,
        commands=["uv run quantctl multi-asset-strict-oos-fdr-falsification-run"],
        changed_files=["src/parangsae/research/multi_asset_strict_falsification.py"],
    )
    out = write_multi_asset_strict_oos_fdr_falsification_artifacts(tmp_path / "v9", result)
    assert {path.name for path in out.iterdir()} == set(MULTI_ASSET_STRICT_FALSIFICATION_REQUIRED_FILES)
    payload = json.loads((out / "multi_asset_strict_oos_fdr_falsification.json").read_text())
    for field in (
        "loop_id",
        "loop_namespace",
        "suite_decision",
        "readiness_baseline_verdict",
        "source_v8_artifact_path",
        "source_v8_queue_count",
        "frozen_queue_count",
        "evaluated_signal_count",
        "evaluated_family_count",
        "family_count",
        "horizon_count",
        "chronological_fold_count",
        "purge_bars",
        "embargo_bars",
        "permutation_count",
        "block_size",
        "total_signal_tests",
        "duplicate_adjusted_test_count",
        "effective_signal_breadth",
        "families_killed_count",
        "families_watch_count",
        "families_negative_control_count",
        "multiple_testing_boundary",
        "cannot_claim_edge_due_to_multiple_testing",
        "fdr_boundary",
        "leakage_boundary",
        "claim_boundary",
        "safety_gates",
        "tsmom_state_changed",
        "holdout_retune_used",
        "strategy_optimization_performed",
        "live_or_testnet_path_created",
        "data_download_performed",
        "validation_commands",
        "changed_files",
    ):
        assert field in payload
    assert payload["readiness_baseline_verdict"] == "NOT_READY"
    assert payload["cannot_claim_edge_due_to_multiple_testing"] is True
    assert payload["no_live_orders_submitted"] is True
    assert payload["no_testnet_orders_submitted"] is True
    assert payload["no_account_data_used"] is True
    assert payload["no_api_key_used"] is True
    assert payload["no_private_order_or_fill_data_used"] is True
    loop_payload = json.loads((out / "loop.json").read_text())
    assert loop_payload["loop_type"] == "validation"
    assert loop_payload["cannot_claim_edge_due_to_multiple_testing"] is True


def test_v10_lineage_manifest_is_deterministic_and_hashes_source_files(tmp_path: Path) -> None:
    artifact = v8_artifact(tmp_path)
    first = build_artifact_hash_manifest(
        artifact,
        ("panel_ready_multi_asset_alpha_prefilter.json", "next_strict_falsification_queue.csv"),
        source_role="source_v8",
    )
    second = build_artifact_hash_manifest(
        artifact,
        ("panel_ready_multi_asset_alpha_prefilter.json", "next_strict_falsification_queue.csv"),
        source_role="source_v8",
    )
    assert first == second
    assert [row["relative_path"] for row in first] == [
        "panel_ready_multi_asset_alpha_prefilter.json",
        "next_strict_falsification_queue.csv",
    ]
    assert all(row["exists"] is True for row in first)
    assert all(len(str(row["sha256"])) == 64 for row in first)
    before_hash = first[1]["sha256"]
    (artifact / "next_strict_falsification_queue.csv").write_text("signal_family\nchanged_family\n")
    changed = build_artifact_hash_manifest(
        artifact,
        ("next_strict_falsification_queue.csv",),
        source_role="source_v8",
    )
    assert changed[0]["sha256"] != before_hash


def test_v10_validates_committed_v9_kill_state_and_detects_mutated_queue(tmp_path: Path) -> None:
    committed_v9 = Path("reports/loops/20260628-multi-asset-strict-oos-fdr-falsification-v9")
    invariants = validate_v9_falsification_artifact_invariants(committed_v9)
    assert invariants
    assert {row["status"] for row in invariants} == {"passed"}
    by_name = {row["invariant"]: row for row in invariants}
    assert by_name["suite_decision"]["observed_value"] == "NO_FAMILY_SURVIVES_STRICT_FALSIFICATION"
    assert by_name["next_mechanism_queue_empty"]["observed_value"] == "0"
    assert by_name["family_decisions_falsified"]["observed_value"] == "6/6"

    mutated_v9 = tmp_path / "mutated-v9"
    shutil.copytree(committed_v9, mutated_v9)
    pd.DataFrame(
        [
            {
                "signal_family": "correlation_breakdown",
                "queue_decision": "WATCH_FAMILY_ONLY",
                "paper_only": True,
                "reason": "mutation should fail governance",
                "claim_boundary": "paper only",
            }
        ]
    ).to_csv(mutated_v9 / "next_mechanism_research_queue.csv", index=False)
    mutated_invariants = validate_v9_falsification_artifact_invariants(mutated_v9)
    assert any(
        row["invariant"] == "next_mechanism_queue_empty" and row["status"] == "failed"
        for row in mutated_invariants
    )


def test_v10_governance_freezes_killed_families_as_negative_controls_and_writes_artifacts(tmp_path: Path) -> None:
    committed_v9 = Path("reports/loops/20260628-multi-asset-strict-oos-fdr-falsification-v9")
    result = evaluate_v9_falsification_lineage_governance(
        source_v9_artifact_path=committed_v9,
        commands=["uv run quantctl multi-asset-v9-falsification-lineage-governance-run"],
        changed_files=["src/parangsae/research/multi_asset_strict_falsification.py"],
    )
    assert result.payload["suite_decision"] == "V9_KILL_STATE_GOVERNANCE_LOCKED"
    assert result.payload["readiness_baseline_verdict"] == "NOT_READY"
    assert result.payload["source_v9_suite_decision"] == "NO_FAMILY_SURVIVES_STRICT_FALSIFICATION"
    assert result.payload["source_v9_next_queue_empty"] is True
    assert result.payload["negative_control_family_count"] == 2
    assert result.payload["disallowed_revival_family_count"] == 2
    assert result.payload["data_download_performed"] is False
    assert result.payload["tsmom_state_changed"] is False
    assert all(row["negative_control_status"] == "frozen_negative_control" for row in result.negative_control_registry)
    assert all(
        row["revival_allowed_without_new_preregistration"] is False
        for row in result.disallowed_revival_registry
    )
    out = write_v9_falsification_lineage_governance_artifacts(tmp_path / "v10", result)
    assert {path.name for path in out.iterdir()} == set(V9_FALSIFICATION_GOVERNANCE_REQUIRED_FILES)
    assert existing_loop_artifact_type(out) == "multi-asset-v9-falsification-lineage-governance-run"
    payload = json.loads((out / "v9_falsification_lineage_governance.json").read_text())
    assert payload["suite_decision"] == "V9_KILL_STATE_GOVERNANCE_LOCKED"
    assert payload["no_live_orders_submitted"] is True
    assert payload["no_testnet_orders_submitted"] is True
    assert pd.read_csv(out / "negative_control_registry.csv")["signal_family"].tolist() == [
        "correlation_breakdown",
        "volatility_dispersion",
    ]


def test_cli_v10_is_no_credential_no_live_no_download_path(monkeypatch, tmp_path: Path) -> None:
    monkeypatch.setenv("BINANCE_API_KEY", "SHOULD_NOT_BE_READ")
    monkeypatch.setenv("BINANCE_API_SECRET", "SHOULD_NOT_BE_READ")
    real_getenv = os.getenv

    def guarded_getenv(key: str, default: str | None = None) -> str | None:
        if key in {"BINANCE_API_KEY", "BINANCE_API_SECRET"}:
            raise AssertionError(f"V10 must not read {key}")
        return real_getenv(key, default)

    monkeypatch.setattr(os, "getenv", guarded_getenv)
    args = Namespace(
        source_v9_artifact="reports/loops/20260628-multi-asset-strict-oos-fdr-falsification-v9",
        source_v8_artifact=None,
        reports_root=str(tmp_path / "reports"),
        output_dir=None,
        overwrite=False,
        command=None,
        changed_file=None,
    )
    assert quantctl.cmd_multi_asset_v9_falsification_lineage_governance_run(args) == 0
    report_dirs = list((tmp_path / "reports" / "loops").iterdir())
    assert len(report_dirs) == 1
    payload = json.loads((report_dirs[0] / "v9_falsification_lineage_governance.json").read_text())
    assert payload["data_download_performed"] is False
    assert payload["readiness_baseline_verdict"] == "NOT_READY"
    assert payload["safety_gates"]["no_api_key_used"] is True
    assert payload["live_or_testnet_path_created"] is False


def test_cli_v9_is_no_credential_no_live_no_download_path(monkeypatch, tmp_path: Path) -> None:
    frames = {symbol: synthetic_asset_frame(symbol) for symbol in ("BTCUSDT", "ETHUSDT", "SOLUSDT")}
    loader_calls = 0

    def fake_loader(args: Namespace, *, symbols: tuple[str, ...], interval: str, months: tuple[str, ...]):
        nonlocal loader_calls
        loader_calls += 1
        assert args.cache_mode == "cache-only"
        assert interval == "1h"
        assert months == ("2024-01",)
        market = {symbol: market_bundle(symbol, frames[symbol]) for symbol in symbols}
        funding = {symbol: None for symbol in symbols}
        return market, funding, [], {symbol: "disabled" for symbol in symbols}

    monkeypatch.setattr(quantctl, "_load_multi_asset_public_bundles", fake_loader)
    monkeypatch.setattr(quantctl, "validate_interval", lambda interval: str(interval))
    monkeypatch.setenv("BINANCE_API_KEY", "SHOULD_NOT_BE_READ")
    monkeypatch.setenv("BINANCE_API_SECRET", "SHOULD_NOT_BE_READ")
    real_getenv = os.getenv

    def guarded_getenv(key: str, default: str | None = None) -> str | None:
        if key in {"BINANCE_API_KEY", "BINANCE_API_SECRET"}:
            raise AssertionError(f"V9 must not read {key}")
        return real_getenv(key, default)

    monkeypatch.setattr(os, "getenv", guarded_getenv)
    mismatch_args = Namespace(
        source_v8_artifact=str(v8_artifact(tmp_path)),
        interval="5m",
        cache_dir="unused-cache",
        funding_source="none",
        funding_cache_dir="unused-funding-cache",
        min_bars_per_symbol=50,
        fold_count=1,
        permutation_count=5,
        seed=101,
        reports_root=str(tmp_path / "mismatch-reports"),
        output_dir=None,
        overwrite=False,
        command=None,
        changed_file=None,
    )
    assert quantctl.cmd_multi_asset_strict_oos_fdr_falsification_run(mismatch_args) == 2
    assert loader_calls == 0
    args = Namespace(
        source_v8_artifact=str(v8_artifact(tmp_path)),
        interval=None,
        cache_dir="unused-cache",
        funding_source="none",
        funding_cache_dir="unused-funding-cache",
        min_bars_per_symbol=50,
        fold_count=1,
        permutation_count=5,
        seed=101,
        reports_root=str(tmp_path / "reports"),
        output_dir=None,
        overwrite=False,
        command=None,
        changed_file=None,
    )
    assert quantctl.cmd_multi_asset_strict_oos_fdr_falsification_run(args) == 0
    assert loader_calls == 1
    report_dirs = list((tmp_path / "reports" / "loops").iterdir())
    assert len(report_dirs) == 1
    payload = json.loads((report_dirs[0] / "multi_asset_strict_oos_fdr_falsification.json").read_text())
    assert payload["data_download_performed"] is False
    assert payload["readiness_baseline_verdict"] == "NOT_READY"
    assert payload["safety_gates"]["no_api_key_used"] is True
    assert payload["live_or_testnet_path_created"] is False


def test_v5_v6_v7_v8_v9_and_v10_commands_are_wired() -> None:
    parser = quantctl.build_parser()
    assert parser.parse_args(["multi-asset-alpha-generator-strict-prefilter-run"]).func is (
        quantctl.cmd_multi_asset_alpha_generator_strict_prefilter_run
    )
    assert parser.parse_args(["public-multi-asset-panel-coverage-run"]).func is quantctl.cmd_public_multi_asset_panel_coverage_run
    assert parser.parse_args(["public-multi-asset-data-backfill-run"]).func is quantctl.cmd_public_multi_asset_data_backfill_run
    assert parser.parse_args(["panel-ready-multi-asset-alpha-prefilter-rerun"]).func is (
        quantctl.cmd_panel_ready_multi_asset_alpha_prefilter_rerun
    )
    parsed = parser.parse_args(["multi-asset-strict-oos-fdr-falsification-run"])
    assert parsed.func is quantctl.cmd_multi_asset_strict_oos_fdr_falsification_run
    assert parsed.source_v8_artifact.endswith("panel-ready-multi-asset-alpha-prefilter-rerun-v8")
    parsed_v10 = parser.parse_args(["multi-asset-v9-falsification-lineage-governance-run"])
    assert parsed_v10.func is quantctl.cmd_multi_asset_v9_falsification_lineage_governance_run
    assert parsed_v10.source_v9_artifact.endswith("multi-asset-strict-oos-fdr-falsification-v9")
    assert default_multi_asset_strict_oos_fdr_falsification_output_dir(today="20260628").name.endswith(
        "multi-asset-strict-oos-fdr-falsification-v9"
    )
