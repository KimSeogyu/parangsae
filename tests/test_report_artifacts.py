from __future__ import annotations


import pandas as pd
import pytest

from packages.marketdata.binance_vision import DownloadRecord, MarketDataBundle, normalized_ohlcv_sha256, validate_ohlcv
from packages.reporting.real_run_report import FAILURE_FILES, SUCCESS_FILES, build_manifest, prepare_output_dir, write_failure_report, write_success_report
from packages.research.backtest_engine import BacktestResult, assert_meaningful_run


def frame(rows: int = 4) -> pd.DataFrame:
    idx = pd.date_range("2024-01-01", periods=rows, freq="1h", tz="UTC")
    return pd.DataFrame({"Open": 100.0, "High": 101.0, "Low": 99.0, "Close": 100.5, "Volume": 10.0}, index=idx)


def bundle() -> MarketDataBundle:
    f = frame()
    record = DownloadRecord("https://data.binance.vision/data/futures/um/monthly/klines/BTCUSDT/1h/BTCUSDT-1h-2024-01.zip", "cache.zip", "a" * 64, 200, "etag", "last", "2026-01-01T00:00:00Z", False, 123)
    return MarketDataBundle("BTCUSDT", "1h", ("2024-01",), f, (record,), normalized_ohlcv_sha256(f), validate_ohlcv(f))


def result(label: str) -> BacktestResult:
    trades = pd.DataFrame({"EntryTime": ["2024-01-01T00:00:00Z"], "ExitTime": ["2024-01-01T01:00:00Z"], "PnL": [1.0]})
    equity = pd.DataFrame({"Equity": [10000.0, 10001.0], "DrawdownPct": [0.0, 0.0]}, index=pd.date_range("2024-01-01", periods=2, freq="1h", tz="UTC"))
    return BacktestResult(label, {"label": label, "closed_trades": 3 if label == "backtest" else 2, "Return_pct": 1.0, "Equity_Final_usd": 10001.0, "Max_Drawdown_pct": 0.0}, trades, equity)


def test_success_and_failure_artifact_contracts(tmp_path):
    b = bundle()
    manifest = build_manifest(bundle=b, symbol="BTCUSDT", interval="1h", months=("2024-01",), split_index=2, project_root=".")
    out = prepare_output_dir(output_dir=tmp_path / "success")
    meaning = assert_meaningful_run(total_bars=720, train_bars=480, holdout_bars=240, backtest_closed_trades=3, holdout_closed_trades=2)
    write_success_report(output_dir=out, manifest=manifest, meaning=meaning, backtest=result("backtest"), holdout=result("holdout_paper"))
    assert {p.name for p in out.iterdir()} == SUCCESS_FILES
    assert "backtesting.py" not in (out / "report.md").read_text()  # report is output, ADR stays in docs
    failed = prepare_output_dir(output_dir=tmp_path / "failed")
    write_failure_report(output_dir=failed, manifest=manifest, failed_gate="backtest_closed_trades", reason="too few", observed={"backtest_closed_trades": 1})
    assert {p.name for p in failed.iterdir()} == FAILURE_FILES
    assert not (failed / "report.md").exists()


def test_explicit_output_dir_refuses_overwrite_without_flag(tmp_path):
    out = prepare_output_dir(output_dir=tmp_path / "collision")
    assert out.exists()
    with pytest.raises(FileExistsError):
        prepare_output_dir(output_dir=out)
    (out / "report.md").write_text("old")
    recreated = prepare_output_dir(output_dir=out, overwrite=True)
    assert recreated.exists()
    assert not (recreated / "report.md").exists()


def test_overwrite_refuses_to_delete_non_report_files(tmp_path):
    out = prepare_output_dir(output_dir=tmp_path / "danger")
    keep = out / "important.txt"
    keep.write_text("do not delete")
    with pytest.raises(FileExistsError, match="non-report files"):
        prepare_output_dir(output_dir=out, overwrite=True)
    assert keep.read_text() == "do not delete"
