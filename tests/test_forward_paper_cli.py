from __future__ import annotations

import json
import os
from argparse import Namespace
from pathlib import Path

import pandas as pd

from parangsae.apps.cli import quantctl
from parangsae.marketdata.binance_funding import BINANCE_USDM_FUNDING_RATE_URL
from parangsae.marketdata.binance_live import BINANCE_USDM_KLINES_URL
from parangsae.research.forward_paper import EQUITY_CSV_COLUMNS, POSITIONS_CSV_COLUMNS, TRADES_COLUMNS


class FakeResponse:
    status_code = 200
    headers = {}

    def __init__(self, json_data):
        self._json_data = json_data
        self.content = b""

    def raise_for_status(self):
        return None

    def json(self):
        return self._json_data


def kline_rows(rows: int = 180) -> list[list]:
    start = pd.Timestamp("2024-01-01T00:00:00Z")
    out = []
    for i in range(rows):
        ts = start + pd.Timedelta(hours=i)
        open_ms = int(ts.timestamp() * 1000)
        close_ms = int((ts + pd.Timedelta(hours=1)).timestamp() * 1000) - 1
        price = 100 + i * 0.2 + (i % 12) * 0.1
        out.append(
            [
                open_ms,
                f"{price:.4f}",
                f"{price + 2:.4f}",
                f"{price - 2:.4f}",
                f"{price + 0.5:.4f}",
                "10",
                close_ms,
                "0",
                1,
                "0",
                "0",
                "0",
            ]
        )
    return out


def funding_rows(rows: int = 24) -> list[dict[str, str | int]]:
    start = pd.Timestamp("2024-01-01T00:00:00Z")
    return [
        {
            "symbol": "BTCUSDT",
            "fundingRate": "0.00010000" if i % 2 == 0 else "-0.00005000",
            "fundingTime": int((start + pd.Timedelta(hours=8 * i)).timestamp() * 1000),
            "markPrice": "43000.0",
        }
        for i in range(rows)
    ]


def args(tmp_path: Path) -> Namespace:
    return Namespace(
        symbol="BTCUSDT",
        interval="1h",
        lookback_bars=180,
        initial_cash=10000.0,
        fee_rate=0.0004,
        slippage_bps=5.0,
        entry_threshold=0.20,
        exit_threshold=0.05,
        max_exposure=1.0,
        funding_source="public-binance",
        funding_cache_dir=str(tmp_path / "funding-cache"),
        include_open_bar=False,
        reports_root=str(tmp_path / "reports"),
        output_dir=None,
        overwrite=False,
    )


def test_forward_paper_run_writes_public_data_artifacts(monkeypatch, tmp_path) -> None:
    calls = []

    def fake_request(self, method, url, **kwargs):
        calls.append((method, url, kwargs))
        assert method == "GET"
        if url == BINANCE_USDM_KLINES_URL:
            assert kwargs["params"] == {"symbol": "BTCUSDT", "interval": "1h", "limit": 181}
            return FakeResponse(kline_rows())
        if url == BINANCE_USDM_FUNDING_RATE_URL:
            assert kwargs["params"]["symbol"] == "BTCUSDT"
            return FakeResponse(funding_rows())
        raise AssertionError(url)

    monkeypatch.setattr("requests.sessions.Session.request", fake_request)
    rc = quantctl.cmd_forward_paper_run(args(tmp_path))
    assert rc == 0
    assert [call[1] for call in calls] == [BINANCE_USDM_KLINES_URL, BINANCE_USDM_FUNDING_RATE_URL]
    report_dirs = list((tmp_path / "reports").iterdir())
    assert len(report_dirs) == 1
    out = report_dirs[0]
    assert out.name.startswith("forward-paper-run-")
    assert {p.name for p in out.iterdir()} == {
        "manifest.json",
        "metrics.json",
        "positions.csv",
        "equity.csv",
        "trades.csv",
        "regime.csv",
        "regime.json",
        "signals.csv",
        "signals.json",
        "report.md",
    }
    manifest = json.loads((out / "manifest.json").read_text())
    metrics = json.loads((out / "metrics.json").read_text())
    assert manifest["run_type"] == "forward-paper-run"
    assert manifest["live_orders_submitted"] is False
    assert manifest["api_keys_required"] is False
    assert manifest["kline_request"]["url"] == BINANCE_USDM_KLINES_URL
    assert manifest["row_count"] == 180
    assert manifest["kline_request"]["params"]["limit"] == 181
    assert manifest["kline_request"]["requested_output_rows"] == 180
    assert manifest["kline_request"]["exchange_request_limit"] == 181
    assert manifest["kline_request"]["rows_after_closed_bar_filter"] == 180
    assert manifest["kline_request"]["closed_bar_only"] is True
    assert manifest["kline_request"]["docs_url"].endswith("Kline-Candlestick-Data")
    assert manifest["normalized_ohlcv_sha256"]
    assert manifest["kline_request"]["normalized_ohlcv_sha256"] == manifest["normalized_ohlcv_sha256"]
    assert metrics["live_orders_submitted"] is False
    assert metrics["api_keys_required"] is False
    assert list(pd.read_csv(out / "positions.csv").columns) == POSITIONS_CSV_COLUMNS
    assert list(pd.read_csv(out / "equity.csv").columns) == EQUITY_CSV_COLUMNS
    assert list(pd.read_csv(out / "trades.csv").columns) == TRADES_COLUMNS
    report = (out / "report.md").read_text()
    assert "not live-fill proof" in report
    assert "not a profit guarantee" in report


def test_forward_paper_run_does_not_read_credentials(monkeypatch, tmp_path) -> None:
    monkeypatch.setenv("BINANCE_API_KEY", "SHOULD_NOT_BE_READ")
    monkeypatch.setenv("BINANCE_API_SECRET", "SHOULD_NOT_BE_READ")
    real_getenv = os.getenv

    def guarded_getenv(key, default=None):
        if key in {"BINANCE_API_KEY", "BINANCE_API_SECRET"}:
            raise AssertionError(f"forward-paper-run must not read {key}")
        return real_getenv(key, default)

    def fake_request(self, method, url, **kwargs):
        if url == BINANCE_USDM_KLINES_URL:
            return FakeResponse(kline_rows())
        if url == BINANCE_USDM_FUNDING_RATE_URL:
            return FakeResponse(funding_rows())
        raise AssertionError(url)

    monkeypatch.setattr(os, "getenv", guarded_getenv)
    monkeypatch.setattr("requests.sessions.Session.request", fake_request)
    assert quantctl.cmd_forward_paper_run(args(tmp_path)) == 0


def test_forward_paper_public_funding_unavailable_fails_but_explicit_none_succeeds(monkeypatch, tmp_path) -> None:
    def empty_funding_request(self, method, url, **kwargs):
        if url == BINANCE_USDM_KLINES_URL:
            return FakeResponse(kline_rows())
        if url == BINANCE_USDM_FUNDING_RATE_URL:
            return FakeResponse([])
        raise AssertionError(url)

    monkeypatch.setattr("requests.sessions.Session.request", empty_funding_request)
    public_args = args(tmp_path)
    assert quantctl.cmd_forward_paper_run(public_args) == 2
    first = next((tmp_path / "reports").iterdir())
    assert (first / "failure.json").exists()
    assert "no public Binance funding rows" in (first / "failure.json").read_text()

    def kline_only_request(self, method, url, **kwargs):
        assert url == BINANCE_USDM_KLINES_URL
        return FakeResponse(kline_rows())

    monkeypatch.setattr("requests.sessions.Session.request", kline_only_request)
    none_args = args(tmp_path)
    none_args.funding_source = "none"
    none_args.reports_root = str(tmp_path / "reports-none")
    assert quantctl.cmd_forward_paper_run(none_args) == 0
    out = next((tmp_path / "reports-none").iterdir())
    metrics = json.loads((out / "metrics.json").read_text())
    assert metrics["funding_source"] == "none"
    assert metrics["funding_rows"] == 0
    assert "funding disabled" in (out / "report.md").read_text()


def test_forward_paper_static_source_has_no_order_or_credential_path() -> None:
    source = Path("src/parangsae/apps/cli/quantctl.py").read_text()
    command_source = source[source.index("def cmd_forward_paper_run") : source.index("def cmd_kill_switch")]
    scanned = {
        "src/parangsae/apps/cli/quantctl.py::cmd_forward_paper_run": command_source,
        "src/parangsae/marketdata/binance_live.py": Path("src/parangsae/marketdata/binance_live.py").read_text(),
        "src/parangsae/research/forward_paper.py": Path("src/parangsae/research/forward_paper.py").read_text(),
    }
    banned = [
        "ccxt",
        "create_order",
        "/fapi/v1/order",
        "target_from_signals",
        "intents_from_target",
        "BinanceFuturesBroker",
        "BinanceSpotBroker",
        "RecordingHttpClient",
        "BINANCE_API_KEY",
        "BINANCE_API_SECRET",
        "apiKey",
        "secret",
        "demo-fapi",
        "testnet",
    ]
    for name, text in scanned.items():
        for needle in banned:
            assert needle not in text, f"{needle} leaked into {name}"
