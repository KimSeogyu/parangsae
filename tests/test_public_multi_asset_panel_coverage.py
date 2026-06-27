from __future__ import annotations

import json
import os
from argparse import Namespace
from pathlib import Path
from typing import Any, cast

import pandas as pd

from parangsae.apps.cli import quantctl
from parangsae.marketdata.binance_funding import FundingRateBundle, funding_cache_path
from parangsae.marketdata.binance_vision import DownloadRecord, MarketDataBundle, canonical_cache_path
from parangsae.research.loop_artifacts import existing_loop_artifact_type
from parangsae.research.public_multi_asset_panel_coverage import (
    PUBLIC_MULTI_ASSET_PANEL_DEFAULT_UNIVERSE,
    PUBLIC_MULTI_ASSET_PANEL_REQUIRED_FILES,
    SYMBOL_COVERAGE_COLUMNS,
    evaluate_public_multi_asset_panel_coverage,
    write_public_multi_asset_panel_coverage_artifacts,
)


def timestamp_at(index: pd.Index, position: int) -> pd.Timestamp:
    return cast(pd.Timestamp, pd.Timestamp(cast(Any, index[position])))


def synthetic_asset_frame(symbol: str, *, rows: int = 744, start: str = "2024-01-01") -> pd.DataFrame:
    index = pd.date_range(start, periods=rows, freq="1h", tz="UTC")
    offset = float(sum(ord(char) for char in symbol) % 19)
    close = pd.Series([100.0 + offset + i * 0.01 for i in range(rows)], index=index)
    frame = pd.DataFrame(
        {
            "Open": close.shift(1).fillna(close.iloc[0]),
            "High": close + 1.0,
            "Low": close - 1.0,
            "Close": close,
            "Volume": 1000.0 + offset,
        },
        index=index,
    )
    frame.attrs["closed_bar_only"] = True
    frame.attrs["open_bar_included"] = False
    frame.attrs["allow_zero_volume_bars"] = True
    return frame


def market_bundle(symbol: str, frame: pd.DataFrame, *, months: tuple[str, ...] = ("2024-01",)) -> MarketDataBundle:
    return MarketDataBundle(
        symbol=symbol,
        interval="1h",
        months=months,
        frame=frame,
        downloads=(
            DownloadRecord(
                url=f"https://data.binance.vision/{symbol}.zip",
                cache_path=f"cache/{symbol}.zip",
                sha256=f"sha-{symbol}",
                http_status=None,
                etag=None,
                last_modified=None,
                downloaded_at="2026-06-28T00:00:00Z",
                from_cache=True,
                bytes=10,
            ),
        ),
        normalized_sha256=f"ohlcv-{symbol}",
        quality={
            "rows": len(frame),
            "first_timestamp": timestamp_at(frame.index, 0).isoformat().replace("+00:00", "Z"),
            "last_timestamp": timestamp_at(frame.index, -1).isoformat().replace("+00:00", "Z"),
            "closed_bar_only": True,
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
            "funding_rate": [0.0001 for _ in funding_index],
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


def touch_ohlcv_cache(cache_dir: Path, symbol: str, months: tuple[str, ...] = ("2024-01",)) -> None:
    for month in months:
        path = canonical_cache_path(cache_dir, symbol, "1h", month)
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(b"placeholder")


def touch_funding_cache(cache_dir: Path, symbol: str, frame: pd.DataFrame) -> None:
    start = timestamp_at(frame.index, 0)
    end = timestamp_at(frame.index, -1)
    path = funding_cache_path(
        cache_dir,
        symbol,
        int(start.timestamp() * 1000),
        int(end.timestamp() * 1000),
    )
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("[]")


def v5_artifact(tmp_path: Path) -> Path:
    out = tmp_path / "v5"
    out.mkdir(parents=True, exist_ok=True)
    payload = {
        "loop_namespace": "multi_asset_alpha_generator_strict_prefilter_v5_only_not_readiness",
        "suite_decision": "NO_SURVIVING_SIGNAL_FAMILY",
        "readiness_baseline_verdict": "NOT_READY",
        "symbol_universe_used": ["BTCUSDT"],
        "excluded_symbols": [
            {
                "symbol": "ETHUSDT",
                "stage": "ohlcv_cache_availability",
                "reason": "missing cached OHLCV month(s): 2024-01",
            },
            {
                "symbol": "SOLUSDT",
                "stage": "ohlcv_cache_availability",
                "reason": "missing cached OHLCV month(s): 2024-01",
            },
        ],
    }
    (out / "multi_asset_alpha_factory.json").write_text(json.dumps(payload, indent=2) + "\n")
    return out


def test_v6_preserves_v5_exclusions_and_does_not_fabricate_missing_cache(tmp_path: Path) -> None:
    cache_dir = tmp_path / "ohlcv"
    touch_ohlcv_cache(cache_dir, "BTCUSDT")
    frames = {"BTCUSDT": synthetic_asset_frame("BTCUSDT")}
    called_symbols: list[str] = []

    def fake_market_loader(**kwargs: Any) -> MarketDataBundle:
        symbol = str(kwargs["symbol"])
        called_symbols.append(symbol)
        return market_bundle(symbol, frames[symbol])

    result = evaluate_public_multi_asset_panel_coverage(
        requested_symbols=("BTCUSDT", "ETHUSDT"),
        months=("2024-01",),
        cache_dir=cache_dir,
        funding_source="none",
        cache_mode="cache-only",
        minimum_required_symbols=2,
        source_v5_artifact_path=v5_artifact(tmp_path),
        market_loader=fake_market_loader,
    )
    payload = result.payload
    assert called_symbols == ["BTCUSDT"]
    assert payload["readiness_baseline_verdict"] == "NOT_READY"
    assert payload["suite_decision"] == "PANEL_NOT_READY_INSUFFICIENT_SYMBOLS"
    assert payload["usable_symbol_universe"] == ["BTCUSDT"]
    assert payload["v5_excluded_symbols_preserved"] is True
    assert payload["tsmom_state_changed"] is False
    assert payload["strategy_optimization_performed"] is False
    assert payload["live_or_testnet_path_created"] is False
    assert payload["safety_gates"]["no_api_key_used"] is True

    eth = next(row for row in result.symbol_coverage if row["symbol"] == "ETHUSDT")
    assert eth["included_in_panel"] is False
    assert eth["excluded_stage"] == "ohlcv_cache_availability"
    assert eth["v5_excluded_reason"] == "missing cached OHLCV month(s): 2024-01"
    assert eth["bar_count"] == 0
    assert any(row["symbol"] == "ETHUSDT" and row["data_type"] == "ohlcv" for row in result.missing_data_plan)


def test_v6_panel_ready_requires_minimum_symbols_and_common_history(tmp_path: Path) -> None:
    cache_dir = tmp_path / "ohlcv"
    symbols = ("BTCUSDT", "ETHUSDT", "SOLUSDT")
    for symbol in symbols:
        touch_ohlcv_cache(cache_dir, symbol)
    frames = {symbol: synthetic_asset_frame(symbol) for symbol in symbols}

    def fake_market_loader(**kwargs: Any) -> MarketDataBundle:
        symbol = str(kwargs["symbol"])
        return market_bundle(symbol, frames[symbol])

    ready = evaluate_public_multi_asset_panel_coverage(
        requested_symbols=symbols,
        months=("2024-01",),
        cache_dir=cache_dir,
        funding_source="none",
        cache_mode="cache-only",
        minimum_required_symbols=3,
        source_v5_artifact_path=v5_artifact(tmp_path),
        market_loader=fake_market_loader,
    )
    assert ready.payload["panel_ready_for_v5_rerun"] is True
    assert ready.payload["suite_decision"] == "PANEL_READY_FOR_V5_RERUN"
    assert ready.payload["common_bar_count"] == 744
    assert pd.Timestamp(ready.payload["common_start"]) < pd.Timestamp(ready.payload["common_end"])

    not_enough_symbols = evaluate_public_multi_asset_panel_coverage(
        requested_symbols=symbols,
        months=("2024-01",),
        cache_dir=cache_dir,
        funding_source="none",
        cache_mode="cache-only",
        minimum_required_symbols=4,
        source_v5_artifact_path=v5_artifact(tmp_path / "second"),
        market_loader=fake_market_loader,
    )
    assert not_enough_symbols.payload["panel_ready_for_v5_rerun"] is False
    assert not_enough_symbols.payload["suite_decision"] == "PANEL_NOT_READY_INSUFFICIENT_SYMBOLS"


def test_v6_common_history_fails_when_symbol_coverage_starts_late(tmp_path: Path) -> None:
    cache_dir = tmp_path / "ohlcv"
    symbols = ("BTCUSDT", "ETHUSDT", "SOLUSDT")
    for symbol in symbols:
        touch_ohlcv_cache(cache_dir, symbol)
    frames = {
        "BTCUSDT": synthetic_asset_frame("BTCUSDT"),
        "ETHUSDT": synthetic_asset_frame("ETHUSDT", rows=720, start="2024-01-02"),
        "SOLUSDT": synthetic_asset_frame("SOLUSDT"),
    }

    def fake_market_loader(**kwargs: Any) -> MarketDataBundle:
        symbol = str(kwargs["symbol"])
        return market_bundle(symbol, frames[symbol])

    result = evaluate_public_multi_asset_panel_coverage(
        requested_symbols=symbols,
        months=("2024-01",),
        cache_dir=cache_dir,
        funding_source="none",
        cache_mode="cache-only",
        minimum_required_symbols=3,
        source_v5_artifact_path=v5_artifact(tmp_path),
        market_loader=fake_market_loader,
    )
    assert result.payload["usable_symbol_count"] == 3
    assert result.payload["panel_ready_for_v5_rerun"] is False
    assert result.payload["suite_decision"] == "PANEL_NOT_READY_INSUFFICIENT_HISTORY"
    assert pd.Timestamp(result.payload["common_start"]) < pd.Timestamp(result.payload["common_end"])
    assert result.payload["common_bar_count"] < result.payload["minimum_common_bar_count"]


def test_v6_cache_only_funding_preflight_does_not_call_fetcher_when_cache_missing(tmp_path: Path) -> None:
    cache_dir = tmp_path / "ohlcv"
    touch_ohlcv_cache(cache_dir, "BTCUSDT")
    frame = synthetic_asset_frame("BTCUSDT")

    def fake_market_loader(**kwargs: Any) -> MarketDataBundle:
        return market_bundle(str(kwargs["symbol"]), frame)

    def fail_funding_loader(**kwargs: Any) -> FundingRateBundle:
        raise AssertionError("cache-only funding preflight must not fetch missing funding cache")

    result = evaluate_public_multi_asset_panel_coverage(
        requested_symbols=("BTCUSDT",),
        months=("2024-01",),
        cache_dir=cache_dir,
        funding_source="public-binance",
        funding_cache_dir=tmp_path / "funding",
        cache_mode="cache-only",
        minimum_required_symbols=1,
        source_v5_artifact_path=v5_artifact(tmp_path),
        market_loader=fake_market_loader,
        funding_loader=fail_funding_loader,
    )
    row = result.symbol_coverage[0]
    assert row["included_in_panel"] is True
    assert row["funding_status"] == "unavailable_missing_cache"
    assert row["funding_available"] is False
    assert any(item["data_type"] == "funding" for item in result.missing_data_plan)


def test_v6_artifact_writing_preserves_schema_and_headers(tmp_path: Path) -> None:
    cache_dir = tmp_path / "ohlcv"
    touch_ohlcv_cache(cache_dir, "BTCUSDT")
    frame = synthetic_asset_frame("BTCUSDT")

    def fake_market_loader(**kwargs: Any) -> MarketDataBundle:
        return market_bundle(str(kwargs["symbol"]), frame)

    result = evaluate_public_multi_asset_panel_coverage(
        requested_symbols=("BTCUSDT",),
        months=("2024-01",),
        cache_dir=cache_dir,
        funding_source="none",
        cache_mode="cache-only",
        minimum_required_symbols=1,
        source_v5_artifact_path=v5_artifact(tmp_path),
        market_loader=fake_market_loader,
        commands=["uv run quantctl public-multi-asset-panel-coverage-run"],
        changed_files=["src/parangsae/research/public_multi_asset_panel_coverage.py"],
    )
    out = write_public_multi_asset_panel_coverage_artifacts(
        tmp_path / "20260628-public-multi-asset-data-panel-coverage-v6",
        result,
    )
    assert {path.name for path in out.iterdir()} == set(PUBLIC_MULTI_ASSET_PANEL_REQUIRED_FILES)
    assert existing_loop_artifact_type(out) == "public-multi-asset-panel-coverage-run"
    saved = json.loads((out / "public_multi_asset_panel_coverage.json").read_text())
    assert saved["loop_id"] == "20260628-public-multi-asset-data-panel-coverage-v6"
    assert saved["readiness_baseline_verdict"] == "NOT_READY"
    assert (out / "symbol_coverage.csv").read_text().splitlines()[0] == ",".join(SYMBOL_COVERAGE_COLUMNS)
    assert (out / "missing_data_plan.csv").read_text().splitlines()[0].startswith("symbol,data_type,")
    assert "public-multi-asset-panel-coverage-run" in (out / "commands.txt").read_text()


def test_cli_v6_uses_public_no_credential_no_live_path_and_does_not_run_v5(monkeypatch, tmp_path: Path) -> None:
    cache_dir = tmp_path / "ohlcv"
    symbols = ("BTCUSDT", "ETHUSDT", "SOLUSDT")
    for symbol in symbols:
        touch_ohlcv_cache(cache_dir, symbol)
    frames = {symbol: synthetic_asset_frame(symbol) for symbol in symbols}

    def fake_market_loader(**kwargs: Any) -> MarketDataBundle:
        symbol = str(kwargs["symbol"])
        return market_bundle(symbol, frames[symbol])

    def fail_v5_prefilter(**kwargs: Any) -> object:
        raise AssertionError("V6 coverage CLI must not invoke V5 alpha prefilter")

    monkeypatch.setattr(quantctl, "load_market_data_bundle", fake_market_loader)
    monkeypatch.setattr(quantctl, "evaluate_multi_asset_alpha_prefilter", fail_v5_prefilter)
    monkeypatch.setenv("BINANCE_API_KEY", "SHOULD_NOT_BE_READ")
    monkeypatch.setenv("BINANCE_API_SECRET", "SHOULD_NOT_BE_READ")
    real_getenv = os.getenv

    def guarded_getenv(key: str, default: str | None = None) -> str | None:
        if key in {"BINANCE_API_KEY", "BINANCE_API_SECRET"}:
            raise AssertionError(f"V6 must not read {key}")
        return real_getenv(key, default)

    monkeypatch.setattr(os, "getenv", guarded_getenv)
    args = Namespace(
        source_v5_artifact=str(v5_artifact(tmp_path)),
        symbols=",".join(symbols),
        interval="1h",
        months="2024-01",
        cache_dir=str(cache_dir),
        funding_source="none",
        funding_cache_dir=str(tmp_path / "funding"),
        cache_mode="cache-only",
        minimum_required_symbols=3,
        reports_root=str(tmp_path / "reports"),
        output_dir=None,
        overwrite=False,
        command=None,
        changed_file=None,
    )
    assert quantctl.cmd_public_multi_asset_panel_coverage_run(args) == 0
    report_dirs = list((tmp_path / "reports" / "loops").iterdir())
    assert len(report_dirs) == 1
    payload = json.loads((report_dirs[0] / "public_multi_asset_panel_coverage.json").read_text())
    assert payload["suite_decision"] == "PANEL_READY_FOR_V5_RERUN"
    assert payload["readiness_baseline_verdict"] == "NOT_READY"
    assert payload["no_live_orders_submitted"] is True
    assert payload["no_testnet_orders_submitted"] is True
    assert payload["no_account_data_used"] is True
    assert payload["no_api_key_used"] is True
    assert payload["no_private_order_or_fill_data_used"] is True
    assert payload["tsmom_state_changed"] is False
    assert not (report_dirs[0] / "multi_asset_alpha_factory.json").exists()


def test_v1_to_v6_commands_are_wired_and_defaults_are_stable() -> None:
    parser = quantctl.build_parser()
    assert parser.parse_args(["alpha-signal-factory-composition-run"]).func is quantctl.cmd_alpha_signal_factory_composition_run
    assert parser.parse_args(["alpha-signal-dag-expansion-run"]).func is quantctl.cmd_alpha_signal_dag_expansion_run
    assert parser.parse_args(["alpha-signal-oos-fdr-validation-run"]).func is quantctl.cmd_alpha_signal_oos_fdr_validation_run
    assert parser.parse_args(["alpha-signal-strict-falsification-run"]).func is quantctl.cmd_alpha_signal_strict_falsification_run
    assert parser.parse_args(["multi-asset-alpha-generator-strict-prefilter-run"]).func is quantctl.cmd_multi_asset_alpha_generator_strict_prefilter_run
    parsed = parser.parse_args(["public-multi-asset-panel-coverage-run"])
    assert parsed.func is quantctl.cmd_public_multi_asset_panel_coverage_run
    assert parsed.symbols == ",".join(PUBLIC_MULTI_ASSET_PANEL_DEFAULT_UNIVERSE)
    assert parsed.minimum_required_symbols == 3
