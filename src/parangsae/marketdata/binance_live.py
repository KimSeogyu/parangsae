# pyright: reportAttributeAccessIssue=false
from __future__ import annotations

import hashlib
import json
from dataclasses import dataclass
from datetime import UTC, datetime
from typing import Any

import pandas as pd
import requests

from parangsae.marketdata.binance_vision import (
    CANONICAL_INTERVAL,
    CANONICAL_SYMBOL,
    DataValidationError,
    normalized_ohlcv_sha256,
    validate_interval,
    validate_ohlcv,
    validate_symbol,
)

BINANCE_USDM_KLINES_URL = "https://fapi.binance.com/fapi/v1/klines"
BINANCE_USDM_KLINES_DOCS = (
    "https://developers.binance.com/docs/derivatives/usds-margined-futures/"
    "market-data/rest-api/Kline-Candlestick-Data"
)


class BinanceLiveKlineError(RuntimeError):
    """Base error for public Binance recent-kline failures."""


class LiveKlineValidationError(BinanceLiveKlineError):
    """Raised when public recent klines cannot be normalized for research use."""


@dataclass(frozen=True)
class LiveKlineDownloadRecord:
    url: str
    params: dict[str, Any]
    http_status: int | None
    downloaded_at: str
    requested_output_rows: int
    exchange_request_limit: int
    rows_received: int
    rows_after_closed_bar_filter: int
    closed_bar_only: bool
    include_open_bar: bool
    response_sha256: str
    source_docs_url: str = BINANCE_USDM_KLINES_DOCS


@dataclass(frozen=True)
class LiveKlineBundle:
    symbol: str
    interval: str
    frame: pd.DataFrame
    download: LiveKlineDownloadRecord
    normalized_sha256: str
    quality: dict[str, Any]
    source_url: str = BINANCE_USDM_KLINES_URL
    docs_url: str = BINANCE_USDM_KLINES_DOCS


def utc_now_iso() -> str:
    return datetime.now(UTC).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def fetch_recent_klines(
    *,
    symbol: str = CANONICAL_SYMBOL,
    interval: str = CANONICAL_INTERVAL,
    limit: int = 720,
    include_open_bar: bool = False,
    session: requests.Session | None = None,
    timeout: int = 30,
    observed_at: pd.Timestamp | str | None = None,
) -> LiveKlineBundle:
    normalized_symbol = validate_symbol(symbol)
    normalized_interval = validate_interval(interval)
    output_limit = _validate_output_limit(limit=limit, include_open_bar=include_open_bar)
    exchange_limit = output_limit + 1 if not include_open_bar else output_limit
    observed_ts = _utc_timestamp(observed_at) if observed_at is not None else pd.Timestamp.now("UTC")
    params = {"symbol": normalized_symbol, "interval": normalized_interval, "limit": exchange_limit}
    client = session or requests.Session()
    response = client.get(BINANCE_USDM_KLINES_URL, params=params, timeout=timeout)
    response.raise_for_status()
    rows = response.json()
    if not isinstance(rows, list):
        raise LiveKlineValidationError("kline response must be a JSON list")
    payload = json.dumps(rows, sort_keys=True, separators=(",", ":")).encode("utf-8")
    raw = normalize_kline_rows(
        rows,
        include_open_bar=include_open_bar,
        observed_at=observed_ts,
        interval=normalized_interval,
    )
    if len(raw) < output_limit:
        raise LiveKlineValidationError(
            f"expected at least {output_limit} kline rows after filtering, got {len(raw)}"
        )
    if len(raw) > output_limit:
        raw = raw.tail(output_limit)
    quality = validate_ohlcv(raw, interval=normalized_interval)
    quality.update(
        {
            "source": "public_binance_usdm_recent_klines",
            "source_url": BINANCE_USDM_KLINES_URL,
            "source_docs_url": BINANCE_USDM_KLINES_DOCS,
            "closed_bar_only": not include_open_bar,
            "include_open_bar": bool(include_open_bar),
            "observed_at": _timestamp(observed_ts),
            "requested_output_rows": int(output_limit),
            "exchange_request_limit": int(exchange_limit),
            "rows_received": int(len(rows)),
            "rows_after_closed_bar_filter": int(len(raw)),
        }
    )
    record = LiveKlineDownloadRecord(
        url=BINANCE_USDM_KLINES_URL,
        params=params,
        http_status=response.status_code,
        downloaded_at=utc_now_iso(),
        requested_output_rows=int(output_limit),
        exchange_request_limit=int(exchange_limit),
        rows_received=int(len(rows)),
        rows_after_closed_bar_filter=int(len(raw)),
        closed_bar_only=not include_open_bar,
        include_open_bar=bool(include_open_bar),
        response_sha256=hashlib.sha256(payload).hexdigest(),
    )
    return LiveKlineBundle(
        symbol=normalized_symbol,
        interval=normalized_interval,
        frame=raw,
        download=record,
        normalized_sha256=normalized_ohlcv_sha256(raw),
        quality=quality,
    )


def _validate_output_limit(*, limit: int, include_open_bar: bool) -> int:
    if limit <= 0 or limit > 1500:
        raise ValueError("kline limit must be between 1 and 1500")
    if not include_open_bar and limit >= 1500:
        raise ValueError(
            "closed-bar kline limit must be below 1500 because one extra exchange row is requested"
        )
    return int(limit)


def normalize_kline_rows(
    rows: list[Any],
    *,
    include_open_bar: bool,
    observed_at: pd.Timestamp | str,
    interval: str = CANONICAL_INTERVAL,
) -> pd.DataFrame:
    validate_interval(interval)
    observed_ts = _utc_timestamp(observed_at)
    if not rows:
        raise LiveKlineValidationError("kline rows are empty")
    records: list[dict[str, Any]] = []
    observed_ms = int(observed_ts.timestamp() * 1000)
    for row in rows:
        if not isinstance(row, list | tuple) or len(row) < 7:
            raise LiveKlineValidationError("each kline row must include Binance open/high/low/close/volume/closeTime fields")
        close_time_ms = int(row[6])
        if not include_open_bar and close_time_ms >= observed_ms:
            continue
        records.append(
            {
                "Timestamp": pd.to_datetime(int(row[0]), unit="ms", utc=True),
                "Open": pd.to_numeric(row[1], errors="coerce"),
                "High": pd.to_numeric(row[2], errors="coerce"),
                "Low": pd.to_numeric(row[3], errors="coerce"),
                "Close": pd.to_numeric(row[4], errors="coerce"),
                "Volume": pd.to_numeric(row[5], errors="coerce"),
            }
        )
    if not records:
        raise LiveKlineValidationError("all kline rows were filtered as open bars")
    frame = pd.DataFrame.from_records(records).set_index("Timestamp")
    try:
        validate_ohlcv(frame, interval=interval)
    except DataValidationError as exc:
        raise LiveKlineValidationError(str(exc)) from exc
    return frame


def live_kline_metadata_for_report(bundle: LiveKlineBundle) -> dict[str, Any]:
    return {
        "source": "public_binance_usdm_recent_klines",
        "source_url": bundle.source_url,
        "source_docs_url": bundle.docs_url,
        "download": bundle.download.__dict__,
        "quality": bundle.quality,
        "normalized_ohlcv_sha256": bundle.normalized_sha256,
    }


def _utc_timestamp(value: pd.Timestamp | str) -> pd.Timestamp:
    timestamp = pd.Timestamp(value)
    if timestamp.tzinfo is None:
        timestamp = timestamp.tz_localize("UTC")
    return timestamp.tz_convert("UTC")


def _timestamp(value: pd.Timestamp) -> str:
    return value.isoformat().replace("+00:00", "Z")
