//! Rows/v1 decoder used only by `migrate-receipts`.
//!
//! `store::load_*` and `open_v2` do not call [`read_v1`]. A v1 file still fails
//! those readers with check `schema`.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use arrow_array::{Array, BooleanArray, RecordBatch};
use arrow_schema::{DataType, Field, Schema};
use parangsae_core::{Amount, AvailableTs, EventTs, Qty, Ratio, Receipt, RunId, Symbol, UtcMicros};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

use crate::civil::{parse_day, CivilDay};
use crate::store::{self, collect_bool, collect_i64, collect_string, collect_u32};
use crate::vision::{zero_volume_flat, BookFact, FundingFact, KlineFact, MetricsFact, TickDomain};
use crate::Error;

#[derive(Clone, Debug)]
pub enum V1Partition {
    Klines {
        dataset: String,
        rows: Vec<KlineFact>,
        received_ts_us: Vec<i64>,
    },
    Funding {
        rows: Vec<FundingFact>,
        received_ts_us: Vec<i64>,
    },
    Metrics {
        rows: Vec<MetricsFact>,
        received_ts_us: Vec<i64>,
    },
    Book {
        rows: Vec<BookFact>,
        received_ts_us: Vec<i64>,
    },
}

impl V1Partition {
    pub fn received_ts_us(&self) -> &[i64] {
        match self {
            Self::Klines { received_ts_us, .. }
            | Self::Funding { received_ts_us, .. }
            | Self::Metrics { received_ts_us, .. }
            | Self::Book { received_ts_us, .. } => received_ts_us,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PartitionKind {
    V1,
    V2,
}

pub(crate) fn partition_kind(path: &Path) -> Result<PartitionKind, Error> {
    let schema = parquet_schema(path)?;
    let has_received = schema.column_with_name("received_ts_us").is_some();
    let encoding = schema
        .metadata()
        .get("parangsae.row_encoding")
        .map(String::as_str);
    match (has_received, encoding) {
        (true, None) => Ok(PartitionKind::V1),
        (false, Some("rows/v2")) => Ok(PartitionKind::V2),
        _ => Err(Error::check(
            "schema",
            format!("{} is neither rows/v1 nor rows/v2", path.display()),
        )),
    }
}

pub(crate) fn v1_partitions(data_dir: &Path) -> Result<Vec<PathBuf>, Error> {
    let root = data_dir.join("parquet");
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut found = Vec::new();
    walk_parquet(&root, &mut found)?;
    found.sort();
    Ok(found)
}

fn walk_parquet(dir: &Path, found: &mut Vec<PathBuf>) -> Result<(), Error> {
    let entries = fs::read_dir(dir).map_err(|source| Error::Io {
        path: dir.display().to_string(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| Error::Io {
            path: dir.display().to_string(),
            source,
        })?;
        let path = entry.path();
        if path.is_dir() {
            walk_parquet(&path, found)?;
            continue;
        }
        if path.file_name().and_then(|name| name.to_str()) != Some("part.parquet") {
            continue;
        }
        if partition_kind(&path)? == PartitionKind::V1 {
            found.push(path);
        }
    }
    Ok(())
}

pub(crate) fn partition_location(
    data_dir: &Path,
    path: &Path,
) -> Result<(String, String, CivilDay), Error> {
    let relative = path.strip_prefix(data_dir).map_err(|_| {
        Error::check(
            "schema",
            format!("{} is outside the data dir", path.display()),
        )
    })?;
    let parts: Vec<String> = relative
        .components()
        .map(|component| {
            component
                .as_os_str()
                .to_str()
                .map(str::to_owned)
                .ok_or_else(|| Error::check("schema", format!("{} is not utf-8", path.display())))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if parts.len() != 5 || parts[0] != "parquet" || parts[4] != "part.parquet" {
        return Err(Error::check(
            "schema",
            format!("{} is not a fact partition", path.display()),
        ));
    }
    let symbol = parts[2].strip_prefix("symbol=").ok_or_else(|| {
        Error::check(
            "schema",
            format!("{} has no symbol partition", path.display()),
        )
    })?;
    let date = parts[3].strip_prefix("date=").ok_or_else(|| {
        Error::check(
            "schema",
            format!("{} has no date partition", path.display()),
        )
    })?;
    let day = parse_day(date)?;
    Ok((parts[1].clone(), symbol.to_owned(), day))
}

/// Decode a rows/v1 fact file. Receipt on the returned rows is a placeholder;
/// the migration assigns the real archive run before it writes rows/v2.
pub fn read_v1(path: &Path, dataset: &str) -> Result<V1Partition, Error> {
    if partition_kind(path)? != PartitionKind::V1 {
        return Err(Error::check(
            "schema",
            format!("{} is not a v1 file", path.display()),
        ));
    }
    let batches = store::read_batches(path)?;
    match dataset {
        "um-kline"
        | "spot-kline"
        | "mark-price-kline"
        | "index-price-kline"
        | "premium-index-kline" => {
            let (rows, received_ts_us) = read_v1_klines(&batches, dataset)?;
            Ok(V1Partition::Klines {
                dataset: dataset.to_owned(),
                rows,
                received_ts_us,
            })
        }
        "funding-rate" => {
            let (rows, received_ts_us) = read_v1_funding(&batches)?;
            Ok(V1Partition::Funding {
                rows,
                received_ts_us,
            })
        }
        "metrics" => {
            let (rows, received_ts_us) = read_v1_metrics(&batches)?;
            Ok(V1Partition::Metrics {
                rows,
                received_ts_us,
            })
        }
        "book-depth" => {
            let (rows, received_ts_us) = read_v1_book(&batches)?;
            Ok(V1Partition::Book {
                rows,
                received_ts_us,
            })
        }
        other => Err(Error::check(
            "schema",
            format!("no v1 layout for dataset {other}"),
        )),
    }
}

fn placeholder_receipt() -> Receipt {
    Receipt::Archive {
        fetched_at_run: RunId::from_bytes([0; 16]),
    }
}

fn read_v1_klines(
    batches: &[RecordBatch],
    dataset: &str,
) -> Result<(Vec<KlineFact>, Vec<i64>), Error> {
    let symbol = collect_string(batches, "symbol")?;
    let interval = collect_i64(batches, "interval_us")?;
    let open_time = collect_i64(batches, "open_time_us")?;
    let close_time = collect_i64(batches, "close_time_us")?;
    let open_ticks = collect_i64(batches, "open_ticks")?;
    let high_ticks = collect_i64(batches, "high_ticks")?;
    let low_ticks = collect_i64(batches, "low_ticks")?;
    let close_ticks = collect_i64(batches, "close_ticks")?;
    let volume = collect_i64(batches, "volume_ticks")?;
    let quote = collect_i64(batches, "quote_volume_ticks")?;
    let count = collect_i64(batches, "count")?;
    let taker = collect_i64(batches, "taker_buy_volume_ticks")?;
    let taker_quote = collect_i64(batches, "taker_buy_quote_volume_ticks")?;
    let event = collect_i64(batches, "event_ts_us")?;
    let available = collect_i64(batches, "available_ts_us")?;
    let received = collect_i64(batches, "received_ts_us")?;
    let schema = collect_u32(batches, "schema_version")?;
    let domain = collect_string(batches, "tick_domain")?;
    let flat = dataset == "um-kline" || dataset == "spot-kline";
    let mut rows = Vec::with_capacity(symbol.len());
    for index in 0..symbol.len() {
        let tick_domain = TickDomain::parse(&domain[index])
            .ok_or_else(|| Error::check("schema", format!("tick domain {}", domain[index])))?;
        let zero_volume_flat = if flat {
            zero_volume_flat(
                volume[index],
                count[index],
                open_ticks[index],
                high_ticks[index],
                low_ticks[index],
                close_ticks[index],
            )
        } else {
            false
        };
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
            receipt: placeholder_receipt(),
            schema_version: schema[index],
            tick_domain,
            zero_volume_flat,
        });
    }
    Ok((rows, received))
}

fn read_v1_funding(batches: &[RecordBatch]) -> Result<(Vec<FundingFact>, Vec<i64>), Error> {
    let symbol = collect_string(batches, "symbol")?;
    let raw = collect_i64(batches, "raw_calc_time_us")?;
    let funding = collect_i64(batches, "funding_time_us")?;
    let interval = collect_i64(batches, "interval_hours")?;
    let rate = collect_i64(batches, "rate_ticks")?;
    let event = collect_i64(batches, "event_ts_us")?;
    let available = collect_i64(batches, "available_ts_us")?;
    let received = collect_i64(batches, "received_ts_us")?;
    let schema = collect_u32(batches, "schema_version")?;
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
            receipt: placeholder_receipt(),
            schema_version: schema[index],
        });
    }
    Ok((rows, received))
}

fn read_v1_metrics(batches: &[RecordBatch]) -> Result<(Vec<MetricsFact>, Vec<i64>), Error> {
    let symbol = collect_string(batches, "symbol")?;
    let create_time = collect_i64(batches, "create_time_us")?;
    let open_interest = collect_i64(batches, "sum_open_interest_ticks")?;
    let open_interest_value = collect_i64(batches, "sum_open_interest_value_ticks")?;
    let count_top = collect_i64(batches, "count_toptrader_long_short_ratio_ticks")?;
    let sum_top = collect_i64(batches, "sum_toptrader_long_short_ratio_ticks")?;
    let count_ratio = collect_i64(batches, "count_long_short_ratio_ticks")?;
    let taker_ratio = collect_i64(batches, "sum_taker_long_short_vol_ratio_ticks")?;
    let event = collect_i64(batches, "event_ts_us")?;
    let available = collect_i64(batches, "available_ts_us")?;
    let received = collect_i64(batches, "received_ts_us")?;
    let lag = collect_bool(batches, "available_lag_verified")?;
    let schema = collect_u32(batches, "schema_version")?;
    let mut rows = Vec::with_capacity(symbol.len());
    for index in 0..symbol.len() {
        let create = UtcMicros::from_micros(create_time[index]);
        rows.push(MetricsFact {
            symbol: Symbol::new(&symbol[index])?,
            create_time: create,
            sum_open_interest: Qty::from_ticks(open_interest[index])?,
            sum_open_interest_value: Amount::from_ticks(open_interest_value[index])?,
            count_toptrader_long_short_ratio: Ratio::from_ticks(count_top[index]),
            sum_toptrader_long_short_ratio: Ratio::from_ticks(sum_top[index]),
            count_long_short_ratio: Ratio::from_ticks(count_ratio[index]),
            sum_taker_long_short_vol_ratio: Ratio::from_ticks(taker_ratio[index]),
            event_ts: EventTs::from_micros(UtcMicros::from_micros(event[index])),
            available_ts: AvailableTs::from_micros(UtcMicros::from_micros(available[index])),
            receipt: placeholder_receipt(),
            available_lag_verified: lag[index],
            schema_version: schema[index],
        });
    }
    Ok((rows, received))
}

fn read_v1_book(batches: &[RecordBatch]) -> Result<(Vec<BookFact>, Vec<i64>), Error> {
    let symbol = collect_string(batches, "symbol")?;
    let timestamp = collect_i64(batches, "timestamp_us")?;
    let percentage = collect_i64(batches, "percentage_ticks")?;
    let depth = collect_i64(batches, "depth_ticks")?;
    let notional = collect_i64(batches, "notional_ticks")?;
    let level_schema = collect_u32(batches, "level_schema")?;
    let event = collect_i64(batches, "event_ts_us")?;
    let available = collect_i64(batches, "available_ts_us")?;
    let received = collect_i64(batches, "received_ts_us")?;
    let schema = collect_u32(batches, "schema_version")?;
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
            receipt: placeholder_receipt(),
            schema_version: schema[index],
        });
    }
    Ok((rows, received))
}

pub fn write_v1_funding(
    path: &Path,
    rows: &[FundingFact],
    received_ts_us: &[i64],
) -> Result<(), Error> {
    same_len(rows.len(), received_ts_us.len())?;
    write_v1(
        path,
        vec![
            store::field("symbol", DataType::Utf8),
            store::field("raw_calc_time_us", DataType::Int64),
            store::field("funding_time_us", DataType::Int64),
            store::field("interval_hours", DataType::Int64),
            store::field("rate_ticks", DataType::Int64),
            store::field("event_ts_us", DataType::Int64),
            store::field("available_ts_us", DataType::Int64),
            store::field("received_ts_us", DataType::Int64),
            store::field("schema_version", DataType::UInt32),
        ],
        vec![
            store::strings(rows.iter().map(|row| row.symbol.as_str())),
            store::i64s(rows.iter().map(|row| row.raw_calc_time.micros())),
            store::i64s(rows.iter().map(|row| row.funding_time.micros())),
            store::i64s(rows.iter().map(|row| row.interval_hours)),
            store::i64s(rows.iter().map(|row| row.rate.ticks())),
            store::i64s(rows.iter().map(|row| row.event_ts.micros().micros())),
            store::i64s(rows.iter().map(|row| row.available_ts.micros().micros())),
            store::i64s(received_ts_us.iter().copied()),
            store::u32s(rows.iter().map(|row| row.schema_version)),
        ],
    )
}

pub fn write_v1_metrics(
    path: &Path,
    rows: &[MetricsFact],
    received_ts_us: &[i64],
) -> Result<(), Error> {
    same_len(rows.len(), received_ts_us.len())?;
    write_v1(
        path,
        vec![
            store::field("symbol", DataType::Utf8),
            store::field("create_time_us", DataType::Int64),
            store::field("sum_open_interest_ticks", DataType::Int64),
            store::field("sum_open_interest_value_ticks", DataType::Int64),
            store::field("count_toptrader_long_short_ratio_ticks", DataType::Int64),
            store::field("sum_toptrader_long_short_ratio_ticks", DataType::Int64),
            store::field("count_long_short_ratio_ticks", DataType::Int64),
            store::field("sum_taker_long_short_vol_ratio_ticks", DataType::Int64),
            store::field("event_ts_us", DataType::Int64),
            store::field("available_ts_us", DataType::Int64),
            store::field("received_ts_us", DataType::Int64),
            store::field("available_lag_verified", DataType::Boolean),
            store::field("schema_version", DataType::UInt32),
        ],
        vec![
            store::strings(rows.iter().map(|row| row.symbol.as_str())),
            store::i64s(rows.iter().map(|row| row.create_time.micros())),
            store::i64s(rows.iter().map(|row| row.sum_open_interest.ticks())),
            store::i64s(rows.iter().map(|row| row.sum_open_interest_value.ticks())),
            store::i64s(
                rows.iter()
                    .map(|row| row.count_toptrader_long_short_ratio.ticks()),
            ),
            store::i64s(
                rows.iter()
                    .map(|row| row.sum_toptrader_long_short_ratio.ticks()),
            ),
            store::i64s(rows.iter().map(|row| row.count_long_short_ratio.ticks())),
            store::i64s(
                rows.iter()
                    .map(|row| row.sum_taker_long_short_vol_ratio.ticks()),
            ),
            store::i64s(rows.iter().map(|row| row.event_ts.micros().micros())),
            store::i64s(rows.iter().map(|row| row.available_ts.micros().micros())),
            store::i64s(received_ts_us.iter().copied()),
            Arc::new(BooleanArray::from(
                rows.iter()
                    .map(|row| row.available_lag_verified)
                    .collect::<Vec<_>>(),
            )),
            store::u32s(rows.iter().map(|row| row.schema_version)),
        ],
    )
}

pub fn write_v1_book(path: &Path, rows: &[BookFact], received_ts_us: &[i64]) -> Result<(), Error> {
    same_len(rows.len(), received_ts_us.len())?;
    write_v1(
        path,
        vec![
            store::field("symbol", DataType::Utf8),
            store::field("timestamp_us", DataType::Int64),
            store::field("percentage_ticks", DataType::Int64),
            store::field("depth_ticks", DataType::Int64),
            store::field("notional_ticks", DataType::Int64),
            store::field("level_schema", DataType::UInt32),
            store::field("event_ts_us", DataType::Int64),
            store::field("available_ts_us", DataType::Int64),
            store::field("received_ts_us", DataType::Int64),
            store::field("schema_version", DataType::UInt32),
        ],
        vec![
            store::strings(rows.iter().map(|row| row.symbol.as_str())),
            store::i64s(rows.iter().map(|row| row.timestamp.micros())),
            store::i64s(rows.iter().map(|row| row.percentage.ticks())),
            store::i64s(rows.iter().map(|row| row.depth.ticks())),
            store::i64s(rows.iter().map(|row| row.notional.ticks())),
            store::u32s(rows.iter().map(|row| row.level_schema)),
            store::i64s(rows.iter().map(|row| row.event_ts.micros().micros())),
            store::i64s(rows.iter().map(|row| row.available_ts.micros().micros())),
            store::i64s(received_ts_us.iter().copied()),
            store::u32s(rows.iter().map(|row| row.schema_version)),
        ],
    )
}

fn same_len(rows: usize, received: usize) -> Result<(), Error> {
    if rows == received {
        Ok(())
    } else {
        Err(Error::check(
            "schema",
            "v1 fixture row count does not match received_ts_us",
        ))
    }
}

fn write_v1(path: &Path, fields: Vec<Field>, columns: Vec<Arc<dyn Array>>) -> Result<(), Error> {
    let schema = store::scale_schema(fields);
    let batch = RecordBatch::try_new(schema, columns)
        .map_err(|err| Error::check("parquet", err.to_string()))?;
    store::write_batch(path, batch)
}

fn parquet_schema(path: &Path) -> Result<Arc<Schema>, Error> {
    let file = File::open(path).map_err(|source| Error::Io {
        path: path.display().to_string(),
        source,
    })?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)
        .map_err(|err| Error::check("parquet", err.to_string()))?;
    Ok(builder.schema().clone())
}
