//! C2: conflict equality is the data payload hash, not `PartialEq`.

use std::path::PathBuf;

use proptest::prelude::*;

use crate::manifest::merge_rows;
use crate::normalize::UM_KLINE_CLOCK;
use crate::run::RunSummary;
use crate::store;
use crate::vision::{
    parse_klines, BookFact, FundingFact, KlineFact, MetricsFact, TickDomain, SCHEMA_KLINE_HEADER,
    SCHEMA_KLINE_HEADERLESS,
};
use parangsae_core::{Amount, AvailableTs, EventTs, Qty, Ratio, Receipt, RunId, Symbol, UtcMicros};

const HEADER: &str = "open_time,open,high,low,close,volume,close_time,quote_volume,count,taker_buy_volume,taker_buy_quote_volume,ignore";
const AGREE_MS: [i64; 3] = [1_699_628_700_000, 1_699_634_760_000, 1_699_634_820_000];
const REF_MS: i64 = 1_699_628_820_000;
const MONTHLY_PAYLOAD: &str = "symbol=BTCUSDT interval_us=60000000 open_time_us=1699628820000000 close_time_us=1699628879999000 open_ticks=3711840000000 high_ticks=3711840000000 low_ticks=3711840000000 close_ticks=3711840000000 volume_ticks=0 quote_volume_ticks=0 count=0 taker_buy_volume_ticks=0 taker_buy_quote_volume_ticks=0 tick_domain=price";
const DAILY_PAYLOAD: &str = "symbol=BTCUSDT interval_us=60000000 open_time_us=1699628820000000 close_time_us=1699628879999000 open_ticks=3714760000000 high_ticks=3715610000000 low_ticks=3711600000000 close_ticks=3715300000000 volume_ticks=34896000000 quote_volume_ticks=1296085691290000 count=3102 taker_buy_volume_ticks=13826900000 taker_buy_quote_volume_ticks=513622261460000 tick_domain=price";

fn symbol() -> Symbol {
    Symbol::new("BTCUSDT").expect("symbol")
}

fn receipt() -> Receipt {
    Receipt::Archive {
        fetched_at_run: RunId::from_bytes([4; 16]),
    }
}

fn read_fix(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/vision")
        .join(name);
    std::fs::read_to_string(path).unwrap_or_else(|err| panic!("read {name}: {err}"))
}

fn parse(csv: &str) -> Vec<KlineFact> {
    parse_klines(
        csv,
        &symbol(),
        UM_KLINE_CLOCK,
        TickDomain::Price,
        receipt(),
        true,
    )
    .expect("parse")
    .rows
}

fn us(ms: i64) -> i64 {
    ms.checked_mul(1_000).expect("ms to us")
}

fn by_open(rows: &[KlineFact], open_ms: i64) -> KlineFact {
    rows.iter()
        .find(|row| row.open_time.micros() == us(open_ms))
        .unwrap_or_else(|| panic!("missing {open_ms}"))
        .clone()
}

fn only(rows: Vec<KlineFact>, keys_ms: &[i64]) -> Vec<KlineFact> {
    let wanted: Vec<i64> = keys_ms.iter().copied().map(us).collect();
    rows.into_iter()
        .filter(|row| wanted.contains(&row.open_time.micros()))
        .collect()
}

fn header_pair() -> (Vec<KlineFact>, Vec<KlineFact>) {
    let headerless = read_fix("um_kline_2021-06-01_headerless.csv");
    let headed = format!("{HEADER}\n{headerless}");
    let monthly = parse(&headerless);
    let daily = parse(&headed);
    assert_eq!(monthly[0].schema_version, SCHEMA_KLINE_HEADERLESS);
    assert_eq!(daily[0].schema_version, SCHEMA_KLINE_HEADER);
    (monthly, daily)
}

#[test]
fn header_only_pair_is_not_a_conflict() -> Result<(), String> {
    let (monthly, daily) = header_pair();
    if monthly.len() != daily.len() || monthly.is_empty() {
        return Err(format!("row counts {} {}", monthly.len(), daily.len()));
    }
    for (left, right) in monthly.iter().zip(daily.iter()) {
        if left == right {
            return Err("header pair compared equal under PartialEq".into());
        }
        if left.data_hash() != right.data_hash() {
            return Err(format!(
                "hash {} vs {}",
                left.data_hash(),
                right.data_hash()
            ));
        }
    }
    let merged = merge_rows(monthly.clone(), daily.clone(), |row| row.open_time.micros());
    if !merged.conflicts.is_empty() || !merged.committed {
        return Err(format!("conflicts {}", merged.conflicts.len()));
    }
    if merged.unchanged != u64::try_from(daily.len()).map_err(|err| err.to_string())? {
        return Err(format!("unchanged {}", merged.unchanged));
    }
    if merged
        .rows
        .iter()
        .any(|row| row.schema_version != SCHEMA_KLINE_HEADERLESS)
    {
        return Err("first-stored schema was replaced".into());
    }

    let dir = tempfile::tempdir().map_err(|err| err.to_string())?;
    let mut summary = RunSummary::default();
    summary.add_counts(
        store::upsert_klines(dir.path(), "um-kline", monthly).map_err(|err| err.to_string())?,
    );
    summary.add_counts(
        store::upsert_klines(dir.path(), "um-kline", daily).map_err(|err| err.to_string())?,
    );
    if summary.conflicts != 0 {
        return Err(format!("RunSummary conflicts {}", summary.conflicts));
    }
    if summary.rows_unchanged != summary.rows_inserted {
        return Err(format!("RunSummary {summary}"));
    }
    let log = dir.path().join("conflicts").join("um-kline.jsonl");
    if log.exists() {
        return Err("a header-only pair wrote a conflict line".into());
    }

    let monthly = only(
        parse(&read_fix("integrity/um_monthly_2023-11-10_slice.csv")),
        &AGREE_MS,
    );
    let daily = only(
        parse(&read_fix("integrity/um_daily_2023-11-10_slice.csv")),
        &AGREE_MS,
    );
    if monthly.len() != 3 || daily.len() != 3 {
        return Err(format!("agree keys {} {}", monthly.len(), daily.len()));
    }
    let merged = merge_rows(monthly, daily, |row| row.open_time.micros());
    if !merged.conflicts.is_empty() || merged.unchanged != 3 {
        return Err(format!(
            "appendix agree conflicts {} unchanged {}",
            merged.conflicts.len(),
            merged.unchanged
        ));
    }
    Ok(())
}

#[test]
fn real_value_differences_stay_conflicts() -> Result<(), String> {
    let monthly = parse(&read_fix("integrity/um_monthly_2023-11-10_slice.csv"));
    let daily = parse(&read_fix("integrity/um_daily_2023-11-10_slice.csv"));
    let merged = merge_rows(monthly, daily, |row| row.open_time.micros());
    let keys: Vec<i64> = merged
        .conflicts
        .iter()
        .map(|(stored, _)| stored.open_time.micros())
        .collect();
    if keys.len() != 100 {
        return Err(format!("november 10 conflicts {}", keys.len()));
    }
    if keys[0] != us(1_699_628_760_000) || keys[99] != us(1_699_634_700_000) {
        return Err(format!("range {}..{}", keys[0], keys[99]));
    }
    for pair in keys.windows(2) {
        if pair[1] - pair[0] != 60_000_000 {
            return Err("conflict keys are not a 1m grid".into());
        }
    }
    if keys
        .iter()
        .any(|key| AGREE_MS.iter().any(|ms| us(*ms) == *key))
    {
        return Err("an identical key was counted as a conflict".into());
    }

    let monthly = parse(&read_fix("integrity/um_monthly_2023-11-14_slice.csv"));
    let daily = parse(&read_fix("integrity/um_daily_2023-11-14_slice.csv"));
    let merged = merge_rows(monthly, daily, |row| row.open_time.micros());
    let keys: Vec<i64> = merged
        .conflicts
        .iter()
        .map(|(stored, _)| stored.open_time.micros())
        .collect();
    if keys.len() != 5 || keys[0] != us(1_699_961_880_000) || keys[4] != us(1_699_962_120_000) {
        return Err(format!("november 14 {keys:?}"));
    }
    Ok(())
}

#[test]
fn reference_payloads_match_section_3_1() -> Result<(), String> {
    let monthly = by_open(
        &parse(&read_fix("integrity/um_monthly_2023-11-10_slice.csv")),
        REF_MS,
    );
    let daily = by_open(
        &parse(&read_fix("integrity/um_daily_2023-11-10_slice.csv")),
        REF_MS,
    );
    if monthly.data_payload_v2() != MONTHLY_PAYLOAD {
        return Err(monthly.data_payload_v2());
    }
    if daily.data_payload_v2() != DAILY_PAYLOAD {
        return Err(daily.data_payload_v2());
    }
    if monthly.data_hash() != "20b05233fe0f4edaa15ee1956896ab1aa2cde1cbe470e7aec52356139b365fa0" {
        return Err(monthly.data_hash());
    }
    if daily.data_hash() != "c5433fe7bbb46f69e4437779521b1f47dfdeb0ee07447b1ffdafbec3a46e90a6" {
        return Err(daily.data_hash());
    }
    Ok(())
}

fn sample() -> KlineFact {
    let open = UtcMicros::from_micros(1_699_628_820_000_000);
    let close = UtcMicros::from_micros(1_699_628_879_999_000);
    KlineFact {
        symbol: symbol(),
        interval_us: 60_000_000,
        open_time: open,
        close_time: close,
        open_ticks: 10,
        high_ticks: 12,
        low_ticks: 9,
        close_ticks: 11,
        volume: Qty::from_ticks(3).unwrap(),
        quote_volume: Amount::from_ticks(4).unwrap(),
        count: 5,
        taker_buy_volume: Qty::from_ticks(1).unwrap(),
        taker_buy_quote_volume: Amount::from_ticks(2).unwrap(),
        event_ts: EventTs::from_micros(open),
        available_ts: AvailableTs::from_micros(close),
        receipt: receipt(),
        schema_version: 1,
        tick_domain: TickDomain::Price,
        zero_volume_flat: false,
    }
}

fn bump(row: &mut KlineFact, which: u8, delta: i64) {
    match which {
        0 => row.symbol = Symbol::new("ETHUSDT").expect("symbol"),
        1 => row.interval_us += delta,
        2 => row.open_time = UtcMicros::from_micros(row.open_time.micros() + delta),
        3 => row.close_time = UtcMicros::from_micros(row.close_time.micros() + delta),
        4 => row.open_ticks += delta,
        5 => row.high_ticks += delta,
        6 => row.low_ticks += delta,
        7 => row.close_ticks += delta,
        8 => row.volume = Qty::from_ticks(row.volume.ticks() + delta).unwrap(),
        9 => row.quote_volume = Amount::from_ticks(row.quote_volume.ticks() + delta).unwrap(),
        10 => row.count += delta,
        11 => row.taker_buy_volume = Qty::from_ticks(row.taker_buy_volume.ticks() + delta).unwrap(),
        12 => {
            row.taker_buy_quote_volume =
                Amount::from_ticks(row.taker_buy_quote_volume.ticks() + delta).unwrap();
        }
        13 => row.tick_domain = TickDomain::Ratio,
        _ => unreachable!("field index"),
    }
}

proptest! {
    #[test]
    fn one_data_field_changes_the_hash(which in 0u8..14, delta in 1i64..1_000) {
        let base = sample();
        let mut changed = base.clone();
        bump(&mut changed, which, delta);
        prop_assert_ne!(base.data_hash(), changed.data_hash());
    }
}

#[test]
fn provenance_fields_do_not_change_the_hash() -> Result<(), String> {
    let base = sample();
    let hash = base.data_hash();
    let mut schema = base.clone();
    schema.schema_version = 2;
    let mut event = base.clone();
    event.event_ts = EventTs::from_micros(UtcMicros::from_micros(1));
    let mut available = base.clone();
    available.available_ts = AvailableTs::from_micros(UtcMicros::from_micros(2));
    let mut live = base.clone();
    live.receipt = Receipt::Live {
        received_ts: UtcMicros::from_micros(3),
    };
    let mut flat = base.clone();
    flat.zero_volume_flat = !base.zero_volume_flat;
    for (name, row) in [
        ("schema", schema),
        ("event", event),
        ("available", available),
        ("receipt", live),
        ("flat", flat),
    ] {
        if row.data_hash() != hash {
            return Err(name.into());
        }
    }
    let funding = funding_row(1);
    let mut other = funding.clone();
    other.event_ts = EventTs::from_micros(UtcMicros::from_micros(9));
    other.schema_version = 9;
    if funding.data_hash() != other.data_hash() {
        return Err("funding provenance".into());
    }
    other.rate = Ratio::from_ticks(other.rate.ticks() + 1);
    if funding.data_hash() == other.data_hash() {
        return Err("funding rate was ignored".into());
    }
    let metrics = metrics_row();
    let mut other = metrics.clone();
    other.schema_version = 9;
    if metrics.data_hash() != other.data_hash()
        || !metrics.data_payload_v2().ends_with("missing_tokens=")
    {
        return Err(metrics.data_payload_v2());
    }
    let book = book_row(1);
    let mut other = book.clone();
    other.level_schema = 12;
    if book.data_hash() != other.data_hash() {
        return Err("level_schema entered the book payload".into());
    }
    other.depth = Qty::from_ticks(8).unwrap();
    if book.data_hash() == other.data_hash() {
        return Err("book depth was ignored".into());
    }
    Ok(())
}

fn funding_row(rate: i64) -> FundingFact {
    let slot = UtcMicros::from_micros(1_577_836_800_000_000);
    FundingFact {
        symbol: symbol(),
        raw_calc_time: slot,
        funding_time: slot,
        interval_hours: 8,
        rate: Ratio::from_ticks(rate),
        event_ts: EventTs::from_micros(slot),
        available_ts: AvailableTs::from_micros(UtcMicros::from_micros(1_577_836_860_000_000)),
        receipt: receipt(),
        schema_version: 3,
    }
}

fn metrics_row() -> MetricsFact {
    let create = UtcMicros::from_micros(1_600_000_000_000_000);
    let ratio = Ratio::from_ticks(1);
    MetricsFact {
        symbol: symbol(),
        create_time: create,
        sum_open_interest: Qty::from_ticks(2).unwrap(),
        sum_open_interest_value: Amount::from_ticks(3).unwrap(),
        count_toptrader_long_short_ratio: ratio,
        sum_toptrader_long_short_ratio: ratio,
        count_long_short_ratio: ratio,
        sum_taker_long_short_vol_ratio: ratio,
        event_ts: EventTs::from_micros(create),
        available_ts: AvailableTs::from_micros(UtcMicros::from_micros(1_600_000_300_000_000)),
        receipt: receipt(),
        available_lag_verified: false,
        schema_version: 4,
    }
}

fn book_row(depth: i64) -> BookFact {
    let timestamp = UtcMicros::from_micros(1_700_000_000_000_000);
    BookFact {
        symbol: symbol(),
        timestamp,
        percentage: Ratio::from_ticks(1),
        depth: Qty::from_ticks(depth).unwrap(),
        notional: Amount::from_ticks(5).unwrap(),
        level_schema: 10,
        event_ts: EventTs::from_micros(timestamp),
        available_ts: AvailableTs::from_micros(timestamp),
        receipt: receipt(),
        schema_version: 5,
    }
}
