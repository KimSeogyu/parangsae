//! Read-only parquet tape. Paths follow the collector hive layout.
//! Nothing in this module writes the store.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use arrow_array::{Array, Int64Array, RecordBatch, StringArray, UInt32Array};
use arrow_schema::Schema;
use parangsae_core::{Price, Qty, Ratio, Symbol, UtcMicros};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use sha2::{Digest, Sha256};

use crate::bar::{Bar, FundingStamp, Tape, ONE_MINUTE_US};
use crate::date::UtcDate;

pub const FUNDING_SCHEMA_VERSION: u32 = 3;
const FUNDING_LAG_US: i64 = 60_000_000;
const POISON: &[u8] = b"PANIC_IF_READ";

/// Explicit reader inputs. `overlays` may be empty; there is no default list.
#[derive(Clone, Debug)]
pub struct TapeConfig {
    pub holdout_start: UtcMicros,
    pub overlays: Vec<Overlay>,
    /// Hive root for `parquet/funding-rate`. Klines stay under the load root.
    pub funding_root: PathBuf,
}

/// A resolution file. The sha is checked before any row is parsed.
#[derive(Clone, Debug)]
pub struct Overlay {
    pub dataset: String,
    pub path: PathBuf,
    pub sha256: [u8; 32],
}

/// What the report records for one applied overlay.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppliedOverlay {
    pub dataset: String,
    pub path: String,
    pub sha256: String,
}

const UM_KLINE: &str = "um-kline";
const MARK_KLINE: &str = "mark-price-kline";
const FUNDING: &str = "funding-rate";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TapeError {
    MissingFile { path: String },
    Range,
    Interval { got: i64 },
    FundingInterval { got: i64 },
    NullColumn { name: String },
    UnknownSchema { dataset: String, got: u32 },
    FundingBeforeEvent { funding_time: UtcMicros },
    HoldoutRequested,
    BeforeSampleRequested,
    HoldoutRow { at: UtcMicros },
    BeforeSample { at: UtcMicros },
    OverlaySha { path: String },
    OverlayShape { open_time: UtcMicros },
    SymbolMismatch { got: String },
    MissingColumn { name: String },
    ColumnType { name: String },
    Scale { got: String },
    OffGrid { funding_time: UtcMicros },
    BadPrice { column: String },
    BadQty { column: String },
    Arrow(String),
    Io { path: String, message: String },
}

impl std::fmt::Display for TapeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingFile { path } => write!(f, "missing parquet file {path}"),
            Self::Range => write!(f, "date range is reversed"),
            Self::Interval { got } => write!(f, "interval_us {got} is not 60000000"),
            Self::FundingInterval { got } => write!(f, "funding interval_hours {got} is invalid"),
            Self::NullColumn { name } => write!(f, "null in column {name}"),
            Self::UnknownSchema { dataset, got } => {
                write!(f, "schema_version {got} is not accepted for {dataset}")
            }
            Self::FundingBeforeEvent { funding_time } => {
                write!(f, "funding available is before the event at {funding_time}")
            }
            Self::HoldoutRequested => write!(f, "requested date is in the holdout"),
            Self::BeforeSampleRequested => {
                write!(f, "requested date is before 2020-12-18")
            }
            Self::HoldoutRow { at } => write!(f, "row {at} is in the holdout"),
            Self::BeforeSample { at } => write!(f, "row {at} is before 2020-12-18"),
            Self::OverlaySha { path } => write!(f, "overlay sha256 mismatch for {path}"),
            Self::OverlayShape { open_time } => {
                write!(f, "overlay open {open_time} is not in the stored grid")
            }
            Self::SymbolMismatch { got } => write!(f, "symbol {got} does not match the request"),
            Self::MissingColumn { name } => write!(f, "missing column {name}"),
            Self::ColumnType { name } => write!(f, "column {name} has the wrong type or length"),
            Self::Scale { got } => write!(f, "parquet scale is {got}"),
            Self::OffGrid { funding_time } => {
                write!(f, "funding stamp {funding_time} is off the interval grid")
            }
            Self::BadPrice { column } => write!(f, "negative price in {column}"),
            Self::BadQty { column } => write!(f, "negative quantity in {column}"),
            Self::Arrow(message) => write!(f, "parquet read failed: {message}"),
            Self::Io { path, message } => write!(f, "io error at {path}: {message}"),
        }
    }
}

impl std::error::Error for TapeError {}

impl Tape {
    /// Load inclusive UTC days from `{root}/parquet/<dataset>/symbol=.../date=.../part.parquet`.
    ///
    /// A requested day before 2020-12-18 or at or after `holdout_start` is refused
    /// before any file is opened. A row in an accepted file with `open_time` or
    /// `funding_time` outside that range is refused as it is read.
    pub fn load(
        root: &Path,
        symbol: &Symbol,
        start: UtcDate,
        end_inclusive: UtcDate,
        cfg: &TapeConfig,
    ) -> Result<Self, TapeError> {
        if end_inclusive < start {
            return Err(TapeError::Range);
        }
        let sample = sample_start();
        let mut day = start;
        loop {
            if day.start_us() < sample {
                return Err(TapeError::BeforeSampleRequested);
            }
            if day.start_us() >= cfg.holdout_start {
                return Err(TapeError::HoldoutRequested);
            }
            if day == end_inclusive {
                break;
            }
            day = day.next();
        }
        let mut bars = Vec::new();
        let mut marks = Vec::new();
        let mut funding = Vec::new();
        let mut day = start;
        loop {
            bars.extend(read_klines(
                &part_path(root, UM_KLINE, symbol, day),
                symbol,
                UM_KLINE,
                cfg.holdout_start,
            )?);
            marks.extend(read_klines(
                &part_path(root, MARK_KLINE, symbol, day),
                symbol,
                MARK_KLINE,
                cfg.holdout_start,
            )?);
            funding.extend(read_funding(
                &part_path(&cfg.funding_root, FUNDING, symbol, day),
                symbol,
                cfg.holdout_start,
            )?);
            if day == end_inclusive {
                break;
            }
            day = day.next();
        }
        let mut overlays = Vec::with_capacity(cfg.overlays.len());
        for overlay in &cfg.overlays {
            overlays.push(apply_overlay(&mut bars, &mut marks, overlay, symbol)?);
        }
        Ok(Self {
            bars,
            marks,
            funding,
            overlays,
        })
    }
}

/// Paths [`Tape::load`] will open for this range, including overlays.
///
/// The check does not read the files. A caller can refuse a path that is not pinned
/// before `load` opens anything.
pub fn planned_paths(
    root: &Path,
    symbol: &Symbol,
    start: UtcDate,
    end_inclusive: UtcDate,
    cfg: &TapeConfig,
) -> Result<Vec<PathBuf>, TapeError> {
    if end_inclusive < start {
        return Err(TapeError::Range);
    }
    let sample = sample_start();
    let mut day = start;
    loop {
        if day.start_us() < sample {
            return Err(TapeError::BeforeSampleRequested);
        }
        if day.start_us() >= cfg.holdout_start {
            return Err(TapeError::HoldoutRequested);
        }
        if day == end_inclusive {
            break;
        }
        day = day.next();
    }
    let mut paths = Vec::new();
    let mut day = start;
    loop {
        paths.push(part_path(root, UM_KLINE, symbol, day));
        paths.push(part_path(root, MARK_KLINE, symbol, day));
        paths.push(part_path(&cfg.funding_root, FUNDING, symbol, day));
        if day == end_inclusive {
            break;
        }
        day = day.next();
    }
    for overlay in &cfg.overlays {
        paths.push(overlay.path.clone());
    }
    Ok(paths)
}

/// First in-sample instant. Rows and requested days before this are refused.
pub(crate) fn sample_start() -> UtcMicros {
    UtcDate::parse("2020-12-18")
        .expect("2020-12-18 is a valid date")
        .start_us()
}

fn in_sample(ts: i64, holdout: UtcMicros) -> Result<(), TapeError> {
    if ts < sample_start().micros() {
        return Err(TapeError::BeforeSample {
            at: UtcMicros::from_micros(ts),
        });
    }
    if ts >= holdout.micros() {
        return Err(TapeError::HoldoutRow {
            at: UtcMicros::from_micros(ts),
        });
    }
    Ok(())
}

/// `max(stored available, funding_time + 60s)`. The result must be `>= event_ts`.
pub fn funding_available(
    stored_available: i64,
    funding_time: i64,
    event_ts: i64,
) -> Result<i64, TapeError> {
    let lagged = funding_time
        .checked_add(FUNDING_LAG_US)
        .ok_or(TapeError::FundingBeforeEvent {
            funding_time: UtcMicros::from_micros(funding_time),
        })?;
    let available = stored_available.max(lagged);
    if available < event_ts {
        return Err(TapeError::FundingBeforeEvent {
            funding_time: UtcMicros::from_micros(funding_time),
        });
    }
    Ok(available)
}

fn part_path(root: &Path, dataset: &str, symbol: &Symbol, day: UtcDate) -> PathBuf {
    root.join("parquet")
        .join(dataset)
        .join(format!("symbol={}", symbol.as_str()))
        .join(format!("date={day}"))
        .join("part.parquet")
}

fn read_klines(
    path: &Path,
    symbol: &Symbol,
    dataset: &str,
    holdout: UtcMicros,
) -> Result<Vec<Bar>, TapeError> {
    let batches = read_batches(path)?;
    let symbols = read_string(&batches, "symbol")?;
    let interval = read_i64(&batches, "interval_us")?;
    let open_time = read_i64(&batches, "open_time_us")?;
    let close_time = read_i64(&batches, "close_time_us")?;
    let available = read_i64(&batches, "available_ts_us")?;
    let open = read_i64(&batches, "open_ticks")?;
    let high = read_i64(&batches, "high_ticks")?;
    let low = read_i64(&batches, "low_ticks")?;
    let close = read_i64(&batches, "close_ticks")?;
    let volume = read_i64(&batches, "volume_ticks")?;
    let schema = read_u32(&batches, "schema_version")?;
    let rows = symbols.len();
    let count = read_i64(&batches, "count")?;
    same_len("interval_us", interval.len(), rows)?;
    same_len("open_time_us", open_time.len(), rows)?;
    same_len("close_time_us", close_time.len(), rows)?;
    same_len("available_ts_us", available.len(), rows)?;
    same_len("open_ticks", open.len(), rows)?;
    same_len("high_ticks", high.len(), rows)?;
    same_len("low_ticks", low.len(), rows)?;
    same_len("close_ticks", close.len(), rows)?;
    same_len("volume_ticks", volume.len(), rows)?;
    same_len("schema_version", schema.len(), rows)?;
    same_len("count", count.len(), rows)?;
    let mut bars = Vec::with_capacity(rows);
    for index in 0..rows {
        if symbols[index] != symbol.as_str() {
            return Err(TapeError::SymbolMismatch {
                got: symbols[index].clone(),
            });
        }
        if interval[index] != ONE_MINUTE_US {
            return Err(TapeError::Interval {
                got: interval[index],
            });
        }
        if schema[index] != 1 && schema[index] != 2 {
            return Err(TapeError::UnknownSchema {
                dataset: dataset.to_owned(),
                got: schema[index],
            });
        }
        in_sample(open_time[index], holdout)?;
        bars.push(Bar {
            open_time: UtcMicros::from_micros(open_time[index]),
            close_time: UtcMicros::from_micros(close_time[index]),
            available: UtcMicros::from_micros(available[index]),
            open: price_at(open[index], "open_ticks")?,
            high: price_at(high[index], "high_ticks")?,
            low: price_at(low[index], "low_ticks")?,
            close: price_at(close[index], "close_ticks")?,
            volume: Qty::from_ticks(volume[index]).map_err(|_| TapeError::BadQty {
                column: "volume_ticks".to_owned(),
            })?,
            count: count[index],
        });
    }
    Ok(bars)
}

fn read_funding(
    path: &Path,
    symbol: &Symbol,
    holdout: UtcMicros,
) -> Result<Vec<FundingStamp>, TapeError> {
    let batches = read_batches(path)?;
    let symbols = read_string(&batches, "symbol")?;
    let funding_time = read_i64(&batches, "funding_time_us")?;
    let interval = read_i64(&batches, "interval_hours")?;
    let rate = read_i64(&batches, "rate_ticks")?;
    let available = read_i64(&batches, "available_ts_us")?;
    let schema = read_u32(&batches, "schema_version")?;
    let rows = symbols.len();
    let event = match read_i64(&batches, "event_ts_us") {
        Ok(values) => values,
        Err(TapeError::MissingColumn { .. }) => funding_time.clone(),
        Err(err) => return Err(err),
    };
    same_len("funding_time_us", funding_time.len(), rows)?;
    same_len("interval_hours", interval.len(), rows)?;
    same_len("rate_ticks", rate.len(), rows)?;
    same_len("available_ts_us", available.len(), rows)?;
    same_len("schema_version", schema.len(), rows)?;
    same_len("event_ts_us", event.len(), rows)?;
    let mut stamps = Vec::with_capacity(rows);
    for index in 0..rows {
        if symbols[index] != symbol.as_str() {
            return Err(TapeError::SymbolMismatch {
                got: symbols[index].clone(),
            });
        }
        if schema[index] != FUNDING_SCHEMA_VERSION {
            return Err(TapeError::UnknownSchema {
                dataset: FUNDING.to_owned(),
                got: schema[index],
            });
        }
        in_sample(funding_time[index], holdout)?;
        let visible = funding_available(available[index], funding_time[index], event[index])?;
        stamps.push(FundingStamp {
            funding_time: UtcMicros::from_micros(funding_time[index]),
            available: UtcMicros::from_micros(visible),
            rate: Ratio::from_ticks(rate[index]),
            interval_hours: interval[index],
        });
    }
    Ok(stamps)
}

fn apply_overlay(
    bars: &mut [Bar],
    marks: &mut [Bar],
    overlay: &Overlay,
    symbol: &Symbol,
) -> Result<AppliedOverlay, TapeError> {
    let path = overlay.path.display().to_string();
    let bytes = std::fs::read(&overlay.path).map_err(|err| TapeError::Io {
        path: path.clone(),
        message: err.to_string(),
    })?;
    let got = sha256_bytes(&bytes);
    if got != overlay.sha256 {
        return Err(TapeError::OverlaySha { path });
    }
    let file = File::open(&overlay.path).map_err(|err| TapeError::Io {
        path: path.clone(),
        message: err.to_string(),
    })?;
    let rows = overlay_rows(file, symbol, overlay.dataset.as_str())?;
    let target: &mut [Bar] = match overlay.dataset.as_str() {
        UM_KLINE => bars,
        MARK_KLINE => marks,
        _ => {
            return Err(TapeError::OverlayShape {
                open_time: UtcMicros::from_micros(0),
            })
        }
    };
    let mut pending = rows;
    for bar in target.iter_mut() {
        if let Some(index) = pending
            .iter()
            .position(|row| row.open_time == bar.open_time)
        {
            let row = pending.swap_remove(index);
            if row.close_time != bar.close_time || row.available != bar.available {
                return Err(TapeError::OverlayShape {
                    open_time: row.open_time,
                });
            }
            *bar = row;
        }
    }
    if let Some(extra) = pending.first() {
        return Err(TapeError::OverlayShape {
            open_time: extra.open_time,
        });
    }
    Ok(AppliedOverlay {
        dataset: overlay.dataset.clone(),
        path,
        sha256: hex_encode(&got),
    })
}

fn overlay_rows(file: File, symbol: &Symbol, dataset: &str) -> Result<Vec<Bar>, TapeError> {
    // The sha pin is the authenticity check. DuckDB resolution files have no
    // `parangsae.scale` key, so this path does not call `require_scale`.
    let batches = record_batches(file, false)?;
    let resolution = read_string(&batches, "resolution")?;
    let symbols = read_string(&batches, "symbol")?;
    let interval = read_i64(&batches, "interval_us")?;
    let open_time = read_i64(&batches, "open_time_us")?;
    let close_time = read_i64(&batches, "close_time_us")?;
    let available = read_i64(&batches, "available_ts_us")?;
    let open = read_i64(&batches, "open_ticks")?;
    let high = read_i64(&batches, "high_ticks")?;
    let low = read_i64(&batches, "low_ticks")?;
    let close = read_i64(&batches, "close_ticks")?;
    let volume = read_i64(&batches, "volume_ticks")?;
    let schema = read_u32(&batches, "schema_version")?;
    let rows = symbols.len();
    let count = match read_i64(&batches, "count") {
        Ok(values) => values,
        Err(TapeError::MissingColumn { .. }) => vec![1; rows],
        Err(err) => return Err(err),
    };
    same_len("resolution", resolution.len(), rows)?;
    same_len("interval_us", interval.len(), rows)?;
    same_len("open_time_us", open_time.len(), rows)?;
    same_len("close_time_us", close_time.len(), rows)?;
    same_len("available_ts_us", available.len(), rows)?;
    same_len("open_ticks", open.len(), rows)?;
    same_len("high_ticks", high.len(), rows)?;
    same_len("low_ticks", low.len(), rows)?;
    same_len("close_ticks", close.len(), rows)?;
    same_len("volume_ticks", volume.len(), rows)?;
    same_len("schema_version", schema.len(), rows)?;
    same_len("count", count.len(), rows)?;
    let mut bars = Vec::with_capacity(rows);
    for index in 0..rows {
        if symbols[index] != symbol.as_str() {
            return Err(TapeError::SymbolMismatch {
                got: symbols[index].clone(),
            });
        }
        if interval[index] != ONE_MINUTE_US {
            return Err(TapeError::Interval {
                got: interval[index],
            });
        }
        if schema[index] != 1 && schema[index] != 2 {
            return Err(TapeError::UnknownSchema {
                dataset: dataset.to_owned(),
                got: schema[index],
            });
        }
        bars.push(Bar {
            open_time: UtcMicros::from_micros(open_time[index]),
            close_time: UtcMicros::from_micros(close_time[index]),
            available: UtcMicros::from_micros(available[index]),
            open: price_at(open[index], "open_ticks")?,
            high: price_at(high[index], "high_ticks")?,
            low: price_at(low[index], "low_ticks")?,
            close: price_at(close[index], "close_ticks")?,
            volume: Qty::from_ticks(volume[index]).map_err(|_| TapeError::BadQty {
                column: "volume_ticks".to_owned(),
            })?,
            count: count[index],
        });
    }
    Ok(bars)
}

fn sha256_bytes(bytes: &[u8]) -> [u8; 32] {
    let digest = Sha256::digest(bytes);
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

fn hex_encode(bytes: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for byte in bytes {
        out.push(HEX[usize::from(byte >> 4)] as char);
        out.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    out
}

fn price_at(ticks: i64, column: &str) -> Result<Price, TapeError> {
    Price::from_ticks(ticks).map_err(|_| TapeError::BadPrice {
        column: column.to_owned(),
    })
}

fn same_len(name: &str, len: usize, expected: usize) -> Result<(), TapeError> {
    if len == expected {
        Ok(())
    } else {
        Err(TapeError::ColumnType {
            name: name.to_owned(),
        })
    }
}

fn read_batches(path: &Path) -> Result<Vec<RecordBatch>, TapeError> {
    if !path.is_file() {
        return Err(TapeError::MissingFile {
            path: path.display().to_string(),
        });
    }
    let mut file = File::open(path).map_err(|err| TapeError::Io {
        path: path.display().to_string(),
        message: err.to_string(),
    })?;
    let mut magic = [0u8; 13];
    let read = file.read(&mut magic).map_err(|err| TapeError::Io {
        path: path.display().to_string(),
        message: err.to_string(),
    })?;
    if read == POISON.len() && magic == POISON {
        panic!("holdout file was read: {}", path.display());
    }
    file.seek(SeekFrom::Start(0)).map_err(|err| TapeError::Io {
        path: path.display().to_string(),
        message: err.to_string(),
    })?;
    record_batches(file, true)
}

fn record_batches(file: File, check_scale: bool) -> Result<Vec<RecordBatch>, TapeError> {
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)
        .map_err(|err| TapeError::Arrow(err.to_string()))?;
    if check_scale {
        require_scale(builder.schema().as_ref())?;
    }
    let reader = builder
        .build()
        .map_err(|err| TapeError::Arrow(err.to_string()))?;
    let mut batches = Vec::new();
    for batch in reader {
        batches.push(batch.map_err(|err| TapeError::Arrow(err.to_string()))?);
    }
    Ok(batches)
}

fn require_scale(schema: &Schema) -> Result<(), TapeError> {
    match schema.metadata().get("parangsae.scale").map(String::as_str) {
        Some("100000000") => Ok(()),
        Some(other) => Err(TapeError::Scale {
            got: other.to_owned(),
        }),
        None => Err(TapeError::Scale {
            got: "missing".to_owned(),
        }),
    }
}

fn read_i64(batches: &[RecordBatch], name: &str) -> Result<Vec<i64>, TapeError> {
    let mut out = Vec::new();
    for batch in batches {
        let column = batch
            .column_by_name(name)
            .ok_or_else(|| TapeError::MissingColumn {
                name: name.to_owned(),
            })?;
        let array = column
            .as_any()
            .downcast_ref::<Int64Array>()
            .ok_or_else(|| TapeError::ColumnType {
                name: name.to_owned(),
            })?;
        for index in 0..array.len() {
            if array.is_null(index) {
                return Err(TapeError::NullColumn {
                    name: name.to_owned(),
                });
            }
            out.push(array.value(index));
        }
    }
    Ok(out)
}

fn read_u32(batches: &[RecordBatch], name: &str) -> Result<Vec<u32>, TapeError> {
    let mut out = Vec::new();
    for batch in batches {
        let column = batch
            .column_by_name(name)
            .ok_or_else(|| TapeError::MissingColumn {
                name: name.to_owned(),
            })?;
        let array = column
            .as_any()
            .downcast_ref::<UInt32Array>()
            .ok_or_else(|| TapeError::ColumnType {
                name: name.to_owned(),
            })?;
        for index in 0..array.len() {
            if array.is_null(index) {
                return Err(TapeError::NullColumn {
                    name: name.to_owned(),
                });
            }
            out.push(array.value(index));
        }
    }
    Ok(out)
}

fn read_string(batches: &[RecordBatch], name: &str) -> Result<Vec<String>, TapeError> {
    let mut out = Vec::new();
    for batch in batches {
        let column = batch
            .column_by_name(name)
            .ok_or_else(|| TapeError::MissingColumn {
                name: name.to_owned(),
            })?;
        let array = column
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| TapeError::ColumnType {
                name: name.to_owned(),
            })?;
        for index in 0..array.len() {
            if array.is_null(index) {
                return Err(TapeError::NullColumn {
                    name: name.to_owned(),
                });
            }
            out.push(array.value(index).to_owned());
        }
    }
    Ok(out)
}
