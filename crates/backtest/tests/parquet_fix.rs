//! Tiny parquet writer for tape-reader tests. Not a store writer.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use arrow_array::{ArrayRef, Int64Array, RecordBatch, StringArray, UInt32Array};
use arrow_schema::{DataType, Field, Schema};
use parangsae_backtest::UtcDate;
use parquet::arrow::ArrowWriter;
use parquet::basic::Compression;
use parquet::file::properties::WriterProperties;

pub const SYMBOL: &str = "BTCUSDT";
pub const DAY: &str = "2024-01-15";

pub struct Day {
    pub interval_us: i64,
    pub schema_version: u32,
    pub funding_schema: u32,
    pub scale: &'static str,
    pub row_symbol: &'static str,
    pub close_sign: i64,
    pub null_open: bool,
    pub omit_open: bool,
    pub funding_rate: i64,
    pub funding_available_delta: i64,
    pub funding_event_delta: i64,
    pub um_open_us: Option<Vec<i64>>,
    pub mark_open_us: Option<Vec<i64>>,
    pub funding_time_us: Option<i64>,
    /// Per-row `volume_ticks`. `None` writes `2 * 10^8` on every row.
    pub volumes: Option<Vec<i64>>,
    /// Per-row `count`. `None` writes 1 on every row unless `omit_count`.
    pub counts: Option<Vec<i64>>,
    pub omit_count: bool,
}

impl Day {
    pub fn good() -> Self {
        Self {
            interval_us: 60_000_000,
            schema_version: 2,
            funding_schema: 3,
            scale: "100000000",
            row_symbol: SYMBOL,
            close_sign: 1,
            null_open: false,
            omit_open: false,
            funding_rate: 7697,
            funding_available_delta: 0,
            funding_event_delta: 0,
            um_open_us: None,
            mark_open_us: None,
            funding_time_us: None,
            volumes: None,
            counts: None,
            omit_count: false,
        }
    }
}

pub fn write_day(root: &Path, day: &Day) -> std::io::Result<()> {
    let date = UtcDate::parse(DAY).expect("fixture date");
    let start = date.start_us().micros();
    write_klines(
        root,
        "um-kline",
        day,
        start,
        day.um_open_us.clone(),
        |index| (100 + index) * 100_000_000,
    )?;
    write_klines(
        root,
        "mark-price-kline",
        day,
        start,
        day.mark_open_us.clone(),
        |index| (200 + index) * 100_000_000,
    )?;
    write_funding(root, day, day.funding_time_us.unwrap_or(start))?;
    Ok(())
}

fn write_klines(
    root: &Path,
    dataset: &str,
    day: &Day,
    start: i64,
    open_us: Option<Vec<i64>>,
    close_of: impl Fn(i64) -> i64,
) -> std::io::Result<()> {
    let n = 3i64;
    let mut fields = Vec::new();
    let mut columns: Vec<ArrayRef> = Vec::new();
    push_utf8(
        &mut fields,
        &mut columns,
        "symbol",
        vec![day.row_symbol; n as usize],
        false,
    );
    push_i64(
        &mut fields,
        &mut columns,
        "interval_us",
        vec![day.interval_us; n as usize],
        false,
    );
    let open_time =
        open_us.unwrap_or_else(|| (0..n).map(|index| start + index * 60_000_000).collect());
    let close_time: Vec<i64> = open_time
        .iter()
        .map(|open| open + 60_000_000 - 1_000)
        .collect();
    let available: Vec<i64> = close_time.iter().map(|close| close + 1_000).collect();
    push_i64(&mut fields, &mut columns, "open_time_us", open_time, false);
    push_i64(
        &mut fields,
        &mut columns,
        "close_time_us",
        close_time,
        false,
    );
    push_i64(
        &mut fields,
        &mut columns,
        "available_ts_us",
        available,
        false,
    );
    let opens: Vec<Option<i64>> = (0..n)
        .map(|index| {
            if day.null_open && index == 1 {
                None
            } else {
                Some((100 + index) * 100_000_000)
            }
        })
        .collect();
    if !day.omit_open {
        push_i64_opt(
            &mut fields,
            &mut columns,
            "open_ticks",
            opens,
            day.null_open,
        );
    }
    push_i64(
        &mut fields,
        &mut columns,
        "high_ticks",
        (0..n).map(|index| (100 + index) * 100_000_000).collect(),
        false,
    );
    push_i64(
        &mut fields,
        &mut columns,
        "low_ticks",
        (0..n).map(|index| (100 + index) * 100_000_000).collect(),
        false,
    );
    push_i64(
        &mut fields,
        &mut columns,
        "close_ticks",
        (0..n)
            .map(|index| day.close_sign * close_of(index))
            .collect(),
        false,
    );
    let volumes = day
        .volumes
        .clone()
        .unwrap_or_else(|| vec![2 * 100_000_000; n as usize]);
    push_i64(&mut fields, &mut columns, "volume_ticks", volumes, false);
    if !day.omit_count {
        let counts = day.counts.clone().unwrap_or_else(|| vec![1; n as usize]);
        push_i64(&mut fields, &mut columns, "count", counts, false);
    }
    push_u32(
        &mut fields,
        &mut columns,
        "schema_version",
        vec![day.schema_version; n as usize],
    );
    write_batch(&part(root, dataset), &fields, columns, Some(day.scale))
}

/// One overlay row. `scale: None` writes no `parangsae.scale` metadata.
pub struct OverlayRow {
    pub open_us: i64,
    pub close_ticks: i64,
    pub close_time_us: i64,
    pub available_us: i64,
    pub volume_ticks: i64,
    pub count: Option<i64>,
    pub schema_version: u32,
    pub symbol: &'static str,
    pub interval_us: i64,
    pub scale: Option<&'static str>,
}

impl OverlayRow {
    /// Times match a stored 1m bar that opens at `open_us`.
    pub fn matching(open_us: i64, close_ticks: i64) -> Self {
        let close_time_us = open_us + 60_000_000 - 1_000;
        Self {
            open_us,
            close_ticks,
            close_time_us,
            available_us: close_time_us + 1_000,
            volume_ticks: 100_000_000,
            count: None,
            schema_version: 2,
            symbol: SYMBOL,
            interval_us: 60_000_000,
            scale: Some("100000000"),
        }
    }
}

pub fn write_overlay(path: &Path, open_us: i64, close_ticks: i64) -> std::io::Result<()> {
    write_overlay_row(path, &OverlayRow::matching(open_us, close_ticks))
}

pub fn write_overlay_row(path: &Path, row: &OverlayRow) -> std::io::Result<()> {
    let mut fields = Vec::new();
    let mut columns: Vec<ArrayRef> = Vec::new();
    push_utf8(&mut fields, &mut columns, "symbol", vec![row.symbol], false);
    push_utf8(
        &mut fields,
        &mut columns,
        "resolution",
        vec!["daily-archive"],
        false,
    );
    push_i64(
        &mut fields,
        &mut columns,
        "interval_us",
        vec![row.interval_us],
        false,
    );
    push_i64(
        &mut fields,
        &mut columns,
        "open_time_us",
        vec![row.open_us],
        false,
    );
    push_i64(
        &mut fields,
        &mut columns,
        "close_time_us",
        vec![row.close_time_us],
        false,
    );
    push_i64(
        &mut fields,
        &mut columns,
        "available_ts_us",
        vec![row.available_us],
        false,
    );
    for name in ["open_ticks", "high_ticks", "low_ticks", "close_ticks"] {
        push_i64(
            &mut fields,
            &mut columns,
            name,
            vec![row.close_ticks],
            false,
        );
    }
    push_i64(
        &mut fields,
        &mut columns,
        "volume_ticks",
        vec![row.volume_ticks],
        false,
    );
    if let Some(count) = row.count {
        push_i64(&mut fields, &mut columns, "count", vec![count], false);
    }
    push_u32(
        &mut fields,
        &mut columns,
        "schema_version",
        vec![row.schema_version],
    );
    write_batch(path, &fields, columns, row.scale)
}

fn write_funding(root: &Path, day: &Day, start: i64) -> std::io::Result<()> {
    let mut fields = Vec::new();
    let mut columns: Vec<ArrayRef> = Vec::new();
    push_utf8(
        &mut fields,
        &mut columns,
        "symbol",
        vec![day.row_symbol],
        false,
    );
    push_i64(
        &mut fields,
        &mut columns,
        "funding_time_us",
        vec![start],
        false,
    );
    push_i64(&mut fields, &mut columns, "interval_hours", vec![8], false);
    push_i64(
        &mut fields,
        &mut columns,
        "rate_ticks",
        vec![day.funding_rate],
        false,
    );
    push_i64(
        &mut fields,
        &mut columns,
        "available_ts_us",
        vec![start + day.funding_available_delta],
        false,
    );
    push_i64(
        &mut fields,
        &mut columns,
        "event_ts_us",
        vec![start + day.funding_event_delta],
        false,
    );
    push_u32(
        &mut fields,
        &mut columns,
        "schema_version",
        vec![day.funding_schema],
    );
    write_batch(
        &part(root, "funding-rate"),
        &fields,
        columns,
        Some(day.scale),
    )
}

fn part(root: &Path, dataset: &str) -> PathBuf {
    root.join("parquet")
        .join(dataset)
        .join(format!("symbol={SYMBOL}"))
        .join(format!("date={DAY}"))
        .join("part.parquet")
}

fn push_utf8(
    fields: &mut Vec<Field>,
    columns: &mut Vec<ArrayRef>,
    name: &str,
    values: Vec<&str>,
    nullable: bool,
) {
    fields.push(Field::new(name, DataType::Utf8, nullable));
    columns.push(Arc::new(StringArray::from(
        values.into_iter().map(str::to_owned).collect::<Vec<_>>(),
    )));
}

fn push_i64(
    fields: &mut Vec<Field>,
    columns: &mut Vec<ArrayRef>,
    name: &str,
    values: Vec<i64>,
    nullable: bool,
) {
    fields.push(Field::new(name, DataType::Int64, nullable));
    columns.push(Arc::new(Int64Array::from(values)));
}

fn push_i64_opt(
    fields: &mut Vec<Field>,
    columns: &mut Vec<ArrayRef>,
    name: &str,
    values: Vec<Option<i64>>,
    nullable: bool,
) {
    fields.push(Field::new(name, DataType::Int64, nullable));
    columns.push(Arc::new(Int64Array::from(values)));
}

fn push_u32(fields: &mut Vec<Field>, columns: &mut Vec<ArrayRef>, name: &str, values: Vec<u32>) {
    fields.push(Field::new(name, DataType::UInt32, false));
    columns.push(Arc::new(UInt32Array::from(values)));
}

fn write_batch(
    path: &Path,
    fields: &[Field],
    columns: Vec<ArrayRef>,
    scale: Option<&str>,
) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut metadata = std::collections::HashMap::new();
    if let Some(scale) = scale {
        metadata.insert("parangsae.scale".to_owned(), scale.to_owned());
    }
    let schema = Arc::new(Schema::new_with_metadata(fields.to_vec(), metadata));
    let batch = RecordBatch::try_new(schema.clone(), columns).expect("fixture batch");
    let file = File::create(path)?;
    let properties = WriterProperties::builder()
        .set_compression(Compression::SNAPPY)
        .build();
    let mut writer = ArrowWriter::try_new(file, schema, Some(properties)).expect("arrow writer");
    writer.write(&batch).expect("write batch");
    writer.close().expect("close parquet");
    Ok(())
}
