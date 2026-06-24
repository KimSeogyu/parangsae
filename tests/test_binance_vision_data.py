from __future__ import annotations

import io
import zipfile
from pathlib import Path

import pandas as pd
import pytest

from packages.marketdata.binance_vision import (
    download_monthly_klines,
    CANONICAL_MONTHS,
    DataValidationError,
    canonical_urls,
    parse_monthly_zip,
    normalized_ohlcv_sha256,
    validate_ohlcv,
)


def write_zip(path: Path, rows: list[list[object]], *, header: bool = False) -> Path:
    csv = io.StringIO()
    if header:
        csv.write(
            "open_time,open,high,low,close,volume,close_time,quote_asset_volume,number_of_trades,taker_buy_base_asset_volume,taker_buy_quote_asset_volume,ignore\n"
        )
    for row in rows:
        csv.write(",".join(str(item) for item in row) + "\n")
    with zipfile.ZipFile(path, "w") as zf:
        zf.writestr(path.with_suffix(".csv").name, csv.getvalue())
    return path


def row(open_time: int, open_: float = 100.0, high: float = 101.0, low: float = 99.0, close: float = 100.5, volume: float = 10.0):
    return [open_time, open_, high, low, close, volume, open_time + 3599999, 0, 1, 0, 0, 0]


def test_canonical_urls_are_exact_binance_vision_monthly_futures_urls():
    urls = canonical_urls()
    assert len(urls) == len(CANONICAL_MONTHS)
    assert urls[0] == "https://data.binance.vision/data/futures/um/monthly/klines/BTCUSDT/1h/BTCUSDT-1h-2024-01.zip"
    assert urls[-1].endswith("BTCUSDT-1h-2024-06.zip")


def test_parse_zip_to_utc_ohlcv_and_hash(tmp_path):
    base = 1704067200000
    path = write_zip(tmp_path / "BTCUSDT-1h-2024-01.zip", [row(base), row(base + 3600000)], header=True)
    frame = parse_monthly_zip(path)
    quality = validate_ohlcv(frame)
    assert list(frame.columns) == ["Open", "High", "Low", "Close", "Volume"]
    assert str(frame.index.tz) == "UTC"  # pyright: ignore[reportAttributeAccessIssue]
    assert quality["rows"] == 2
    assert len(normalized_ohlcv_sha256(frame)) == 64


@pytest.mark.parametrize(
    "mutate, message",
    [
        (lambda f: pd.concat([f, f.iloc[[0]]]), "duplicate"),
        (lambda f: f.iloc[::-1], "monotonic"),
        (lambda f: f.drop(f.index[1]), "gap"),
        (lambda f: f.assign(Open=0.0), "strictly positive"),
        (lambda f: f.assign(High=f["Close"] - 1), "High"),
        (lambda f: f.assign(Low=f["Close"] + 1), "Low"),
    ],
)
def test_validate_ohlcv_rejects_malformed_rows(mutate, message):
    idx = pd.date_range("2024-01-01", periods=3, freq="1h", tz="UTC")
    frame = pd.DataFrame(
        {"Open": [100.0, 101.0, 102.0], "High": [101.0, 102.0, 103.0], "Low": [99.0, 100.0, 101.0], "Close": [100.5, 101.5, 102.5], "Volume": [1.0, 1.0, 1.0]},
        index=idx,
    )
    with pytest.raises(DataValidationError, match=message):
        validate_ohlcv(mutate(frame))


class _Response:
    status_code = 200
    headers = {"ETag": "abc", "Last-Modified": "Wed, 01 Jan 2024 00:00:00 GMT"}

    def __init__(self, content: bytes) -> None:
        self.content = content

    def raise_for_status(self) -> None:
        return None


class _Session:
    def __init__(self, content: bytes) -> None:
        self.content = content
        self.calls = 0

    def get(self, url: str, timeout: int = 30):
        self.calls += 1
        return _Response(self.content)


def test_cached_download_preserves_sidecar_http_metadata(tmp_path):
    payload_path = write_zip(tmp_path / "payload.zip", [row(1704067200000)])
    session = _Session(payload_path.read_bytes())
    first = download_monthly_klines(months=("2024-01",), cache_dir=tmp_path / "cache", session=session)  # pyright: ignore[reportArgumentType]
    second = download_monthly_klines(months=("2024-01",), cache_dir=tmp_path / "cache", session=session)  # pyright: ignore[reportArgumentType]
    assert session.calls == 1
    assert first[0].etag == "abc"
    assert second[0].from_cache is True
    assert second[0].etag == "abc"
    assert second[0].last_modified == "Wed, 01 Jan 2024 00:00:00 GMT"
