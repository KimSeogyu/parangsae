# pyright: reportAttributeAccessIssue=false
from __future__ import annotations

import csv
import hashlib
import io
import json
import math
import re
import zipfile
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Iterable

import pandas as pd
import requests

CANONICAL_SYMBOL = "BTCUSDT"
CANONICAL_INTERVAL = "1h"
CANONICAL_MONTHS = ("2024-01", "2024-02", "2024-03", "2024-04", "2024-05", "2024-06")
BINANCE_VISION_BASE = "https://data.binance.vision/data/futures/um/monthly/klines"
RAW_COLUMNS = (
    "open_time",
    "open",
    "high",
    "low",
    "close",
    "volume",
    "close_time",
    "quote_asset_volume",
    "number_of_trades",
    "taker_buy_base_asset_volume",
    "taker_buy_quote_asset_volume",
    "ignore",
)
OHLCV_COLUMNS = ("Open", "High", "Low", "Close", "Volume")
_INTERVAL_SECONDS = {"1h": 3600}
_INTERVAL_MILLISECONDS = {key: value * 1000 for key, value in _INTERVAL_SECONDS.items()}
_MONTH_RE = re.compile(r"^\d{4}-\d{2}$")


class BinanceVisionError(RuntimeError):
    """Base error for Binance Vision data failures."""


class DataValidationError(BinanceVisionError):
    """Raised when downloaded or parsed OHLCV data is not suitable for a report."""


@dataclass(frozen=True)
class DownloadRecord:
    url: str
    cache_path: str
    sha256: str
    http_status: int | None
    etag: str | None
    last_modified: str | None
    downloaded_at: str
    from_cache: bool
    bytes: int


@dataclass(frozen=True)
class MarketDataBundle:
    symbol: str
    interval: str
    months: tuple[str, ...]
    frame: pd.DataFrame
    downloads: tuple[DownloadRecord, ...]
    normalized_sha256: str
    quality: dict


def utc_now_iso() -> str:
    return datetime.now(UTC).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def validate_symbol(symbol: str) -> str:
    normalized = symbol.strip().upper()
    if not re.fullmatch(r"[A-Z0-9]{5,20}", normalized):
        raise ValueError(f"invalid Binance symbol: {symbol!r}")
    return normalized


def validate_interval(interval: str) -> str:
    if interval not in _INTERVAL_SECONDS:
        raise ValueError(f"unsupported interval for canonical real-run: {interval!r}")
    return interval


def normalize_months(months: Iterable[str]) -> tuple[str, ...]:
    result = tuple(m.strip() for m in months if m.strip())
    if not result:
        raise ValueError("at least one month is required")
    invalid = [m for m in result if not _MONTH_RE.fullmatch(m)]
    if invalid:
        raise ValueError(f"invalid month(s): {invalid}")
    return result


def canonical_urls(
    symbol: str = CANONICAL_SYMBOL,
    interval: str = CANONICAL_INTERVAL,
    months: Iterable[str] = CANONICAL_MONTHS,
) -> tuple[str, ...]:
    symbol = validate_symbol(symbol)
    interval = validate_interval(interval)
    months = normalize_months(months)
    return tuple(
        f"{BINANCE_VISION_BASE}/{symbol}/{interval}/{symbol}-{interval}-{month}.zip" for month in months
    )


def canonical_cache_path(cache_dir: Path, symbol: str, interval: str, month: str) -> Path:
    return cache_dir / "futures" / "um" / "monthly" / "klines" / symbol / interval / f"{symbol}-{interval}-{month}.zip"


def sha256_bytes(payload: bytes) -> str:
    return hashlib.sha256(payload).hexdigest()


def sha256_file(path: str | Path) -> str:
    h = hashlib.sha256()
    with Path(path).open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def _metadata_path(path: Path) -> Path:
    return path.with_suffix(path.suffix + ".metadata.json")


def _read_cached_zip(path: Path, url: str) -> DownloadRecord:
    payload = path.read_bytes()
    sha = sha256_bytes(payload)
    meta_path = _metadata_path(path)
    metadata = json.loads(meta_path.read_text()) if meta_path.exists() else {}
    return DownloadRecord(
        url=url,
        cache_path=str(path),
        sha256=sha,
        http_status=metadata.get("http_status"),
        etag=metadata.get("etag"),
        last_modified=metadata.get("last_modified"),
        downloaded_at=metadata.get("downloaded_at", utc_now_iso()),
        from_cache=True,
        bytes=len(payload),
    )


def download_monthly_klines(
    *,
    symbol: str = CANONICAL_SYMBOL,
    interval: str = CANONICAL_INTERVAL,
    months: Iterable[str] = CANONICAL_MONTHS,
    cache_dir: str | Path = "data/binance-vision",
    session: requests.Session | None = None,
    timeout: int = 30,
) -> tuple[DownloadRecord, ...]:
    symbol = validate_symbol(symbol)
    interval = validate_interval(interval)
    months = normalize_months(months)
    urls = canonical_urls(symbol, interval, months)
    cache_root = Path(cache_dir)
    client = session or requests.Session()
    records: list[DownloadRecord] = []
    for month, url in zip(months, urls, strict=True):
        path = canonical_cache_path(cache_root, symbol, interval, month)
        if path.exists():
            records.append(_read_cached_zip(path, url))
            continue
        response = client.get(url, timeout=timeout)
        response.raise_for_status()
        payload = response.content
        if not payload:
            raise BinanceVisionError(f"empty Binance Vision response: {url}")
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(payload)
        downloaded_at = utc_now_iso()
        record = DownloadRecord(
            url=url,
            cache_path=str(path),
            sha256=sha256_bytes(payload),
            http_status=response.status_code,
            etag=response.headers.get("ETag"),
            last_modified=response.headers.get("Last-Modified"),
            downloaded_at=downloaded_at,
            from_cache=False,
            bytes=len(payload),
        )
        _metadata_path(path).write_text(json.dumps(record.__dict__, indent=2, sort_keys=True) + "\n")
        records.append(record)
    return tuple(records)


def _csv_bytes_from_zip(path: str | Path) -> bytes:
    with zipfile.ZipFile(path) as zf:
        csv_names = [name for name in zf.namelist() if name.lower().endswith(".csv")]
        if len(csv_names) != 1:
            raise DataValidationError(f"expected exactly one csv in {path}, found {csv_names}")
        return zf.read(csv_names[0])


def parse_monthly_zip(path: str | Path, *, interval: str = CANONICAL_INTERVAL) -> pd.DataFrame:
    interval = validate_interval(interval)
    payload = _csv_bytes_from_zip(path)
    sample = payload[:256].decode("utf-8", errors="replace")
    first_cell = next(csv.reader(io.StringIO(sample)))[0].strip() if sample.strip() else ""
    has_header = first_cell == "open_time"
    raw = pd.read_csv(
        io.BytesIO(payload),
        header=0 if has_header else None,
        names=None if has_header else list(RAW_COLUMNS),
    )
    if has_header:
        raw.columns = [str(c).strip() for c in raw.columns]
    missing = [col for col in RAW_COLUMNS[:7] if col not in raw.columns]
    if missing:
        raise DataValidationError(f"missing required kline columns: {missing}")
    open_time_ms = pd.to_numeric(raw["open_time"], errors="coerce")
    close_time_ms = pd.to_numeric(raw["close_time"], errors="coerce")
    if open_time_ms.isna().any():
        raise DataValidationError("open_time contains missing/non-numeric values")
    if close_time_ms.isna().any():
        raise DataValidationError("close_time contains missing/non-numeric values")
    open_time_ms_int = open_time_ms.astype("int64")
    close_time_ms_int = close_time_ms.astype("int64")
    expected_close_time = open_time_ms_int + _INTERVAL_MILLISECONDS[interval] - 1
    mismatched_close_time = close_time_ms_int != expected_close_time
    if mismatched_close_time.any():
        row_index = int(mismatched_close_time[mismatched_close_time].index[0])
        raise DataValidationError(
            "monthly kline is not a closed bar: "
            f"row {row_index} close_time={int(close_time_ms_int.iloc[row_index])} "
            f"expected={int(expected_close_time.iloc[row_index])}"
        )
    frame = pd.DataFrame(
        {
            "Timestamp": pd.to_datetime(open_time_ms_int.to_numpy(), unit="ms", utc=True),
            "Open": pd.to_numeric(raw["open"], errors="coerce"),
            "High": pd.to_numeric(raw["high"], errors="coerce"),
            "Low": pd.to_numeric(raw["low"], errors="coerce"),
            "Close": pd.to_numeric(raw["close"], errors="coerce"),
            "Volume": pd.to_numeric(raw["volume"], errors="coerce"),
        }
    )
    frame = frame.set_index("Timestamp")
    frame.attrs["closed_bar_only"] = True
    frame.attrs["open_bar_included"] = False
    frame.attrs["close_time_policy"] = (
        f"binance_monthly_close_time_equals_open_time_plus_{interval}_minus_1ms"
    )
    frame.attrs["allow_zero_volume_bars"] = True
    return frame


def load_downloaded_klines(records: Iterable[DownloadRecord], *, interval: str = CANONICAL_INTERVAL) -> pd.DataFrame:
    frames = [parse_monthly_zip(record.cache_path, interval=interval) for record in records]
    if not frames:
        raise DataValidationError("no downloaded kline files to parse")
    frame = pd.concat(frames, axis=0)
    frame.attrs["closed_bar_only"] = all(
        bool(item.attrs.get("closed_bar_only", False)) for item in frames
    )
    frame.attrs["open_bar_included"] = any(
        bool(item.attrs.get("open_bar_included", True)) for item in frames
    )
    frame.attrs["close_time_policy"] = (
        f"binance_monthly_close_time_equals_open_time_plus_{interval}_minus_1ms"
    )
    frame.attrs["allow_zero_volume_bars"] = all(
        bool(item.attrs.get("allow_zero_volume_bars", False)) for item in frames
    )
    validate_ohlcv(frame, interval=interval)
    return frame


def _is_utc_index(index: pd.DatetimeIndex) -> bool:
    return index.tz is not None and str(index.tz) in {"UTC", "datetime.timezone.utc"}


def validate_ohlcv(frame: pd.DataFrame, *, interval: str = CANONICAL_INTERVAL) -> dict:
    interval = validate_interval(interval)
    if not isinstance(frame.index, pd.DatetimeIndex):
        raise DataValidationError("OHLCV index must be a DatetimeIndex")
    if not _is_utc_index(frame.index):
        raise DataValidationError("OHLCV index must be timezone-aware UTC")
    missing_cols = [col for col in OHLCV_COLUMNS if col not in frame.columns]
    if missing_cols:
        raise DataValidationError(f"missing OHLCV columns: {missing_cols}")
    if frame.index.has_duplicates:
        raise DataValidationError("duplicate timestamps are not allowed")
    if not frame.index.is_monotonic_increasing:
        raise DataValidationError("timestamps must be monotonic increasing")
    if len(frame) == 0:
        raise DataValidationError("OHLCV frame is empty")
    expected = pd.Timedelta(seconds=_INTERVAL_SECONDS[interval])
    diffs = frame.index.to_series().diff().dropna()
    bad_diffs = diffs[diffs != expected]
    if not bad_diffs.empty:
        first_bad = bad_diffs.index[0].isoformat()
        raise DataValidationError(f"non-{interval} gap at {first_bad}: {bad_diffs.iloc[0]}")
    values = frame.loc[:, OHLCV_COLUMNS]
    for col in OHLCV_COLUMNS:
        if values[col].isna().any():
            raise DataValidationError(f"{col} contains missing/non-numeric values")
        if not values[col].map(math.isfinite).all():
            raise DataValidationError(f"{col} contains non-finite values")
    for col in ("Open", "High", "Low", "Close"):
        if (values[col] <= 0).any():
            raise DataValidationError(f"{col} must be strictly positive")
    zero_volume_bars = int((values["Volume"] == 0).sum())
    allow_zero_volume_bars = bool(frame.attrs.get("allow_zero_volume_bars", False))
    if (values["Volume"] < 0).any():
        raise DataValidationError("Volume must be non-negative")
    if zero_volume_bars and not allow_zero_volume_bars:
        raise DataValidationError(
            "Volume must be strictly positive unless source-specific zero-volume policy is set"
        )
    if (values["High"] < values[["Open", "Close"]].max(axis=1)).any():
        raise DataValidationError("High must be >= max(Open, Close)")
    if (values["Low"] > values[["Open", "Close"]].min(axis=1)).any():
        raise DataValidationError("Low must be <= min(Open, Close)")
    return {
        "rows": int(len(frame)),
        "first_timestamp": frame.index[0].isoformat().replace("+00:00", "Z"),
        "last_timestamp": frame.index[-1].isoformat().replace("+00:00", "Z"),
        "interval": interval,
        "closed_bar_only": bool(frame.attrs.get("closed_bar_only", False)),
        "open_bar_included": bool(
            frame.attrs.get("open_bar_included", not bool(frame.attrs.get("closed_bar_only", False)))
        ),
        "close_time_policy": frame.attrs.get(
            "close_time_policy",
            "unknown_unverified_close_time",
        ),
        "volume_policy": (
            "source_allows_zero_volume_bars_recorded_as_quality_flag"
            if allow_zero_volume_bars
            else "strictly_positive_volume_required"
        ),
        "zero_volume_bars": zero_volume_bars,
        "zero_volume_bars_allowed": allow_zero_volume_bars,
        "expected_gap_seconds": _INTERVAL_SECONDS[interval],
        "gap_tolerance_seconds": 0,
        "duplicates": 0,
    }


def normalized_ohlcv_csv(frame: pd.DataFrame) -> str:
    validate_ohlcv(frame)
    out = pd.DataFrame(
        {
            "Timestamp": frame.index.strftime("%Y-%m-%dT%H:%M:%SZ"),
            "Open": frame["Open"].map(lambda v: f"{float(v):.10f}"),
            "High": frame["High"].map(lambda v: f"{float(v):.10f}"),
            "Low": frame["Low"].map(lambda v: f"{float(v):.10f}"),
            "Close": frame["Close"].map(lambda v: f"{float(v):.10f}"),
            "Volume": frame["Volume"].map(lambda v: f"{float(v):.10f}"),
        }
    )
    return out.to_csv(index=False, lineterminator="\n")


def normalized_ohlcv_sha256(frame: pd.DataFrame) -> str:
    return hashlib.sha256(normalized_ohlcv_csv(frame).encode("utf-8")).hexdigest()


def load_market_data_bundle(
    *,
    symbol: str = CANONICAL_SYMBOL,
    interval: str = CANONICAL_INTERVAL,
    months: Iterable[str] = CANONICAL_MONTHS,
    cache_dir: str | Path = "data/binance-vision",
    session: requests.Session | None = None,
) -> MarketDataBundle:
    symbol = validate_symbol(symbol)
    interval = validate_interval(interval)
    months = normalize_months(months)
    records = download_monthly_klines(
        symbol=symbol,
        interval=interval,
        months=months,
        cache_dir=cache_dir,
        session=session,
    )
    frame = load_downloaded_klines(records, interval=interval)
    quality = validate_ohlcv(frame, interval=interval)
    return MarketDataBundle(
        symbol=symbol,
        interval=interval,
        months=months,
        frame=frame,
        downloads=records,
        normalized_sha256=normalized_ohlcv_sha256(frame),
        quality=quality,
    )
