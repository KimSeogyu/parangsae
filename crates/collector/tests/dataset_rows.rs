//! Row-encoding checks that an observer can see: the flat flag, v2 round trip,
//! and payload equality ignoring the receipt.

use std::fs;

use parangsae_collector::civil::{day_of, CivilDay};
use parangsae_collector::store::{self, fact_path};
use parangsae_collector::vision::{parse_klines, zero_volume_flat, KlineFact, TickDomain};
use parangsae_collector::Error;
use parangsae_core::{Receipt, RunId, Symbol, UtcMicros};

fn symbol() -> Symbol {
    Symbol::new("BTCUSDT").expect("symbol")
}

fn run() -> RunId {
    RunId::from_bytes([9; 16])
}

fn archive() -> Receipt {
    Receipt::Archive {
        fetched_at_run: run(),
    }
}

fn parse_um(csv: &str, record_flat: bool) -> Result<Vec<KlineFact>, Error> {
    Ok(parse_klines(
        csv,
        &symbol(),
        parangsae_collector::normalize::UM_KLINE_CLOCK,
        TickDomain::Price,
        archive(),
        record_flat,
    )?
    .rows)
}

#[test]
fn zero_volume_flat_is_descriptive_and_mark_files_omit_it() -> Result<(), Error> {
    let flat = "1622505600000,1,1,1,1,0,1622505659999,0,0,0,0,0\n";
    let counted = "1622505600000,1,1,1,1,0,1622505659999,0,1,0,0,0\n";
    let flat_rows = parse_um(flat, true)?;
    assert!(flat_rows[0].zero_volume_flat);
    assert!(zero_volume_flat(0, 0, 1, 1, 1, 1));
    let counted_rows = parse_um(counted, true)?;
    assert!(!counted_rows[0].zero_volume_flat);
    assert!(!zero_volume_flat(0, 1, 1, 1, 1, 1));

    let marked = parse_um(flat, false)?;
    assert!(!marked[0].zero_volume_flat);

    let day = day_of(marked[0].open_time)?;
    let mark_dir = tempfile::tempdir().expect("temp");
    store::upsert_klines(mark_dir.path(), "mark-price-kline", marked)?;
    let loaded = store::load_klines(mark_dir.path(), "mark-price-kline", "BTCUSDT", day)?;
    assert!(!loaded[0].zero_volume_flat);

    let um_dir = tempfile::tempdir().expect("temp");
    let um_path = fact_path(um_dir.path(), "um-kline", "BTCUSDT", day);
    fs::create_dir_all(um_path.parent().expect("parent")).expect("dir");
    fs::copy(
        fact_path(mark_dir.path(), "mark-price-kline", "BTCUSDT", day),
        &um_path,
    )
    .expect("copy");
    let missing =
        store::load_klines(um_dir.path(), "um-kline", "BTCUSDT", day).expect_err("column");
    assert_eq!(missing.check_name(), Some("schema"));

    let with_column = tempfile::tempdir().expect("temp");
    store::upsert_klines(with_column.path(), "um-kline", flat_rows.clone())?;
    let mark_with_column = tempfile::tempdir().expect("temp");
    let mark_path = fact_path(mark_with_column.path(), "mark-price-kline", "BTCUSDT", day);
    fs::create_dir_all(mark_path.parent().expect("parent")).expect("dir");
    fs::copy(
        fact_path(with_column.path(), "um-kline", "BTCUSDT", day),
        &mark_path,
    )
    .expect("copy");
    let unexpected =
        store::load_klines(mark_with_column.path(), "mark-price-kline", "BTCUSDT", day)
            .expect_err("column");
    assert_eq!(unexpected.check_name(), Some("schema"));

    let mut illegal = flat_rows;
    illegal[0].zero_volume_flat = true;
    let rejected = store::upsert_klines(
        tempfile::tempdir().expect("temp").path(),
        "mark-price-kline",
        illegal,
    )
    .expect_err("flag");
    assert_eq!(rejected.check_name(), Some("zero_volume_flat"));
    Ok(())
}

#[test]
fn v2_round_trip_keeps_the_archive_receipt_out_of_equality() -> Result<(), Error> {
    let csv = fs::read_to_string(format!(
        "{}/../../fixtures/vision/um_kline_2021-06-01_headerless.csv",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("fixture");
    let rows = parse_um(&csv, true)?;
    assert!(rows.iter().all(|row| {
        row.receipt.archive_run() == Some(run()) && row.receipt.live_received_ts().is_none()
    }));
    let mut other = rows.clone();
    for row in &mut other {
        row.receipt = Receipt::Live {
            received_ts: UtcMicros::from_micros(5),
        };
    }
    assert_eq!(rows, other);

    let dir = tempfile::tempdir().expect("temp");
    let day = CivilDay::new(2021, 6, 1).expect("day");
    store::upsert_klines(dir.path(), "um-kline", rows.clone())?;
    let loaded = store::load_klines(dir.path(), "um-kline", "BTCUSDT", day)?;
    assert_eq!(loaded, rows);
    assert!(loaded
        .iter()
        .all(|row| row.receipt.live_received_ts().is_none()));

    let path = fact_path(dir.path(), "um-kline", "BTCUSDT", day);
    let received = vec![7_i64; rows.len()];
    let v1 = dir.path().join("v1.parquet");
    store::write_v1_klines(&v1, &rows, &received)?;
    fs::copy(&v1, &path).expect("replace");
    let err = store::load_klines(dir.path(), "um-kline", "BTCUSDT", day).expect_err("v1");
    assert_eq!(err.check_name(), Some("schema"));
    Ok(())
}
