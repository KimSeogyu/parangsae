//! Partitioned Parquet. Fact rows use `merge_rows`: a conflict does not rewrite
//! the partition. Gap files are derived and may be replaced.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use arrow_array::{Array, BooleanArray, Int64Array, RecordBatch, StringArray, UInt32Array};
use arrow_schema::{DataType, Field, Schema};
use parangsae_core::{Amount, AvailableTs, EventTs, Qty, Ratio, ReceivedTs, Symbol, UtcMicros};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::arrow::ArrowWriter;
use parquet::basic::Compression;
use parquet::file::properties::WriterProperties;
use serde::Serialize;

use crate::civil::{day_of, CivilDay};
use crate::manifest::{merge_rows, BoundFact, MergeOutcome};
use crate::vision::{BookFact, FundingFact, Gap, GapKind, KlineFact, MetricsFact, TickDomain};
use crate::Error;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WriteCounts {
    pub inserted: u64,
    pub unchanged: u64,
    pub conflicts: u64,
}

impl WriteCounts {
    fn add(&mut self, other: WriteCounts) {
        self.inserted += other.inserted;
        self.unchanged += other.unchanged;
        self.conflicts += other.conflicts;
    }
}

pub fn upsert_klines(
    data_dir: &Path,
    dataset: &str,
    rows: Vec<KlineFact>,
) -> Result<WriteCounts, Error> {
    upsert_by_day(
        data_dir,
        dataset,
        rows,
        |row| day_of(row.open_time),
        |row| row.open_time.micros(),
        read_klines_at,
        write_klines,
        kline_key,
        kline_payload,
    )
}

pub fn upsert_funding(
    data_dir: &Path,
    dataset: &str,
    rows: Vec<FundingFact>,
) -> Result<WriteCounts, Error> {
    upsert_by_day(
        data_dir,
        dataset,
        rows,
        |row| day_of(row.funding_time),
        |row| row.funding_time.micros(),
        read_funding_at,
        write_funding,
        funding_key,
        funding_payload,
    )
}

pub fn upsert_metrics(
    data_dir: &Path,
    dataset: &str,
    rows: Vec<MetricsFact>,
) -> Result<WriteCounts, Error> {
    upsert_by_day(
        data_dir,
        dataset,
        rows,
        |row| day_of(row.create_time),
        |row| row.create_time.micros(),
        read_metrics_at,
        write_metrics,
        metrics_key,
        metrics_payload,
    )
}

pub fn upsert_book(
    data_dir: &Path,
    dataset: &str,
    rows: Vec<BookFact>,
) -> Result<WriteCounts, Error> {
    upsert_by_day(
        data_dir,
        dataset,
        rows,
        |row| day_of(row.timestamp),
        |row| (row.timestamp.micros(), row.percentage.ticks()),
        read_book_at,
        write_book,
        book_key,
        book_payload,
    )
}

#[allow(clippy::too_many_arguments)]
fn upsert_by_day<T, K, DayOf, KeyOf, Read, Write, KeyText, Payload>(
    data_dir: &Path,
    dataset: &str,
    rows: Vec<T>,
    day_of_row: DayOf,
    key_of: KeyOf,
    read: Read,
    write: Write,
    key_text: KeyText,
    payload: Payload,
) -> Result<WriteCounts, Error>
where
    T: BoundFact + SymbolField,
    K: Ord + Clone,
    DayOf: Fn(&T) -> Result<CivilDay, Error>,
    KeyOf: Fn(&T) -> K,
    Read: Fn(&Path) -> Result<Vec<T>, Error>,
    Write: Fn(&Path, &[T]) -> Result<(), Error>,
    KeyText: Fn(&T) -> String,
    Payload: Fn(&T) -> String,
{
    let mut grouped: std::collections::BTreeMap<CivilDay, Vec<T>> =
        std::collections::BTreeMap::new();
    let mut symbol = String::new();
    for row in rows {
        let text = row.symbol_text();
        if symbol.is_empty() {
            symbol = text.to_owned();
        } else if symbol != text {
            return Err(Error::check("symbol", "a partition mixes symbols"));
        }
        let day = day_of_row(&row)?;
        grouped.entry(day).or_default().push(row);
    }
    let mut total = WriteCounts::default();
    for (day, incoming) in grouped {
        let path = fact_path(data_dir, dataset, &symbol, day);
        let existing = if path.exists() {
            read(&path)?
        } else {
            Vec::new()
        };
        let merged = merge_rows(existing, incoming, &key_of)
            .map_err(|error| Error::check("fact_authority", error.to_string()))?;
        total.add(commit_merge(
            data_dir, dataset, &symbol, &path, merged, &write, &key_text, &payload,
        )?);
    }
    Ok(total)
}

#[allow(clippy::too_many_arguments)]
fn commit_merge<T, Write, KeyText, Payload>(
    data_dir: &Path,
    dataset: &str,
    symbol: &str,
    path: &Path,
    merged: MergeOutcome<T>,
    write: Write,
    key_text: KeyText,
    payload: Payload,
) -> Result<WriteCounts, Error>
where
    Write: Fn(&Path, &[T]) -> Result<(), Error>,
    KeyText: Fn(&T) -> String,
    Payload: Fn(&T) -> String,
{
    merged.commit(
        |rows| write(path, rows),
        |stored, rejected| {
            append_conflict(
                data_dir,
                dataset,
                symbol,
                &key_text(stored),
                &payload(stored),
                &payload(rejected),
            )
        },
    )
}

pub fn load_klines(
    data_dir: &Path,
    dataset: &str,
    symbol: &str,
    day: CivilDay,
) -> Result<Vec<KlineFact>, Error> {
    let path = fact_path(data_dir, dataset, symbol, day);
    if path.exists() {
        read_klines_at(&path)
    } else {
        Ok(Vec::new())
    }
}

pub fn load_funding(
    data_dir: &Path,
    dataset: &str,
    symbol: &str,
    day: CivilDay,
) -> Result<Vec<FundingFact>, Error> {
    let path = fact_path(data_dir, dataset, symbol, day);
    if path.exists() {
        read_funding_at(&path)
    } else {
        Ok(Vec::new())
    }
}

pub fn load_metrics(
    data_dir: &Path,
    dataset: &str,
    symbol: &str,
    day: CivilDay,
) -> Result<Vec<MetricsFact>, Error> {
    let path = fact_path(data_dir, dataset, symbol, day);
    if path.exists() {
        read_metrics_at(&path)
    } else {
        Ok(Vec::new())
    }
}

pub fn load_book(
    data_dir: &Path,
    dataset: &str,
    symbol: &str,
    day: CivilDay,
) -> Result<Vec<BookFact>, Error> {
    let path = fact_path(data_dir, dataset, symbol, day);
    if path.exists() {
        read_book_at(&path)
    } else {
        Ok(Vec::new())
    }
}

pub fn write_gaps(
    data_dir: &Path,
    dataset: &str,
    symbol: &str,
    day: CivilDay,
    gaps: &[Gap],
) -> Result<(), Error> {
    let path = gap_path(data_dir, dataset, symbol, day);
    if gaps.is_empty() && !path.exists() {
        return Ok(());
    }
    let kind: Vec<String> = gaps
        .iter()
        .map(|gap| gap.kind.as_str().to_owned())
        .collect();
    let start: Vec<i64> = gaps.iter().map(|gap| gap.start.micros()).collect();
    let end: Vec<i64> = gaps.iter().map(|gap| gap.end.micros()).collect();
    let detail: Vec<String> = gaps.iter().map(|gap| gap.detail.clone()).collect();
    let schema = scale_schema(vec![
        field("kind", DataType::Utf8),
        field("start_us", DataType::Int64),
        field("end_us", DataType::Int64),
        field("detail", DataType::Utf8),
    ]);
    let batch = RecordBatch::try_new(
        schema,
        vec![
            Arc::new(StringArray::from(kind)),
            Arc::new(Int64Array::from(start)),
            Arc::new(Int64Array::from(end)),
            Arc::new(StringArray::from(detail)),
        ],
    )
    .map_err(|err| Error::check("parquet", err.to_string()))?;
    write_batch(&path, batch)
}

pub fn load_gaps(
    data_dir: &Path,
    dataset: &str,
    symbol: &str,
    day: CivilDay,
) -> Result<Vec<Gap>, Error> {
    let path = gap_path(data_dir, dataset, symbol, day);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let batches = read_batches(&path)?;
    let kind = collect_string(&batches, "kind")?;
    let start = collect_i64(&batches, "start_us")?;
    let end = collect_i64(&batches, "end_us")?;
    let detail = collect_string(&batches, "detail")?;
    let mut gaps = Vec::with_capacity(kind.len());
    for index in 0..kind.len() {
        let parsed = GapKind::parse(&kind[index]).ok_or_else(|| {
            Error::check(
                "parquet_schema",
                format!("unknown gap kind {}", kind[index]),
            )
        })?;
        gaps.push(Gap {
            kind: parsed,
            start: UtcMicros::from_micros(start[index]),
            end: UtcMicros::from_micros(end[index]),
            detail: detail[index].clone(),
        });
    }
    Ok(gaps)
}

pub fn fact_path(data_dir: &Path, dataset: &str, symbol: &str, day: CivilDay) -> PathBuf {
    data_dir
        .join("parquet")
        .join(dataset)
        .join(format!("symbol={symbol}"))
        .join(format!("date={day}"))
        .join("part.parquet")
}

pub fn gap_path(data_dir: &Path, dataset: &str, symbol: &str, day: CivilDay) -> PathBuf {
    data_dir
        .join("gaps")
        .join(dataset)
        .join(format!("symbol={symbol}"))
        .join(format!("date={day}"))
        .join("part.parquet")
}

fn append_conflict(
    data_dir: &Path,
    dataset: &str,
    symbol: &str,
    key: &str,
    stored: &str,
    rejected: &str,
) -> Result<(), Error> {
    #[derive(Serialize)]
    struct Line<'a> {
        dataset: &'a str,
        symbol: &'a str,
        key: &'a str,
        stored: &'a str,
        rejected: &'a str,
    }
    let path = data_dir.join("conflicts").join(format!("{dataset}.jsonl"));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| Error::Io {
            path: parent.display().to_string(),
            source,
        })?;
    }
    let line = serde_json::to_string(&Line {
        dataset,
        symbol,
        key,
        stored,
        rejected,
    })
    .map_err(|err| Error::check("conflict", err.to_string()))?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|source| Error::Io {
            path: path.display().to_string(),
            source,
        })?;
    writeln!(file, "{line}").map_err(|source| Error::Io {
        path: path.display().to_string(),
        source,
    })?;
    Ok(())
}

fn kline_key(row: &KlineFact) -> String {
    row.open_time.micros().to_string()
}

fn kline_payload(row: &KlineFact) -> String {
    format!(
        "open={} high={} low={} close={} volume={} close_time={} count={} taker={} domain={} schema={}",
        row.open_ticks,
        row.high_ticks,
        row.low_ticks,
        row.close_ticks,
        row.volume.ticks(),
        row.close_time.micros(),
        row.count,
        row.taker_buy_volume.ticks(),
        row.tick_domain.as_str(),
        row.schema_version
    )
}

fn funding_key(row: &FundingFact) -> String {
    row.funding_time.micros().to_string()
}

fn funding_payload(row: &FundingFact) -> String {
    format!(
        "raw={} rate={} interval={}",
        row.raw_calc_time.micros(),
        row.rate.ticks(),
        row.interval_hours
    )
}

fn metrics_key(row: &MetricsFact) -> String {
    row.create_time.micros().to_string()
}

fn metrics_payload(row: &MetricsFact) -> String {
    format!(
        "oi={} value={} ratios={},{},{},{}",
        row.sum_open_interest.ticks(),
        row.sum_open_interest_value.ticks(),
        row.count_toptrader_long_short_ratio.ticks(),
        row.sum_toptrader_long_short_ratio.ticks(),
        row.count_long_short_ratio.ticks(),
        row.sum_taker_long_short_vol_ratio.ticks()
    )
}

fn book_key(row: &BookFact) -> String {
    format!("{}:{}", row.timestamp.micros(), row.percentage.ticks())
}

fn book_payload(row: &BookFact) -> String {
    format!(
        "depth={} notional={} schema={}",
        row.depth.ticks(),
        row.notional.ticks(),
        row.level_schema
    )
}

fn write_klines(path: &Path, rows: &[KlineFact]) -> Result<(), Error> {
    let schema = scale_schema(vec![
        field("symbol", DataType::Utf8),
        field("interval_us", DataType::Int64),
        field("open_time_us", DataType::Int64),
        field("close_time_us", DataType::Int64),
        field("open_ticks", DataType::Int64),
        field("high_ticks", DataType::Int64),
        field("low_ticks", DataType::Int64),
        field("close_ticks", DataType::Int64),
        field("volume_ticks", DataType::Int64),
        field("quote_volume_ticks", DataType::Int64),
        field("count", DataType::Int64),
        field("taker_buy_volume_ticks", DataType::Int64),
        field("taker_buy_quote_volume_ticks", DataType::Int64),
        field("event_ts_us", DataType::Int64),
        field("available_ts_us", DataType::Int64),
        field("received_ts_us", DataType::Int64),
        field("schema_version", DataType::UInt32),
        field("tick_domain", DataType::Utf8),
    ]);
    let batch = RecordBatch::try_new(
        schema,
        vec![
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|row| row.symbol.as_str().to_owned())
                    .collect::<Vec<_>>(),
            )),
            Arc::new(Int64Array::from(
                rows.iter().map(|row| row.interval_us).collect::<Vec<_>>(),
            )),
            Arc::new(Int64Array::from(
                rows.iter()
                    .map(|row| row.open_time.micros())
                    .collect::<Vec<_>>(),
            )),
            Arc::new(Int64Array::from(
                rows.iter()
                    .map(|row| row.close_time.micros())
                    .collect::<Vec<_>>(),
            )),
            Arc::new(Int64Array::from(
                rows.iter().map(|row| row.open_ticks).collect::<Vec<_>>(),
            )),
            Arc::new(Int64Array::from(
                rows.iter().map(|row| row.high_ticks).collect::<Vec<_>>(),
            )),
            Arc::new(Int64Array::from(
                rows.iter().map(|row| row.low_ticks).collect::<Vec<_>>(),
            )),
            Arc::new(Int64Array::from(
                rows.iter().map(|row| row.close_ticks).collect::<Vec<_>>(),
            )),
            Arc::new(Int64Array::from(
                rows.iter()
                    .map(|row| row.volume.ticks())
                    .collect::<Vec<_>>(),
            )),
            Arc::new(Int64Array::from(
                rows.iter()
                    .map(|row| row.quote_volume.ticks())
                    .collect::<Vec<_>>(),
            )),
            Arc::new(Int64Array::from(
                rows.iter().map(|row| row.count).collect::<Vec<_>>(),
            )),
            Arc::new(Int64Array::from(
                rows.iter()
                    .map(|row| row.taker_buy_volume.ticks())
                    .collect::<Vec<_>>(),
            )),
            Arc::new(Int64Array::from(
                rows.iter()
                    .map(|row| row.taker_buy_quote_volume.ticks())
                    .collect::<Vec<_>>(),
            )),
            Arc::new(Int64Array::from(
                rows.iter()
                    .map(|row| row.event_ts.micros().micros())
                    .collect::<Vec<_>>(),
            )),
            Arc::new(Int64Array::from(
                rows.iter()
                    .map(|row| row.available_ts.micros().micros())
                    .collect::<Vec<_>>(),
            )),
            Arc::new(Int64Array::from(
                rows.iter()
                    .map(|row| row.received_ts.micros().micros())
                    .collect::<Vec<_>>(),
            )),
            Arc::new(UInt32Array::from(
                rows.iter()
                    .map(|row| row.schema_version)
                    .collect::<Vec<_>>(),
            )),
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|row| row.tick_domain.as_str().to_owned())
                    .collect::<Vec<_>>(),
            )),
        ],
    )
    .map_err(|err| Error::check("parquet", err.to_string()))?;
    write_batch(path, batch)
}

fn read_klines_at(path: &Path) -> Result<Vec<KlineFact>, Error> {
    let batches = read_batches(path)?;
    let symbol = collect_string(&batches, "symbol")?;
    let interval = collect_i64(&batches, "interval_us")?;
    let open_time = collect_i64(&batches, "open_time_us")?;
    let close_time = collect_i64(&batches, "close_time_us")?;
    let open_ticks = collect_i64(&batches, "open_ticks")?;
    let high_ticks = collect_i64(&batches, "high_ticks")?;
    let low_ticks = collect_i64(&batches, "low_ticks")?;
    let close_ticks = collect_i64(&batches, "close_ticks")?;
    let volume = collect_i64(&batches, "volume_ticks")?;
    let quote = collect_i64(&batches, "quote_volume_ticks")?;
    let count = collect_i64(&batches, "count")?;
    let taker = collect_i64(&batches, "taker_buy_volume_ticks")?;
    let taker_quote = collect_i64(&batches, "taker_buy_quote_volume_ticks")?;
    let event = collect_i64(&batches, "event_ts_us")?;
    let available = collect_i64(&batches, "available_ts_us")?;
    let received = collect_i64(&batches, "received_ts_us")?;
    let schema = collect_u32(&batches, "schema_version")?;
    let domain = collect_string(&batches, "tick_domain")?;
    let mut rows = Vec::with_capacity(symbol.len());
    for index in 0..symbol.len() {
        let tick_domain = TickDomain::parse(&domain[index]).ok_or_else(|| {
            Error::check("parquet_schema", format!("tick domain {}", domain[index]))
        })?;
        rows.push(KlineFact {
            symbol: Symbol::new(&symbol[index])?,
            interval_us: interval[index],
            open_time: UtcMicros::from_micros(open_time[index]),
            close_time: UtcMicros::from_micros(close_time[index]),
            open_ticks: open_ticks[index],
            high_ticks: high_ticks[index],
            low_ticks: low_ticks[index],
            close_ticks: close_ticks[index],
            volume: Qty::from_ticks(volume[index])?,
            quote_volume: Amount::from_ticks(quote[index])?,
            count: count[index],
            taker_buy_volume: Qty::from_ticks(taker[index])?,
            taker_buy_quote_volume: Amount::from_ticks(taker_quote[index])?,
            event_ts: EventTs::from_micros(UtcMicros::from_micros(event[index])),
            available_ts: AvailableTs::from_micros(UtcMicros::from_micros(available[index])),
            received_ts: ReceivedTs::from_micros(UtcMicros::from_micros(received[index])),
            schema_version: schema[index],
            tick_domain,
        });
    }
    Ok(rows)
}

fn write_funding(path: &Path, rows: &[FundingFact]) -> Result<(), Error> {
    let schema = scale_schema(vec![
        field("symbol", DataType::Utf8),
        field("raw_calc_time_us", DataType::Int64),
        field("funding_time_us", DataType::Int64),
        field("interval_hours", DataType::Int64),
        field("rate_ticks", DataType::Int64),
        field("event_ts_us", DataType::Int64),
        field("available_ts_us", DataType::Int64),
        field("received_ts_us", DataType::Int64),
        field("schema_version", DataType::UInt32),
    ]);
    let batch = RecordBatch::try_new(
        schema,
        vec![
            strings(rows.iter().map(|row| row.symbol.as_str())),
            i64s(rows.iter().map(|row| row.raw_calc_time.micros())),
            i64s(rows.iter().map(|row| row.funding_time.micros())),
            i64s(rows.iter().map(|row| row.interval_hours)),
            i64s(rows.iter().map(|row| row.rate.ticks())),
            i64s(rows.iter().map(|row| row.event_ts.micros().micros())),
            i64s(rows.iter().map(|row| row.available_ts.micros().micros())),
            i64s(rows.iter().map(|row| row.received_ts.micros().micros())),
            u32s(rows.iter().map(|row| row.schema_version)),
        ],
    )
    .map_err(|err| Error::check("parquet", err.to_string()))?;
    write_batch(path, batch)
}

fn read_funding_at(path: &Path) -> Result<Vec<FundingFact>, Error> {
    let batches = read_batches(path)?;
    let symbol = collect_string(&batches, "symbol")?;
    let raw = collect_i64(&batches, "raw_calc_time_us")?;
    let funding = collect_i64(&batches, "funding_time_us")?;
    let interval = collect_i64(&batches, "interval_hours")?;
    let rate = collect_i64(&batches, "rate_ticks")?;
    let event = collect_i64(&batches, "event_ts_us")?;
    let available = collect_i64(&batches, "available_ts_us")?;
    let received = collect_i64(&batches, "received_ts_us")?;
    let schema = collect_u32(&batches, "schema_version")?;
    let mut rows = Vec::with_capacity(symbol.len());
    for index in 0..symbol.len() {
        rows.push(FundingFact {
            symbol: Symbol::new(&symbol[index])?,
            raw_calc_time: UtcMicros::from_micros(raw[index]),
            funding_time: UtcMicros::from_micros(funding[index]),
            interval_hours: interval[index],
            rate: Ratio::from_ticks(rate[index]),
            event_ts: EventTs::from_micros(UtcMicros::from_micros(event[index])),
            available_ts: AvailableTs::from_micros(UtcMicros::from_micros(available[index])),
            received_ts: ReceivedTs::from_micros(UtcMicros::from_micros(received[index])),
            schema_version: schema[index],
        });
    }
    Ok(rows)
}

fn write_metrics(path: &Path, rows: &[MetricsFact]) -> Result<(), Error> {
    let schema = scale_schema(vec![
        field("symbol", DataType::Utf8),
        field("create_time_us", DataType::Int64),
        field("sum_open_interest_ticks", DataType::Int64),
        field("sum_open_interest_value_ticks", DataType::Int64),
        field("count_toptrader_long_short_ratio_ticks", DataType::Int64),
        field("sum_toptrader_long_short_ratio_ticks", DataType::Int64),
        field("count_long_short_ratio_ticks", DataType::Int64),
        field("sum_taker_long_short_vol_ratio_ticks", DataType::Int64),
        field("event_ts_us", DataType::Int64),
        field("available_ts_us", DataType::Int64),
        field("received_ts_us", DataType::Int64),
        field("available_lag_verified", DataType::Boolean),
        field("schema_version", DataType::UInt32),
    ]);
    let batch = RecordBatch::try_new(
        schema,
        vec![
            strings(rows.iter().map(|row| row.symbol.as_str())),
            i64s(rows.iter().map(|row| row.create_time.micros())),
            i64s(rows.iter().map(|row| row.sum_open_interest.ticks())),
            i64s(rows.iter().map(|row| row.sum_open_interest_value.ticks())),
            i64s(
                rows.iter()
                    .map(|row| row.count_toptrader_long_short_ratio.ticks()),
            ),
            i64s(
                rows.iter()
                    .map(|row| row.sum_toptrader_long_short_ratio.ticks()),
            ),
            i64s(rows.iter().map(|row| row.count_long_short_ratio.ticks())),
            i64s(
                rows.iter()
                    .map(|row| row.sum_taker_long_short_vol_ratio.ticks()),
            ),
            i64s(rows.iter().map(|row| row.event_ts.micros().micros())),
            i64s(rows.iter().map(|row| row.available_ts.micros().micros())),
            i64s(rows.iter().map(|row| row.received_ts.micros().micros())),
            Arc::new(BooleanArray::from(
                rows.iter()
                    .map(|row| row.available_lag_verified)
                    .collect::<Vec<_>>(),
            )),
            u32s(rows.iter().map(|row| row.schema_version)),
        ],
    )
    .map_err(|err| Error::check("parquet", err.to_string()))?;
    write_batch(path, batch)
}

fn read_metrics_at(path: &Path) -> Result<Vec<MetricsFact>, Error> {
    let batches = read_batches(path)?;
    let symbol = collect_string(&batches, "symbol")?;
    let create = collect_i64(&batches, "create_time_us")?;
    let oi = collect_i64(&batches, "sum_open_interest_ticks")?;
    let value = collect_i64(&batches, "sum_open_interest_value_ticks")?;
    let count_top = collect_i64(&batches, "count_toptrader_long_short_ratio_ticks")?;
    let sum_top = collect_i64(&batches, "sum_toptrader_long_short_ratio_ticks")?;
    let count_ratio = collect_i64(&batches, "count_long_short_ratio_ticks")?;
    let taker = collect_i64(&batches, "sum_taker_long_short_vol_ratio_ticks")?;
    let event = collect_i64(&batches, "event_ts_us")?;
    let available = collect_i64(&batches, "available_ts_us")?;
    let received = collect_i64(&batches, "received_ts_us")?;
    let verified = collect_bool(&batches, "available_lag_verified")?;
    let schema = collect_u32(&batches, "schema_version")?;
    let mut rows = Vec::with_capacity(symbol.len());
    for index in 0..symbol.len() {
        let create_time = UtcMicros::from_micros(create[index]);
        rows.push(MetricsFact {
            symbol: Symbol::new(&symbol[index])?,
            create_time,
            sum_open_interest: Qty::from_ticks(oi[index])?,
            sum_open_interest_value: Amount::from_ticks(value[index])?,
            count_toptrader_long_short_ratio: Ratio::from_ticks(count_top[index]),
            sum_toptrader_long_short_ratio: Ratio::from_ticks(sum_top[index]),
            count_long_short_ratio: Ratio::from_ticks(count_ratio[index]),
            sum_taker_long_short_vol_ratio: Ratio::from_ticks(taker[index]),
            event_ts: EventTs::from_micros(UtcMicros::from_micros(event[index])),
            available_ts: AvailableTs::from_micros(UtcMicros::from_micros(available[index])),
            received_ts: ReceivedTs::from_micros(UtcMicros::from_micros(received[index])),
            available_lag_verified: verified[index],
            schema_version: schema[index],
        });
    }
    Ok(rows)
}

fn write_book(path: &Path, rows: &[BookFact]) -> Result<(), Error> {
    let schema = scale_schema(vec![
        field("symbol", DataType::Utf8),
        field("timestamp_us", DataType::Int64),
        field("percentage_ticks", DataType::Int64),
        field("depth_ticks", DataType::Int64),
        field("notional_ticks", DataType::Int64),
        field("level_schema", DataType::UInt32),
        field("event_ts_us", DataType::Int64),
        field("available_ts_us", DataType::Int64),
        field("received_ts_us", DataType::Int64),
        field("schema_version", DataType::UInt32),
    ]);
    let batch = RecordBatch::try_new(
        schema,
        vec![
            strings(rows.iter().map(|row| row.symbol.as_str())),
            i64s(rows.iter().map(|row| row.timestamp.micros())),
            i64s(rows.iter().map(|row| row.percentage.ticks())),
            i64s(rows.iter().map(|row| row.depth.ticks())),
            i64s(rows.iter().map(|row| row.notional.ticks())),
            u32s(rows.iter().map(|row| row.level_schema)),
            i64s(rows.iter().map(|row| row.event_ts.micros().micros())),
            i64s(rows.iter().map(|row| row.available_ts.micros().micros())),
            i64s(rows.iter().map(|row| row.received_ts.micros().micros())),
            u32s(rows.iter().map(|row| row.schema_version)),
        ],
    )
    .map_err(|err| Error::check("parquet", err.to_string()))?;
    write_batch(path, batch)
}

fn read_book_at(path: &Path) -> Result<Vec<BookFact>, Error> {
    let batches = read_batches(path)?;
    let symbol = collect_string(&batches, "symbol")?;
    let timestamp = collect_i64(&batches, "timestamp_us")?;
    let percentage = collect_i64(&batches, "percentage_ticks")?;
    let depth = collect_i64(&batches, "depth_ticks")?;
    let notional = collect_i64(&batches, "notional_ticks")?;
    let level_schema = collect_u32(&batches, "level_schema")?;
    let event = collect_i64(&batches, "event_ts_us")?;
    let available = collect_i64(&batches, "available_ts_us")?;
    let received = collect_i64(&batches, "received_ts_us")?;
    let schema = collect_u32(&batches, "schema_version")?;
    let mut rows = Vec::with_capacity(symbol.len());
    for index in 0..symbol.len() {
        let ts = UtcMicros::from_micros(timestamp[index]);
        rows.push(BookFact {
            symbol: Symbol::new(&symbol[index])?,
            timestamp: ts,
            percentage: Ratio::from_ticks(percentage[index]),
            depth: Qty::from_ticks(depth[index])?,
            notional: Amount::from_ticks(notional[index])?,
            level_schema: level_schema[index],
            event_ts: EventTs::from_micros(UtcMicros::from_micros(event[index])),
            available_ts: AvailableTs::from_micros(UtcMicros::from_micros(available[index])),
            received_ts: ReceivedTs::from_micros(UtcMicros::from_micros(received[index])),
            schema_version: schema[index],
        });
    }
    Ok(rows)
}

fn strings<'a>(values: impl Iterator<Item = &'a str>) -> std::sync::Arc<dyn Array> {
    Arc::new(StringArray::from(
        values.map(str::to_owned).collect::<Vec<_>>(),
    ))
}

fn i64s(values: impl Iterator<Item = i64>) -> std::sync::Arc<dyn Array> {
    Arc::new(Int64Array::from(values.collect::<Vec<_>>()))
}

fn u32s(values: impl Iterator<Item = u32>) -> std::sync::Arc<dyn Array> {
    Arc::new(UInt32Array::from(values.collect::<Vec<_>>()))
}

fn field(name: &str, data_type: DataType) -> Field {
    Field::new(name, data_type, false)
}

fn scale_schema(fields: Vec<Field>) -> Arc<Schema> {
    let mut metadata = std::collections::HashMap::new();
    metadata.insert("parangsae.scale".to_owned(), "100000000".to_owned());
    Arc::new(Schema::new_with_metadata(fields, metadata))
}

fn write_batch(path: &Path, batch: RecordBatch) -> Result<(), Error> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| Error::Io {
            path: parent.display().to_string(),
            source,
        })?;
    }
    let tmp = path.with_extension("partial");
    let file = File::create(&tmp).map_err(|source| Error::Io {
        path: tmp.display().to_string(),
        source,
    })?;
    let properties = WriterProperties::builder()
        .set_compression(Compression::SNAPPY)
        .build();
    let mut writer = ArrowWriter::try_new(file, batch.schema(), Some(properties))
        .map_err(|err| Error::check("parquet", err.to_string()))?;
    writer
        .write(&batch)
        .map_err(|err| Error::check("parquet", err.to_string()))?;
    writer
        .close()
        .map_err(|err| Error::check("parquet", err.to_string()))?;
    fs::rename(&tmp, path).map_err(|source| Error::Io {
        path: path.display().to_string(),
        source,
    })?;
    Ok(())
}

fn read_batches(path: &Path) -> Result<Vec<RecordBatch>, Error> {
    let file = File::open(path).map_err(|source| Error::Io {
        path: path.display().to_string(),
        source,
    })?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)
        .map_err(|err| Error::check("parquet", err.to_string()))?;
    require_scale(builder.schema().as_ref())?;
    let reader = builder
        .build()
        .map_err(|err| Error::check("parquet", err.to_string()))?;
    let mut batches = Vec::new();
    for batch in reader {
        batches.push(batch.map_err(|err| Error::check("parquet", err.to_string()))?);
    }
    Ok(batches)
}

fn require_scale(schema: &Schema) -> Result<(), Error> {
    match schema.metadata().get("parangsae.scale").map(String::as_str) {
        Some("100000000") => Ok(()),
        Some(other) => Err(Error::check("scale", format!("parquet scale is {other}"))),
        None => Err(Error::check("scale", "parquet schema has no scale")),
    }
}

fn collect_i64(batches: &[RecordBatch], name: &str) -> Result<Vec<i64>, Error> {
    let mut out = Vec::new();
    for batch in batches {
        let array = batch
            .column_by_name(name)
            .ok_or_else(|| Error::check("parquet_schema", format!("missing {name}")))?
            .as_any()
            .downcast_ref::<Int64Array>()
            .ok_or_else(|| Error::check("parquet_schema", format!("{name} is not int64")))?;
        for index in 0..array.len() {
            if array.is_null(index) {
                return Err(Error::check("parquet_schema", format!("{name} has a null")));
            }
            out.push(array.value(index));
        }
    }
    Ok(out)
}

fn collect_u32(batches: &[RecordBatch], name: &str) -> Result<Vec<u32>, Error> {
    let mut out = Vec::new();
    for batch in batches {
        let array = batch
            .column_by_name(name)
            .ok_or_else(|| Error::check("parquet_schema", format!("missing {name}")))?
            .as_any()
            .downcast_ref::<UInt32Array>()
            .ok_or_else(|| Error::check("parquet_schema", format!("{name} is not uint32")))?;
        for index in 0..array.len() {
            if array.is_null(index) {
                return Err(Error::check("parquet_schema", format!("{name} has a null")));
            }
            out.push(array.value(index));
        }
    }
    Ok(out)
}

fn collect_string(batches: &[RecordBatch], name: &str) -> Result<Vec<String>, Error> {
    let mut out = Vec::new();
    for batch in batches {
        let array = batch
            .column_by_name(name)
            .ok_or_else(|| Error::check("parquet_schema", format!("missing {name}")))?
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| Error::check("parquet_schema", format!("{name} is not utf8")))?;
        for index in 0..array.len() {
            if array.is_null(index) {
                return Err(Error::check("parquet_schema", format!("{name} has a null")));
            }
            out.push(array.value(index).to_owned());
        }
    }
    Ok(out)
}

fn collect_bool(batches: &[RecordBatch], name: &str) -> Result<Vec<bool>, Error> {
    let mut out = Vec::new();
    for batch in batches {
        let array = batch
            .column_by_name(name)
            .ok_or_else(|| Error::check("parquet_schema", format!("missing {name}")))?
            .as_any()
            .downcast_ref::<BooleanArray>()
            .ok_or_else(|| Error::check("parquet_schema", format!("{name} is not bool")))?;
        for index in 0..array.len() {
            if array.is_null(index) {
                return Err(Error::check("parquet_schema", format!("{name} has a null")));
            }
            out.push(array.value(index));
        }
    }
    Ok(out)
}

trait SymbolField {
    fn symbol_text(&self) -> &str;
}

impl SymbolField for KlineFact {
    fn symbol_text(&self) -> &str {
        self.symbol.as_str()
    }
}
impl SymbolField for FundingFact {
    fn symbol_text(&self) -> &str {
        self.symbol.as_str()
    }
}
impl SymbolField for MetricsFact {
    fn symbol_text(&self) -> &str {
        self.symbol.as_str()
    }
}
impl SymbolField for BookFact {
    fn symbol_text(&self) -> &str {
        self.symbol.as_str()
    }
}
