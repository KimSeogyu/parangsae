use std::collections::HashMap;
use std::io::{Cursor, Write};
use std::time::Duration;

use proptest::prelude::*;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

use crate::civil::{CivilDay, DayRange};
use crate::fetch::MapBlob;
use crate::run::{self, RunRequest};
use crate::store;
use crate::vision::{download_url, list_url, sha256_bytes, VisionDataset};
use crate::Error;
use parangsae_core::{Amount, AvailableTs, EventTs, Qty, ReceivedTs, Symbol, UtcMicros};

trait ToU64 {
    fn to_u64(self) -> u64;
}

impl ToU64 for usize {
    fn to_u64(self) -> u64 {
        u64::try_from(self).unwrap()
    }
}

fn symbol() -> Symbol {
    Symbol::new("BTCUSDT").expect("symbol")
}

fn at(micros: i64) -> ReceivedTs {
    ReceivedTs::from_micros(UtcMicros::from_micros(micros))
}

fn zip_csv(name: &str, csv: &str) -> Vec<u8> {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer = ZipWriter::new(&mut cursor);
        writer
            .start_file(name, SimpleFileOptions::default())
            .expect("zip entry");
        writer.write_all(csv.as_bytes()).expect("zip bytes");
        writer.finish().expect("zip finish");
    }
    cursor.into_inner()
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn checksum_bytes(file_name: &str, zip_bytes: &[u8]) -> Vec<u8> {
    format!("{}  {file_name}\n", hex_encode(&sha256_bytes(zip_bytes))).into_bytes()
}

fn empty_list() -> Vec<u8> {
    b"<ListBucketResult><IsTruncated>false</IsTruncated></ListBucketResult>".to_vec()
}

fn listing(entries: &[(&str, u64)]) -> Vec<u8> {
    let mut xml = String::from("<ListBucketResult><IsTruncated>false</IsTruncated>");
    for (key, size) in entries {
        xml.push_str(&format!(
            "<Contents><Key>{key}</Key><Size>{size}</Size></Contents>"
        ));
    }
    xml.push_str("</ListBucketResult>");
    xml.into_bytes()
}

fn minute_csv(day: CivilDay, minutes: i64, price: &str) -> String {
    let start_ms = day.start_us().micros() / 1_000;
    let mut csv = String::new();
    for index in 0..minutes {
        let open = start_ms + index * 60_000;
        let close = open + 59_999;
        csv.push_str(&format!(
            "{open},{price},{price},{price},{price},0,{close},0,0,0,0,0\n"
        ));
    }
    csv
}

fn request(dir: &std::path::Path, dataset: VisionDataset, day: CivilDay) -> RunRequest {
    RunRequest {
        datasets: vec![dataset],
        symbol: symbol(),
        range: DayRange::new(day, day).expect("range"),
        data_dir: dir.to_path_buf(),
        jobs: 2,
        min_interval: Duration::from_millis(0),
        fetched_at: at(10),
    }
}

fn run_with(
    map: HashMap<String, Vec<u8>>,
    dataset: VisionDataset,
    day: CivilDay,
) -> Result<run::RunSummary, Error> {
    let dir = tempfile::tempdir().expect("temp");
    let rows =
        run::run_vision(&request(dir.path(), dataset, day), &MapBlob::new(map)).expect("run");
    Ok(rows.into_iter().next().expect("dataset").1)
}

#[test]
fn a_partial_day_is_stored_and_a_rerun_does_not_insert_again() {
    let day = CivilDay::new(2021, 6, 1).unwrap();
    let csv = std::fs::read_to_string(format!(
        "{}/../../fixtures/vision/um_kline_2021-06-01_headerless.csv",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let zip_name = "BTCUSDT-1m-2021-06-01.zip";
    let zip_bytes = zip_csv("BTCUSDT-1m-2021-06-01.csv", &csv);
    let checksum = checksum_bytes(zip_name, &zip_bytes);
    let prefix = "data/futures/um/daily/klines/BTCUSDT/1m/";
    let key = format!("{prefix}{zip_name}");
    let checksum_key = format!("{key}.CHECKSUM");
    let part = format!("{prefix}part-00000-example-c000.zip");
    let mut map = HashMap::new();
    for (_, listed) in VisionDataset::UmKline.prefixes("BTCUSDT") {
        map.insert(list_url(&listed, "").unwrap(), empty_list());
    }
    map.insert(
        list_url(prefix, "").unwrap(),
        listing(&[
            (&key, zip_bytes.len().to_u64()),
            (&checksum_key, checksum.len().to_u64()),
            (&part, 4),
        ]),
    );
    map.insert(download_url(&key), zip_bytes);
    map.insert(download_url(&checksum_key), checksum);

    let dir = tempfile::tempdir().unwrap();
    let req = request(dir.path(), VisionDataset::UmKline, day);
    let blob = MapBlob::new(map);
    let first = run::run_vision(&req, &blob).unwrap().pop().unwrap().1;
    assert_eq!(first.rows_inserted, 3);
    assert_eq!(first.quarantined, 1);
    assert!(first.gaps > 0);
    assert!(!first.ok());
    let manifest = std::fs::read_to_string(dir.path().join("manifest/objects.jsonl")).unwrap();
    let lines = manifest.lines().count();
    let second = run::run_vision(&req, &blob).unwrap().pop().unwrap().1;
    assert_eq!(second.rows_inserted, 0);
    assert_eq!(second.rows_unchanged, 3);
    let again = std::fs::read_to_string(dir.path().join("manifest/objects.jsonl")).unwrap();
    assert_eq!(again.lines().count(), lines);
}

#[test]
fn a_complete_day_verifies_and_a_replacement_price_conflicts_without_overwrite() {
    let day = CivilDay::new(2026, 9, 1).unwrap();
    let daily_prefix = "data/futures/um/daily/klines/BTCUSDT/1m/";
    let monthly_prefix = "data/futures/um/monthly/klines/BTCUSDT/1m/";
    let daily_name = "BTCUSDT-1m-2026-09-01.zip";
    let monthly_name = "BTCUSDT-1m-2026-09.zip";
    let daily_zip = zip_csv("a.csv", &minute_csv(day, 1440, "10"));
    let monthly_zip = zip_csv("b.csv", &minute_csv(day, 1440, "10"));
    let daily_key = format!("{daily_prefix}{daily_name}");
    let monthly_key = format!("{monthly_prefix}{monthly_name}");
    let daily_checksum = checksum_bytes(daily_name, &daily_zip);
    let monthly_checksum = checksum_bytes(monthly_name, &monthly_zip);
    let mut map = HashMap::new();
    for (_, prefix) in VisionDataset::UmKline.prefixes("BTCUSDT") {
        map.insert(list_url(&prefix, "").unwrap(), empty_list());
    }
    map.insert(
        list_url(daily_prefix, "").unwrap(),
        listing(&[
            (&daily_key, daily_zip.len().to_u64()),
            (
                &format!("{daily_key}.CHECKSUM"),
                daily_checksum.len().to_u64(),
            ),
        ]),
    );
    map.insert(
        list_url(monthly_prefix, "").unwrap(),
        listing(&[
            (&monthly_key, monthly_zip.len().to_u64()),
            (
                &format!("{monthly_key}.CHECKSUM"),
                monthly_checksum.len().to_u64(),
            ),
        ]),
    );
    map.insert(download_url(&daily_key), daily_zip);
    map.insert(
        download_url(&format!("{daily_key}.CHECKSUM")),
        daily_checksum,
    );
    map.insert(download_url(&monthly_key), monthly_zip);
    map.insert(
        download_url(&format!("{monthly_key}.CHECKSUM")),
        monthly_checksum,
    );

    let dir = tempfile::tempdir().unwrap();
    let req = request(dir.path(), VisionDataset::UmKline, day);
    let summary = run::run_vision(&req, &MapBlob::new(map))
        .unwrap()
        .pop()
        .unwrap()
        .1;
    assert_eq!(summary.rows_inserted, 1440, "{summary}");
    assert_eq!(summary.rows_unchanged, 1440, "{summary}");
    assert_eq!(summary.gaps, 0, "{summary}");
    assert!(summary.ok(), "{summary}");
    let verified = run::verify(dir.path(), &run::VerifyFilter::all()).unwrap();
    assert!(verified.ok(), "{verified}");

    let replacement = zip_csv("c.csv", &minute_csv(day, 1440, "11"));
    let replacement_checksum = checksum_bytes(daily_name, &replacement);
    let mut replaced = HashMap::new();
    for (_, prefix) in VisionDataset::UmKline.prefixes("BTCUSDT") {
        replaced.insert(list_url(&prefix, "").unwrap(), empty_list());
    }
    replaced.insert(
        list_url(daily_prefix, "").unwrap(),
        listing(&[
            (&daily_key, replacement.len().to_u64()),
            (
                &format!("{daily_key}.CHECKSUM"),
                replacement_checksum.len().to_u64(),
            ),
        ]),
    );
    replaced.insert(download_url(&daily_key), replacement);
    replaced.insert(
        download_url(&format!("{daily_key}.CHECKSUM")),
        replacement_checksum,
    );
    let conflict = run::run_vision(&req, &MapBlob::new(replaced))
        .unwrap()
        .pop()
        .unwrap()
        .1;
    assert!(conflict.conflicts > 0, "{conflict}");
    assert!(!conflict.ok());
    let stored = store::load_klines(dir.path(), "um-kline", "BTCUSDT", day).unwrap();
    assert_eq!(stored.len(), 1440);
    assert!(stored.iter().all(|row| row.open_ticks == 1_000_000_000));
}

#[test]
fn a_bad_checksum_stores_nothing() {
    let day = CivilDay::new(2021, 6, 1).unwrap();
    let prefix = "data/futures/um/daily/klines/BTCUSDT/1m/";
    let name = "BTCUSDT-1m-2021-06-01.zip";
    let zip_bytes = zip_csv("a.csv", "1622505600000,1,1,1,1,0,1622505659999,0,0,0,0,0\n");
    let key = format!("{prefix}{name}");
    let mut checksum = checksum_bytes(name, &zip_bytes);
    checksum[0] ^= 0x01;
    let mut map = HashMap::new();
    for (_, listed) in VisionDataset::UmKline.prefixes("BTCUSDT") {
        map.insert(list_url(&listed, "").unwrap(), empty_list());
    }
    map.insert(
        list_url(prefix, "").unwrap(),
        listing(&[
            (&key, zip_bytes.len().to_u64()),
            (&format!("{key}.CHECKSUM"), checksum.len().to_u64()),
        ]),
    );
    map.insert(download_url(&key), zip_bytes);
    map.insert(download_url(&format!("{key}.CHECKSUM")), checksum);
    let summary = run_with(map, VisionDataset::UmKline, day).unwrap();
    assert!(summary.faults > 0, "{summary}");
    assert_eq!(summary.rows_inserted, 0);
    assert!(summary
        .first_fault
        .as_deref()
        .unwrap_or("")
        .contains("checksum"));
}

#[test]
fn january_2020_funding_exponent_month_is_accepted() -> Result<(), Box<dyn std::error::Error>> {
    let csv = std::fs::read_to_string(format!(
        "{}/../../fixtures/vision/BTCUSDT-fundingRate-2020-01.csv",
        env!("CARGO_MANIFEST_DIR")
    ))?;
    let from = CivilDay::new(2020, 1, 1).expect("2020-01-01");
    let to = CivilDay::new(2020, 1, 31).expect("2020-01-31");
    let dir = tempfile::tempdir()?;
    let request = RunRequest {
        datasets: vec![VisionDataset::FundingRate],
        symbol: symbol(),
        range: DayRange::new(from, to)?,
        data_dir: dir.path().to_path_buf(),
        jobs: 2,
        min_interval: Duration::from_millis(0),
        fetched_at: at(10),
    };
    let summary = run::run_vision(&request, &MapBlob::new(funding_january_map(&csv)?))?
        .into_iter()
        .next()
        .ok_or_else(|| -> Box<dyn std::error::Error> { "missing funding summary".into() })?
        .1;
    assert_eq!(summary.faults, 0, "{summary}");
    assert_eq!(summary.conflicts, 0, "{summary}");
    assert_eq!(summary.gaps, 0, "{summary}");
    assert!(summary.ok(), "{summary}");
    assert_eq!(summary.rows_inserted, 93, "{summary}");

    let mut stored = Vec::new();
    let mut day = from;
    for _ in 0..31 {
        stored.extend(store::load_funding(
            dir.path(),
            VisionDataset::FundingRate.as_str(),
            "BTCUSDT",
            day,
        )?);
        day = day.next();
    }
    assert_eq!(stored.len(), 93);
    let rate_at = |calc_ms: i64| -> Option<i64> {
        let calc_us = calc_ms.checked_mul(1_000).expect("calc_time fits");
        stored
            .iter()
            .find(|row| row.raw_calc_time.micros() == calc_us)
            .map(|row| row.rate.ticks())
    };
    assert_eq!(rate_at(1_577_836_800_000), Some(-35));
    assert_eq!(rate_at(1_577_865_600_000), Some(-1));
    assert_eq!(rate_at(1_577_894_400_000), Some(-10));
    assert_eq!(rate_at(1_577_923_200_000), Some(6));
    assert_eq!(rate_at(1_577_952_000_000), Some(0));
    assert_eq!(rate_at(1_578_124_800_000), Some(84));
    Ok(())
}

fn funding_january_map(csv: &str) -> Result<HashMap<String, Vec<u8>>, Error> {
    let zip_name = "BTCUSDT-fundingRate-2020-01.zip";
    let zip_bytes = zip_csv("BTCUSDT-fundingRate-2020-01.csv", csv);
    let checksum = checksum_bytes(zip_name, &zip_bytes);
    let prefix = "data/futures/um/monthly/fundingRate/BTCUSDT/";
    let key = format!("{prefix}{zip_name}");
    let checksum_key = format!("{key}.CHECKSUM");
    let mut map = HashMap::new();
    for (_, listed) in VisionDataset::FundingRate.prefixes("BTCUSDT") {
        map.insert(list_url(&listed, "")?, empty_list());
    }
    map.insert(
        list_url(prefix, "")?,
        listing(&[
            (&key, zip_bytes.len() as u64),
            (&checksum_key, checksum.len() as u64),
        ]),
    );
    map.insert(download_url(&key), zip_bytes);
    map.insert(download_url(&checksum_key), checksum);
    Ok(map)
}

fn sample_bar(minute: i64, price: i64, received: i64) -> crate::vision::KlineFact {
    let open = 1_788_220_800_000_000 + minute * 60_000_000;
    let close = open + 59_999_000;
    let open_time = UtcMicros::from_micros(open);
    let close_time = UtcMicros::from_micros(close);
    crate::vision::KlineFact {
        symbol: symbol(),
        interval_us: 60_000_000,
        open_time,
        close_time,
        open_ticks: price,
        high_ticks: price,
        low_ticks: price,
        close_ticks: price,
        volume: Qty::from_ticks(0).unwrap(),
        quote_volume: Amount::from_ticks(0).unwrap(),
        count: 0,
        taker_buy_volume: Qty::from_ticks(0).unwrap(),
        taker_buy_quote_volume: Amount::from_ticks(0).unwrap(),
        event_ts: EventTs::from_micros(open_time),
        available_ts: AvailableTs::from_micros(close_time.checked_add_micros(1_000).unwrap()),
        received_ts: at(received),
        schema_version: 1,
        tick_domain: crate::vision::TickDomain::Price,
    }
}

proptest! {
    #[test]
    fn reingest_keeps_the_first_open_and_the_first_received_time(count in 1usize..24, price in 1i64..1_000_000i64) {
        let dir = tempfile::tempdir().unwrap();
        let rows: Vec<_> = (0..i64::try_from(count).unwrap()).map(|minute| sample_bar(minute, price, 1)).collect();
        let first = store::upsert_klines(dir.path(), "um-kline", rows.clone()).unwrap();
        prop_assert_eq!(first.inserted, count.to_u64());
        let again: Vec<_> = (0..i64::try_from(count).unwrap()).map(|minute| sample_bar(minute, price, 99)).collect();
        let second = store::upsert_klines(dir.path(), "um-kline", again).unwrap();
        prop_assert_eq!(second.inserted, 0);
        prop_assert_eq!(second.unchanged, count.to_u64());
        let day = CivilDay::new(2026, 9, 1).unwrap();
        let stored = store::load_klines(dir.path(), "um-kline", "BTCUSDT", day).unwrap();
        prop_assert_eq!(stored.len(), count);
        prop_assert!(stored.iter().all(|row| row.open_ticks == price && row.received_ts == at(1)));
        let mut changed = rows;
        changed[0].open_ticks += 1;
        let conflict = store::upsert_klines(dir.path(), "um-kline", changed).unwrap();
        prop_assert!(conflict.conflicts >= 1);
        prop_assert_eq!(conflict.inserted, 0);
        let kept = store::load_klines(dir.path(), "um-kline", "BTCUSDT", day).unwrap();
        prop_assert!(kept.iter().all(|row| row.open_ticks == price));
    }
}
