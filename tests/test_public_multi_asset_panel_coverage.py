from __future__ import annotations

import json
import os
from argparse import Namespace
from pathlib import Path
from typing import Any, cast

import pandas as pd

from parangsae.apps.cli import quantctl
from parangsae.marketdata.binance_funding import FundingDownloadRecord, FundingRateBundle, funding_cache_path
from parangsae.marketdata.binance_vision import DownloadRecord, MarketDataBundle, canonical_cache_path
from parangsae.research.loop_artifacts import existing_loop_artifact_type
from parangsae.research.public_multi_asset_panel_coverage import (
    BACKFILL_PLAN_COLUMNS,
    FAILED_DOWNLOADS_COLUMNS,
    MISSING_DATA_PLAN_COLUMNS,
    PANEL_READINESS_COLUMNS,
    PROVENANCE_MANIFEST_COLUMNS,
    PUBLIC_MULTI_ASSET_DATA_BACKFILL_REQUIRED_FILES,
    PUBLIC_MULTI_ASSET_PANEL_DEFAULT_UNIVERSE,
    PUBLIC_MULTI_ASSET_PANEL_REQUIRED_FILES,
    SYMBOL_COVERAGE_COLUMNS,
    PublicMultiAssetDataBackfillResult,
    build_public_multi_asset_data_backfill_plan,
    evaluate_public_multi_asset_data_backfill,
    evaluate_public_multi_asset_panel_coverage,
    write_public_multi_asset_data_backfill_artifacts,
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


def v6_artifact(
    tmp_path: Path,
    *,
    requested_symbols: tuple[str, ...] = ("BTCUSDT", "ETHUSDT"),
    usable_symbols: tuple[str, ...] = ("BTCUSDT",),
    months: tuple[str, ...] = ("2024-01",),
    missing_by_symbol: dict[str, tuple[str, ...]] | None = None,
) -> Path:
    out = tmp_path / "v6"
    out.mkdir(parents=True, exist_ok=True)
    payload = {
        "loop_namespace": "public_multi_asset_data_panel_coverage_v6_only_not_alpha",
        "suite_decision": "PANEL_NOT_READY_INSUFFICIENT_SYMBOLS",
        "readiness_baseline_verdict": "NOT_READY",
        "requested_symbol_universe": list(requested_symbols),
        "usable_symbol_universe": list(usable_symbols),
        "requested_symbol_count": len(requested_symbols),
        "usable_symbol_count": len(usable_symbols),
        "requested_months": list(months),
        "interval": "1h",
        "common_start": f"{months[0]}-01T00:00:00Z",
        "common_end": cast(pd.Period, pd.Period(months[-1], freq="M")).end_time.floor("h").isoformat() + "Z",
        "common_bar_count": sum(int(pd.Period(month, freq="M").days_in_month) * 24 for month in months),
        "panel_ready_for_v5_rerun": False,
    }
    (out / "public_multi_asset_panel_coverage.json").write_text(json.dumps(payload, indent=2) + "\n")
    coverage_rows = [{"symbol": symbol} for symbol in requested_symbols]
    pd.DataFrame(coverage_rows, columns=list(SYMBOL_COVERAGE_COLUMNS)).to_csv(out / "symbol_coverage.csv", index=False)
    missing_rows: list[dict[str, Any]] = []
    for symbol, missing_months in (missing_by_symbol or {}).items():
        missing_rows.append(
            {
                "symbol": symbol,
                "data_type": "ohlcv",
                "missing_item": ",".join(missing_months),
                "reason": "missing cached OHLCV month(s): " + ",".join(missing_months),
                "remediation": "download public Binance Vision monthly kline cache files only",
                "public_only": True,
                "no_private_data_required": True,
                "cache_mode": "cache-only",
                "v5_excluded_reason": "",
            }
        )
    pd.DataFrame(missing_rows, columns=list(MISSING_DATA_PLAN_COLUMNS)).to_csv(out / "missing_data_plan.csv", index=False)
    pd.DataFrame([], columns=list(PANEL_READINESS_COLUMNS)).to_csv(out / "panel_readiness.csv", index=False)
    pd.DataFrame([], columns=list(PROVENANCE_MANIFEST_COLUMNS)).to_csv(out / "provenance_manifest.csv", index=False)
    return out


def cache_backed_market_loader(cache_dir: Path):
    def fake_market_loader(**kwargs: Any) -> MarketDataBundle:
        symbol = str(kwargs["symbol"])
        months = tuple(str(month) for month in kwargs["months"])
        for month in months:
            path = canonical_cache_path(cache_dir, symbol, "1h", month)
            if not path.exists():
                raise ValueError(f"missing cache for {symbol} {month}")
        rows = sum(int(pd.Period(month, freq="M").days_in_month) * 24 for month in months)
        return market_bundle(symbol, synthetic_asset_frame(symbol, rows=rows, start=f"{months[0]}-01"), months=months)

    return fake_market_loader


def downloader_that_writes_cache(cache_dir: Path, *, fail: set[tuple[str, str]] | None = None):
    calls: list[tuple[str, str]] = []

    def fake_downloader(**kwargs: Any) -> tuple[DownloadRecord, ...]:
        symbol = str(kwargs["symbol"])
        month = str(tuple(kwargs["months"])[0])
        calls.append((symbol, month))
        if fail and (symbol, month) in fail:
            raise ValueError(f"network unavailable for {symbol} {month}")
        path = canonical_cache_path(cache_dir, symbol, "1h", month)
        existed = path.exists()
        path.parent.mkdir(parents=True, exist_ok=True)
        if not existed:
            path.write_bytes(f"{symbol}-{month}".encode())
        return (
            DownloadRecord(
                url=f"https://data.binance.vision/{symbol}-{month}.zip",
                cache_path=str(path),
                sha256="cache-sha" if existed else f"sha-{symbol}-{month}",
                http_status=None if existed else 200,
                etag=None,
                last_modified=None,
                downloaded_at="2026-06-28T00:00:00Z",
                from_cache=existed,
                bytes=path.stat().st_size,
            ),
        )

    return fake_downloader


def fake_funding_loader(**kwargs: Any) -> FundingRateBundle:
    symbol = str(kwargs["symbol"])
    start = cast(pd.Timestamp, pd.Timestamp(kwargs["start_time"]))
    end = cast(pd.Timestamp, pd.Timestamp(kwargs["end_time"]))
    path = funding_cache_path(
        kwargs["cache_dir"],
        symbol,
        int(start.timestamp() * 1000),
        int(end.timestamp() * 1000),
    )
    existed = path.exists()
    path.parent.mkdir(parents=True, exist_ok=True)
    payload = b"[]"
    if not existed:
        path.write_bytes(payload)
    index = pd.date_range(start, end, freq="8h", tz="UTC")
    frame = pd.DataFrame(
        {
            "symbol": [symbol for _ in index],
            "funding_time_ms": [int(cast(pd.Timestamp, pd.Timestamp(item)).timestamp() * 1000) for item in index],
            "funding_rate": [0.0001 for _ in index],
            "mark_price": [100.0 for _ in index],
        },
        index=index,
    )
    return FundingRateBundle(
        symbol=symbol,
        frame=frame,
        downloads=(
            FundingDownloadRecord(
                url="https://fapi.binance.com/fapi/v1/fundingRate",
                cache_path=str(path),
                sha256="funding-sha",
                http_status=None if existed else 200,
                downloaded_at="2026-06-28T00:00:00Z",
                from_cache=existed,
                rows=len(frame),
                params={"symbol": symbol},
            ),
        ),
        normalized_sha256=f"funding-{symbol}",
        quality={"rows": len(frame), "symbol": symbol},
    )


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


def test_v7_reads_v6_missing_plan_and_generates_deterministic_backfill_plan(tmp_path: Path) -> None:
    cache_dir = tmp_path / "ohlcv"
    touch_ohlcv_cache(cache_dir, "BTCUSDT", ("2024-01",))
    source_v6 = v6_artifact(
        tmp_path,
        requested_symbols=("BTCUSDT", "ETHUSDT"),
        usable_symbols=("BTCUSDT",),
        months=("2024-01", "2024-02"),
        missing_by_symbol={"ETHUSDT": ("2024-01", "2024-02")},
    )

    plan_a = build_public_multi_asset_data_backfill_plan(
        source_v6_artifact_path=source_v6,
        cache_dir=cache_dir,
        funding_source="none",
    )
    plan_b = build_public_multi_asset_data_backfill_plan(
        source_v6_artifact_path=source_v6,
        cache_dir=cache_dir,
        funding_source="none",
    )
    assert plan_a == plan_b
    assert [tuple(row[column] for column in ("symbol", "data_type", "month")) for row in plan_a] == sorted(
        tuple(row[column] for column in ("symbol", "data_type", "month")) for row in plan_a
    )
    assert any(
        row["symbol"] == "ETHUSDT"
        and row["month"] == "2024-01"
        and row["plan_action"] == "download"
        and row["reason"] == "missing_ohlcv_month_from_v6_plan"
        for row in plan_a
    )
    assert any(row["symbol"] == "BTCUSDT" and row["month"] == "2024-01" and row["plan_action"] == "skip_existing" for row in plan_a)


def test_v7_failed_downloads_are_recorded_without_fabricating_data(tmp_path: Path) -> None:
    cache_dir = tmp_path / "ohlcv"
    touch_ohlcv_cache(cache_dir, "BTCUSDT", ("2024-01",))
    source_v6 = v6_artifact(
        tmp_path,
        requested_symbols=("BTCUSDT", "ETHUSDT"),
        usable_symbols=("BTCUSDT",),
        missing_by_symbol={"ETHUSDT": ("2024-01",)},
    )
    downloader = downloader_that_writes_cache(cache_dir, fail={("ETHUSDT", "2024-01")})

    result = evaluate_public_multi_asset_data_backfill(
        source_v6_artifact_path=source_v6,
        cache_dir=cache_dir,
        funding_source="none",
        minimum_required_symbols=3,
        ohlcv_downloader=downloader,
        market_loader=cache_backed_market_loader(cache_dir),
    )

    assert result.payload["readiness_baseline_verdict"] == "NOT_READY"
    assert result.payload["tsmom_state_changed"] is False
    assert result.payload["planned_download_count"] == 1
    assert result.payload["skipped_existing_count"] == 1
    assert result.payload["successful_download_count"] == 0
    assert result.payload["failed_download_count"] == 1
    assert result.payload["panel_ready_for_v5_rerun"] is False
    assert result.failed_downloads[0]["symbol"] == "ETHUSDT"
    assert result.failed_downloads[0]["failed_safe"] is True
    assert not canonical_cache_path(cache_dir, "ETHUSDT", "1h", "2024-01").exists()


def test_v7_panel_readiness_requires_three_symbols_and_can_become_true(tmp_path: Path) -> None:
    cache_dir = tmp_path / "ohlcv"
    touch_ohlcv_cache(cache_dir, "BTCUSDT", ("2024-01",))
    source_v6 = v6_artifact(
        tmp_path,
        requested_symbols=("BTCUSDT", "ETHUSDT", "SOLUSDT"),
        usable_symbols=("BTCUSDT",),
        missing_by_symbol={"ETHUSDT": ("2024-01",), "SOLUSDT": ("2024-01",)},
    )
    one_symbol = evaluate_public_multi_asset_data_backfill(
        source_v6_artifact_path=source_v6,
        requested_symbols=("BTCUSDT",),
        cache_dir=cache_dir,
        funding_source="none",
        minimum_required_symbols=3,
        market_loader=cache_backed_market_loader(cache_dir),
    )
    assert one_symbol.payload["panel_ready_for_v5_rerun"] is False
    assert one_symbol.payload["usable_symbol_count_after"] == 1

    downloader = downloader_that_writes_cache(cache_dir)
    ready = evaluate_public_multi_asset_data_backfill(
        source_v6_artifact_path=source_v6,
        cache_dir=cache_dir,
        funding_source="none",
        minimum_required_symbols=3,
        ohlcv_downloader=downloader,
        market_loader=cache_backed_market_loader(cache_dir),
    )
    assert ready.payload["panel_ready_for_v5_rerun"] is True
    assert ready.payload["suite_decision"] == "BACKFILL_COMPLETED_PANEL_READY_FOR_V5_RERUN"
    assert ready.payload["usable_symbol_universe_after"] == ["BTCUSDT", "ETHUSDT", "SOLUSDT"]
    assert ready.payload["common_bar_count_after"] > 72


def test_v7_idempotent_rerun_skips_existing_cache(tmp_path: Path) -> None:
    cache_dir = tmp_path / "ohlcv"
    source_v6 = v6_artifact(
        tmp_path,
        requested_symbols=("BTCUSDT", "ETHUSDT"),
        usable_symbols=(),
        missing_by_symbol={"BTCUSDT": ("2024-01",), "ETHUSDT": ("2024-01",)},
    )
    first_downloader = downloader_that_writes_cache(cache_dir)
    first = evaluate_public_multi_asset_data_backfill(
        source_v6_artifact_path=source_v6,
        cache_dir=cache_dir,
        funding_source="none",
        minimum_required_symbols=2,
        ohlcv_downloader=first_downloader,
        market_loader=cache_backed_market_loader(cache_dir),
    )
    assert first.payload["successful_download_count"] == 2

    def fail_if_called(**kwargs: Any) -> tuple[DownloadRecord, ...]:
        raise AssertionError("idempotent rerun must not redownload existing cache")

    second = evaluate_public_multi_asset_data_backfill(
        source_v6_artifact_path=source_v6,
        cache_dir=cache_dir,
        funding_source="none",
        minimum_required_symbols=2,
        ohlcv_downloader=fail_if_called,
        market_loader=cache_backed_market_loader(cache_dir),
    )
    assert second.payload["planned_download_count"] == 0
    assert second.payload["skipped_existing_count"] == 2
    assert second.payload["successful_download_count"] == 0


def test_v7_artifact_writing_filters_raw_changed_files_and_records_provenance(tmp_path: Path) -> None:
    cache_dir = tmp_path / "ohlcv"
    source_v6 = v6_artifact(tmp_path, requested_symbols=("BTCUSDT",), usable_symbols=(), missing_by_symbol={"BTCUSDT": ("2024-01",)})
    result = evaluate_public_multi_asset_data_backfill(
        source_v6_artifact_path=source_v6,
        cache_dir=cache_dir,
        funding_source="none",
        minimum_required_symbols=1,
        plan_only=True,
        market_loader=cache_backed_market_loader(cache_dir),
        changed_files=[
            "src/parangsae/research/public_multi_asset_panel_coverage.py",
            "data/binance-vision/futures/um/monthly/klines/BTCUSDT/1h/BTCUSDT-1h-2024-01.zip",
            "data/binance-vision/futures/um/monthly/klines/BTCUSDT/1h/BTCUSDT-1h-2024-01.zip.metadata.json",
            str(Path.cwd() / "data/binance-funding/futures/um/fundingRate/BTCUSDT/BTCUSDT.json"),
            "tests/data/legitimate-fixture.csv",
            "tests/data/binance-vision/legitimate-fixture.csv",
            "src/parangsae/research/data/reference.py",
            "src/parangsae/research/data/binance-funding/reference.py",
        ],
    )
    out = write_public_multi_asset_data_backfill_artifacts(
        tmp_path / "20260628-public-multi-asset-data-backfill-v7",
        result,
    )
    assert {path.name for path in out.iterdir()} == set(PUBLIC_MULTI_ASSET_DATA_BACKFILL_REQUIRED_FILES)
    assert existing_loop_artifact_type(out) == "public-multi-asset-data-backfill-run"
    assert (out / "backfill_plan.csv").read_text().splitlines()[0] == ",".join(BACKFILL_PLAN_COLUMNS)
    assert (out / "failed_downloads.csv").read_text().splitlines()[0] == ",".join(FAILED_DOWNLOADS_COLUMNS)
    changed = (out / "changed_files.txt").read_text().splitlines()
    assert changed == [
        "src/parangsae/research/public_multi_asset_panel_coverage.py",
        "tests/data/legitimate-fixture.csv",
        "tests/data/binance-vision/legitimate-fixture.csv",
        "src/parangsae/research/data/reference.py",
        "src/parangsae/research/data/binance-funding/reference.py",
    ]
    saved = json.loads((out / "public_multi_asset_data_backfill.json").read_text())
    assert saved["source_v6_artifact_path"] == str(source_v6)
    assert saved["source_v6_payload_hash"]
    assert saved["source_v6_missing_data_plan_hash"]
    assert saved["raw_public_data_committed"] is False
    assert saved["raw_data_files_committed"] is False

    bad_payload = dict(result.payload)
    bad_payload["raw_data_files_committed"] = True
    bad = PublicMultiAssetDataBackfillResult(
        payload=bad_payload,
        backfill_plan=result.backfill_plan,
        download_manifest=result.download_manifest,
        symbol_month_coverage=result.symbol_month_coverage,
        checksum_manifest=result.checksum_manifest,
        failed_downloads=result.failed_downloads,
        panel_readiness_after_backfill=result.panel_readiness_after_backfill,
        missing_data_remaining=result.missing_data_remaining,
    )
    try:
        write_public_multi_asset_data_backfill_artifacts(tmp_path / "bad-v7", bad)
    except ValueError as exc:
        assert "raw public data files" in str(exc)
    else:
        raise AssertionError("truthy raw-data commit flags must be rejected")


def test_v7_symbol_month_coverage_uses_post_backfill_validation_not_file_existence(tmp_path: Path) -> None:
    cache_dir = tmp_path / "ohlcv"
    touch_ohlcv_cache(cache_dir, "BTCUSDT", ("2024-01",))
    source_v6 = v6_artifact(
        tmp_path,
        requested_symbols=("BTCUSDT",),
        usable_symbols=("BTCUSDT",),
        missing_by_symbol={},
    )

    def invalid_market_loader(**kwargs: Any) -> MarketDataBundle:
        raise ValueError("corrupt OHLCV cache")

    result = evaluate_public_multi_asset_data_backfill(
        source_v6_artifact_path=source_v6,
        cache_dir=cache_dir,
        funding_source="none",
        minimum_required_symbols=1,
        market_loader=invalid_market_loader,
    )
    row = next(item for item in result.symbol_month_coverage if item["data_type"] == "ohlcv")
    assert row["ohlcv_cache_present_after"] is True
    assert row["ohlcv_valid_after"] is False
    assert result.payload["panel_ready_for_v5_rerun"] is False
    assert result.payload["suite_decision"] == "BACKFILL_COMPLETED_PANEL_STILL_NOT_READY"


def test_cli_v7_uses_public_no_credential_no_live_path_and_does_not_run_v5(monkeypatch, tmp_path: Path) -> None:
    cache_dir = tmp_path / "ohlcv"
    touch_ohlcv_cache(cache_dir, "BTCUSDT", ("2024-01",))
    source_v6 = v6_artifact(
        tmp_path,
        requested_symbols=("BTCUSDT", "ETHUSDT", "SOLUSDT"),
        usable_symbols=("BTCUSDT",),
        months=("2024-01",),
        missing_by_symbol={"ETHUSDT": ("2024-01",), "SOLUSDT": ("2024-01",)},
    )
    monkeypatch.setattr(quantctl, "download_monthly_klines", downloader_that_writes_cache(cache_dir))
    monkeypatch.setattr(quantctl, "load_market_data_bundle", cache_backed_market_loader(cache_dir))
    monkeypatch.setattr(quantctl, "fetch_funding_rate_history", fake_funding_loader)

    def fail_v5_prefilter(**kwargs: Any) -> object:
        raise AssertionError("V7 backfill CLI must not invoke V5 alpha prefilter")

    monkeypatch.setattr(quantctl, "evaluate_multi_asset_alpha_prefilter", fail_v5_prefilter)
    monkeypatch.setenv("BINANCE_API_KEY", "SHOULD_NOT_BE_READ")
    monkeypatch.setenv("BINANCE_API_SECRET", "SHOULD_NOT_BE_READ")
    real_getenv = os.getenv

    def guarded_getenv(key: str, default: str | None = None) -> str | None:
        if key in {"BINANCE_API_KEY", "BINANCE_API_SECRET"}:
            raise AssertionError(f"V7 must not read {key}")
        return real_getenv(key, default)

    monkeypatch.setattr(os, "getenv", guarded_getenv)
    args = Namespace(
        source_v6_artifact=str(source_v6),
        symbols=None,
        interval=None,
        start_month=None,
        end_month=None,
        cache_dir=str(cache_dir),
        funding_source="none",
        funding_cache_dir=str(tmp_path / "funding"),
        minimum_required_symbols=3,
        plan_only=False,
        force=False,
        reports_root=str(tmp_path / "reports"),
        output_dir=None,
        overwrite=False,
        command=None,
        changed_file=None,
    )
    assert quantctl.cmd_public_multi_asset_data_backfill_run(args) == 0
    report_dirs = list((tmp_path / "reports" / "loops").iterdir())
    payload = json.loads((report_dirs[0] / "public_multi_asset_data_backfill.json").read_text())
    assert payload["target_start_month"] == "2024-01"
    assert payload["target_end_month"] == "2024-01"
    assert payload["requested_symbol_universe"] == ["BTCUSDT", "ETHUSDT", "SOLUSDT"]
    assert payload["panel_ready_for_v5_rerun"] is True
    assert payload["readiness_baseline_verdict"] == "NOT_READY"
    assert payload["no_live_orders_submitted"] is True
    assert payload["no_testnet_orders_submitted"] is True
    assert payload["no_account_data_used"] is True
    assert payload["no_api_key_used"] is True
    assert payload["no_private_order_or_fill_data_used"] is True
    assert payload["tsmom_state_changed"] is False
    assert not (report_dirs[0] / "multi_asset_alpha_factory.json").exists()


def test_v1_to_v7_commands_are_wired_and_defaults_are_stable() -> None:
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
    v7 = parser.parse_args(["public-multi-asset-data-backfill-run"])
    assert v7.func is quantctl.cmd_public_multi_asset_data_backfill_run
    assert v7.symbols is None
    assert v7.interval is None
    assert v7.start_month is None
    assert v7.end_month is None
    assert v7.minimum_required_symbols == 3
