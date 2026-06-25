from __future__ import annotations

import json

import pandas as pd

from packages.marketdata.binance_funding import (
    BINANCE_USDM_FUNDING_RATE_URL,
    FundingDataValidationError,
    fetch_funding_rate_history,
    funding_metadata_for_report,
    normalized_funding_sha256,
)


class FakeFundingResponse:
    status_code = 200

    def __init__(self, rows):
        self._rows = rows

    def raise_for_status(self):
        return None

    def json(self):
        return self._rows


def funding_rows() -> list[dict[str, str | int]]:
    base = pd.Timestamp("2024-01-01T00:00:00Z")
    return [
        {
            "symbol": "BTCUSDT",
            "fundingRate": "0.00010000" if i % 2 == 0 else "0.00020000",
            "fundingTime": int((base + pd.Timedelta(hours=8 * i)).timestamp() * 1000),
            "markPrice": "43000.0",
        }
        for i in range(4)
    ]


def test_fetch_funding_history_uses_public_no_key_endpoint(monkeypatch, tmp_path) -> None:
    calls = []

    def fake_request(self, method, url, **kwargs):
        calls.append((method, url, kwargs))
        assert method == "GET"
        assert url == BINANCE_USDM_FUNDING_RATE_URL
        assert kwargs["params"] == {
            "symbol": "BTCUSDT",
            "startTime": 1704067200000,
            "endTime": 1704153600000,
            "limit": 1000,
        }
        return FakeFundingResponse(funding_rows())

    monkeypatch.setattr("requests.sessions.Session.request", fake_request)
    bundle = fetch_funding_rate_history(
        symbol="BTCUSDT",
        start_time="2024-01-01T00:00:00Z",
        end_time="2024-01-02T00:00:00Z",
        cache_dir=tmp_path,
    )
    assert len(calls) == 1
    assert list(bundle.frame.columns) == ["symbol", "funding_time_ms", "funding_rate", "mark_price"]
    assert bundle.quality["rows"] == 4
    assert bundle.quality["median_funding_rate"] == 0.00015000000000000001
    assert bundle.normalized_sha256 == normalized_funding_sha256(bundle.frame)
    metadata = funding_metadata_for_report(bundle)
    assert metadata["source"] == "public_binance_usdm_funding_history"
    assert metadata["quality"]["symbol"] == "BTCUSDT"
    assert "downloads" in metadata

    cache_files = list(tmp_path.rglob("*.json"))
    assert cache_files
    assert json.loads(cache_files[0].read_text())


def test_invalid_funding_response_does_not_poison_cache(monkeypatch, tmp_path) -> None:
    def fake_request(self, method, url, **kwargs):
        assert url == BINANCE_USDM_FUNDING_RATE_URL
        return FakeFundingResponse(
            [
                {
                    "symbol": "BTCUSDT",
                    "fundingTime": 1704067200000,
                    # missing fundingRate must fail validation before cache write
                    "markPrice": "43000.0",
                }
            ]
        )

    monkeypatch.setattr("requests.sessions.Session.request", fake_request)
    try:
        fetch_funding_rate_history(
            symbol="BTCUSDT",
            start_time="2024-01-01T00:00:00Z",
            end_time="2024-01-02T00:00:00Z",
            cache_dir=tmp_path,
        )
    except FundingDataValidationError:
        pass
    else:  # pragma: no cover - defensive assertion path
        raise AssertionError("invalid funding rows must raise")
    assert not list(tmp_path.rglob("*.json"))
    assert not list(tmp_path.rglob("*.tmp"))
