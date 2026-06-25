# pyright: reportAttributeAccessIssue=false
from __future__ import annotations

import hashlib
import json
import math
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any

import pandas as pd
import requests

from packages.marketdata.binance_vision import validate_symbol

BINANCE_USDM_FUNDING_RATE_URL = "https://fapi.binance.com/fapi/v1/fundingRate"
BINANCE_USDM_FUNDING_RATE_DOCS = (
    "https://developers.binance.com/docs/derivatives/usds-margined-futures/"
    "market-data/rest-api/Get-Funding-Rate-History"
)
FUNDING_COLUMNS = ("symbol", "funding_time_ms", "funding_rate", "mark_price")


class BinanceFundingError(RuntimeError):
    """Base error for public Binance funding-history failures."""


class FundingDataValidationError(BinanceFundingError):
    """Raised when public funding history cannot be normalized for research use."""


@dataclass(frozen=True)
class FundingDownloadRecord:
    url: str
    cache_path: str
    sha256: str
    http_status: int | None
    downloaded_at: str
    from_cache: bool
    rows: int
    params: dict[str, Any]


@dataclass(frozen=True)
class FundingRateBundle:
    symbol: str
    frame: pd.DataFrame
    downloads: tuple[FundingDownloadRecord, ...]
    normalized_sha256: str
    quality: dict[str, Any]
    source_url: str = BINANCE_USDM_FUNDING_RATE_URL
    docs_url: str = BINANCE_USDM_FUNDING_RATE_DOCS


def funding_cache_path(cache_dir: str | Path, symbol: str, start_ms: int, end_ms: int) -> Path:
    normalized = validate_symbol(symbol)
    return Path(cache_dir) / "futures" / "um" / "fundingRate" / normalized / f"{normalized}-{start_ms}-{end_ms}.json"


def fetch_funding_rate_history(
    *,
    symbol: str,
    start_time: pd.Timestamp | str,
    end_time: pd.Timestamp | str,
    cache_dir: str | Path = "data/binance-funding",
    session: requests.Session | None = None,
    timeout: int = 30,
    limit: int = 1000,
) -> FundingRateBundle:
    """Fetch public, no-key Binance USDⓈ-M funding history and normalize it.

    Binance's public endpoint returns rows in ascending order and accepts
    inclusive millisecond `startTime` / `endTime` bounds. The canonical
    readiness window is below the endpoint max page size, but pagination is
    implemented so the function remains deterministic for wider research
    windows.
    """

    normalized_symbol = validate_symbol(symbol)
    start_ms = _timestamp_ms(start_time)
    end_ms = _timestamp_ms(end_time)
    if end_ms < start_ms:
        raise ValueError("funding end_time must be greater than or equal to start_time")
    if limit <= 0 or limit > 1000:
        raise ValueError("funding limit must be between 1 and 1000")

    path = funding_cache_path(cache_dir, normalized_symbol, start_ms, end_ms)
    if path.exists():
        payload = path.read_bytes()
        rows = json.loads(payload.decode("utf-8"))
        record = FundingDownloadRecord(
            url=BINANCE_USDM_FUNDING_RATE_URL,
            cache_path=str(path),
            sha256=_sha256_bytes(payload),
            http_status=None,
            downloaded_at=_metadata(path).get("downloaded_at", ""),
            from_cache=True,
            rows=len(rows),
            params={"symbol": normalized_symbol, "startTime": start_ms, "endTime": end_ms, "limit": limit},
        )
        frame = normalize_funding_rows(rows, symbol=normalized_symbol)
        return FundingRateBundle(
            symbol=normalized_symbol,
            frame=frame,
            downloads=(record,),
            normalized_sha256=normalized_funding_sha256(frame),
            quality=validate_funding_frame(frame, symbol=normalized_symbol),
        )

    client = session or requests.Session()
    cursor = start_ms
    rows: list[dict[str, Any]] = []
    http_status: int | None = None
    while cursor <= end_ms:
        params = {"symbol": normalized_symbol, "startTime": cursor, "endTime": end_ms, "limit": limit}
        response = client.get(BINANCE_USDM_FUNDING_RATE_URL, params=params, timeout=timeout)
        response.raise_for_status()
        http_status = response.status_code
        page = response.json()
        if not isinstance(page, list):
            raise FundingDataValidationError("funding response must be a JSON list")
        if not page:
            break
        rows.extend(page)
        last_ms = max(int(item["fundingTime"]) for item in page if "fundingTime" in item)
        if len(page) < limit or last_ms >= end_ms:
            break
        cursor = last_ms + 1

    if not rows:
        raise FundingDataValidationError(
            f"no public Binance funding rows for {normalized_symbol} between {start_ms} and {end_ms}"
        )
    frame = normalize_funding_rows(rows, symbol=normalized_symbol)
    quality = validate_funding_frame(frame, symbol=normalized_symbol)
    payload = json.dumps(rows, sort_keys=True, separators=(",", ":")).encode("utf-8")
    path.parent.mkdir(parents=True, exist_ok=True)
    metadata = {
        "downloaded_at": pd.Timestamp.now("UTC").floor("s").isoformat().replace("+00:00", "Z"),
        "docs_url": BINANCE_USDM_FUNDING_RATE_DOCS,
        "source_url": BINANCE_USDM_FUNDING_RATE_URL,
        "http_status": http_status,
        "rows": len(rows),
    }
    _atomic_write_bytes(path, payload)
    _atomic_write_text(path.with_suffix(path.suffix + ".metadata.json"), json.dumps(metadata, indent=2, sort_keys=True) + "\n")
    record = FundingDownloadRecord(
        url=BINANCE_USDM_FUNDING_RATE_URL,
        cache_path=str(path),
        sha256=_sha256_bytes(payload),
        http_status=http_status,
        downloaded_at=metadata["downloaded_at"],
        from_cache=False,
        rows=len(rows),
        params={"symbol": normalized_symbol, "startTime": start_ms, "endTime": end_ms, "limit": limit},
    )
    return FundingRateBundle(
        symbol=normalized_symbol,
        frame=frame,
        downloads=(record,),
        normalized_sha256=normalized_funding_sha256(frame),
        quality=quality,
    )


def normalize_funding_rows(rows: list[dict[str, Any]], *, symbol: str) -> pd.DataFrame:
    normalized_symbol = validate_symbol(symbol)
    if not rows:
        raise FundingDataValidationError("funding rows are empty")
    raw = pd.DataFrame(rows)
    missing = [column for column in ("symbol", "fundingTime", "fundingRate") if column not in raw.columns]
    if missing:
        raise FundingDataValidationError(f"missing funding columns: {missing}")
    frame = pd.DataFrame(
        {
            "symbol": raw["symbol"].astype(str).str.upper(),
            "funding_time_ms": pd.to_numeric(raw["fundingTime"], errors="coerce").astype("Int64"),
            "funding_rate": pd.to_numeric(raw["fundingRate"], errors="coerce"),
            "mark_price": pd.to_numeric(raw.get("markPrice"), errors="coerce") if "markPrice" in raw else pd.NA,
        }
    )
    frame["funding_time"] = pd.to_datetime(frame["funding_time_ms"], unit="ms", utc=True)
    frame = frame.set_index("funding_time").sort_index()
    frame = frame[~frame.index.duplicated(keep="last")]
    if (frame["symbol"] != normalized_symbol).any():
        raise FundingDataValidationError(f"funding response contains non-{normalized_symbol} symbols")
    validate_funding_frame(frame, symbol=normalized_symbol)
    return frame.loc[:, FUNDING_COLUMNS]


def validate_funding_frame(frame: pd.DataFrame, *, symbol: str) -> dict[str, Any]:
    normalized_symbol = validate_symbol(symbol)
    if not isinstance(frame.index, pd.DatetimeIndex):
        raise FundingDataValidationError("funding index must be a DatetimeIndex")
    if frame.index.tz is None:
        raise FundingDataValidationError("funding index must be timezone-aware UTC")
    if not frame.index.is_monotonic_increasing:
        raise FundingDataValidationError("funding timestamps must be monotonic increasing")
    missing = [column for column in FUNDING_COLUMNS if column not in frame.columns]
    if missing:
        raise FundingDataValidationError(f"missing normalized funding columns: {missing}")
    if frame.empty:
        raise FundingDataValidationError("funding frame is empty")
    if (frame["symbol"].astype(str).str.upper() != normalized_symbol).any():
        raise FundingDataValidationError(f"funding frame contains non-{normalized_symbol} symbols")
    rates = pd.to_numeric(frame["funding_rate"], errors="coerce")
    if rates.isna().any() or not rates.map(math.isfinite).all():
        raise FundingDataValidationError("funding_rate contains missing/non-finite values")
    return {
        "rows": int(len(frame)),
        "first_timestamp": _timestamp(frame.index[0]),
        "last_timestamp": _timestamp(frame.index[-1]),
        "symbol": normalized_symbol,
        "min_funding_rate": float(rates.min()),
        "max_funding_rate": float(rates.max()),
        "mean_funding_rate": float(rates.mean()),
        "median_funding_rate": float(rates.median()),
    }


def normalized_funding_csv(frame: pd.DataFrame) -> str:
    validate_funding_frame(frame, symbol=str(frame["symbol"].iloc[0]))
    out = pd.DataFrame(
        {
            "funding_time": frame.index.strftime("%Y-%m-%dT%H:%M:%SZ"),
            "symbol": frame["symbol"].astype(str),
            "funding_time_ms": frame["funding_time_ms"].astype("int64"),
            "funding_rate": frame["funding_rate"].map(lambda value: f"{float(value):.10f}"),
            "mark_price": pd.to_numeric(frame["mark_price"], errors="coerce").map(
                lambda value: "" if pd.isna(value) else f"{float(value):.10f}"
            ),
        }
    )
    return out.to_csv(index=False, lineterminator="\n")


def normalized_funding_sha256(frame: pd.DataFrame) -> str:
    return hashlib.sha256(normalized_funding_csv(frame).encode("utf-8")).hexdigest()


def funding_metadata_for_report(bundle: FundingRateBundle) -> dict[str, Any]:
    return {
        "source": "public_binance_usdm_funding_history",
        "source_url": bundle.source_url,
        "docs_url": bundle.docs_url,
        "normalized_funding_sha256": bundle.normalized_sha256,
        "quality": bundle.quality,
        "downloads": [asdict(record) for record in bundle.downloads],
    }


def _timestamp_ms(value: pd.Timestamp | str) -> int:
    ts = pd.Timestamp(value)
    if ts.tzinfo is None:
        ts = ts.tz_localize("UTC")
    else:
        ts = ts.tz_convert("UTC")
    return int(ts.timestamp() * 1000)


def _timestamp(value: pd.Timestamp) -> str:
    ts = pd.Timestamp(value)
    if ts.tzinfo is None:
        ts = ts.tz_localize("UTC")
    else:
        ts = ts.tz_convert("UTC")
    return ts.isoformat().replace("+00:00", "Z")


def _sha256_bytes(payload: bytes) -> str:
    return hashlib.sha256(payload).hexdigest()


def _metadata(path: Path) -> dict[str, Any]:
    metadata_path = path.with_suffix(path.suffix + ".metadata.json")
    if not metadata_path.exists():
        return {}
    return json.loads(metadata_path.read_text())


def _atomic_write_bytes(path: Path, payload: bytes) -> None:
    tmp = path.with_suffix(path.suffix + ".tmp")
    tmp.write_bytes(payload)
    tmp.replace(path)


def _atomic_write_text(path: Path, payload: str) -> None:
    tmp = path.with_suffix(path.suffix + ".tmp")
    tmp.write_text(payload)
    tmp.replace(path)
