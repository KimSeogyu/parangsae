from __future__ import annotations

import pandas as pd
import pytest

from parangsae.marketdata.binance_live import (
    BINANCE_USDM_KLINES_URL,
    LiveKlineValidationError,
    fetch_recent_klines,
    normalize_kline_rows,
)


class FakeKlineResponse:
    status_code = 200

    def __init__(self, rows):
        self._rows = rows

    def raise_for_status(self):
        return None

    def json(self):
        return self._rows


def kline_row(timestamp: str, price: float) -> list:
    ts = pd.Timestamp(timestamp, tz="UTC")
    open_ms = int(ts.timestamp() * 1000)
    close_ms = int((ts + pd.Timedelta(hours=1)).timestamp() * 1000) - 1
    return [open_ms, str(price), str(price + 2), str(price - 2), str(price + 1), "10", close_ms, "0", 1, "0", "0", "0"]


def test_fetch_recent_klines_uses_public_no_key_endpoint(monkeypatch) -> None:
    rows = [kline_row("2024-01-01T00:00:00", 100), kline_row("2024-01-01T01:00:00", 101)]
    calls = []

    def fake_request(self, method, url, **kwargs):
        calls.append((method, url, kwargs))
        assert method == "GET"
        assert url == BINANCE_USDM_KLINES_URL
        assert kwargs["params"] == {"symbol": "BTCUSDT", "interval": "1h", "limit": 3}
        assert "headers" not in kwargs
        return FakeKlineResponse(rows)

    monkeypatch.setattr("requests.sessions.Session.request", fake_request)
    bundle = fetch_recent_klines(
        symbol="BTCUSDT",
        interval="1h",
        limit=2,
        observed_at="2024-01-01T03:00:00Z",
    )
    assert len(calls) == 1
    assert list(bundle.frame.columns) == ["Open", "High", "Low", "Close", "Volume"]
    assert bundle.quality["rows"] == 2
    assert bundle.quality["closed_bar_only"] is True
    assert bundle.download.params["symbol"] == "BTCUSDT"
    assert bundle.download.requested_output_rows == 2
    assert bundle.download.exchange_request_limit == 3
    assert bundle.download.source_docs_url.endswith("Kline-Candlestick-Data")
    assert bundle.normalized_sha256


def test_fetch_recent_klines_preserves_requested_closed_row_count(monkeypatch) -> None:
    rows = [
        kline_row("2024-01-01T00:00:00", 100),
        kline_row("2024-01-01T01:00:00", 101),
        kline_row("2024-01-01T02:00:00", 102),
    ]

    def fake_request(self, method, url, **kwargs):
        assert method == "GET"
        assert url == BINANCE_USDM_KLINES_URL
        assert kwargs["params"]["limit"] == 3
        return FakeKlineResponse(rows)

    monkeypatch.setattr("requests.sessions.Session.request", fake_request)
    bundle = fetch_recent_klines(
        symbol="BTCUSDT",
        interval="1h",
        limit=2,
        observed_at="2024-01-01T02:30:00Z",
    )
    assert len(bundle.frame) == 2
    assert bundle.quality["rows"] == 2
    assert bundle.quality["requested_output_rows"] == 2
    assert bundle.quality["exchange_request_limit"] == 3
    assert bundle.frame.index[-1] == pd.Timestamp("2024-01-01T01:00:00Z")


def test_closed_bar_filter_defaults_to_dropping_current_open_bar() -> None:
    rows = [kline_row("2024-01-01T00:00:00", 100), kline_row("2024-01-01T01:00:00", 101)]
    default_frame = normalize_kline_rows(
        rows,
        include_open_bar=False,
        observed_at="2024-01-01T01:30:00Z",
    )
    assert len(default_frame) == 1
    assert default_frame.index[-1] == pd.Timestamp("2024-01-01T00:00:00Z")

    explicit_frame = normalize_kline_rows(
        rows,
        include_open_bar=True,
        observed_at="2024-01-01T01:30:00Z",
    )
    assert len(explicit_frame) == 2


@pytest.mark.parametrize(
    "rows",
    [
        [[1704067200000, "100", "101", "99", "100.5", "10"]],
        [kline_row("2024-01-01T00:00:00", 100), kline_row("2024-01-01T00:00:00", 101)],
        [kline_row("2024-01-01T01:00:00", 101), kline_row("2024-01-01T00:00:00", 100)],
        [[1704067200000, "nan", "101", "99", "100.5", "10", 1704070799999]],
    ],
)
def test_normalize_kline_rows_rejects_malformed_rows(rows) -> None:
    with pytest.raises(LiveKlineValidationError):
        normalize_kline_rows(
            rows,
            include_open_bar=True,
            observed_at="2024-01-01T03:00:00Z",
        )
