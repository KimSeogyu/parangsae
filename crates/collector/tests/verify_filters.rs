//! C12: `verify --dataset/--from/--to` narrows scope and still runs the checks inside it.

use std::collections::{BTreeMap, HashMap};
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use parangsae_collector::civil::{CivilDay, DayRange};
use parangsae_collector::fetch::MapBlob;
use parangsae_collector::run::{self, RunRequest, VerifyFilter, VerifyReport};
use parangsae_collector::store;
use parangsae_collector::vision::{download_url, list_url, sha256_bytes, Cadence, VisionDataset};
use parangsae_core::{AvailableTs, EventTs, Ratio, ReceivedTs, Symbol, UtcMicros};
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

fn day(year: i32, month: u8, day_num: u8) -> Result<CivilDay, Box<dyn std::error::Error>> {
    CivilDay::new(year, month, day_num)
        .ok_or_else(|| format!("bad day {year}-{month}-{day_num}").into())
}

fn fixture(name: &str) -> Result<String, Box<dyn std::error::Error>> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/vision")
        .join(name);
    Ok(std::fs::read_to_string(path)?)
}

fn day_csv(day: CivilDay, minutes: i64) -> String {
    let start_ms = day.start_us().micros() / 1_000;
    let mut csv = String::new();
    for index in 0..minutes {
        let open = start_ms + index * 60_000;
        let close = open + 59_999;
        csv.push_str(&format!("{open},10,10,10,10,0,{close},0,0,0,0,0\n"));
    }
    csv
}

fn zip_csv(name: &str, csv: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer = ZipWriter::new(&mut cursor);
        writer
            .start_file(name, SimpleFileOptions::default())
            .map_err(|err| format!("zip entry: {err}"))?;
        writer
            .write_all(csv.as_bytes())
            .map_err(|err| format!("zip bytes: {err}"))?;
        writer
            .finish()
            .map_err(|err| format!("zip finish: {err}"))?;
    }
    Ok(cursor.into_inner())
}

fn checksum_bytes(file_name: &str, zip_bytes: &[u8]) -> Vec<u8> {
    let hex: String = sha256_bytes(zip_bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("{hex}  {file_name}\n").into_bytes()
}

fn empty_list() -> Vec<u8> {
    b"<ListBucketResult><IsTruncated>false</IsTruncated></ListBucketResult>".to_vec()
}

fn listing(entries: &[(String, u64)]) -> Vec<u8> {
    let mut xml = String::from("<ListBucketResult><IsTruncated>false</IsTruncated>");
    for (key, size) in entries {
        xml.push_str(&format!(
            "<Contents><Key>{key}</Key><Size>{size}</Size></Contents>"
        ));
    }
    xml.push_str("</ListBucketResult>");
    xml.into_bytes()
}

struct ArchiveFile {
    prefix: String,
    file_name: String,
    csv: String,
}

fn prefix(dataset: VisionDataset, cadence: Cadence) -> Result<String, Box<dyn std::error::Error>> {
    dataset
        .prefixes("BTCUSDT")
        .into_iter()
        .find(|(kind, _)| *kind == cadence)
        .map(|(_, text)| text)
        .ok_or_else(|| format!("missing {cadence:?} prefix for {dataset}").into())
}

fn ingest(
    dir: &Path,
    dataset: VisionDataset,
    from: CivilDay,
    to: CivilDay,
    files: &[ArchiveFile],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut map = HashMap::new();
    for (_, listed) in dataset.prefixes("BTCUSDT") {
        map.insert(list_url(&listed, "")?, empty_list());
    }
    let mut by_prefix: BTreeMap<String, Vec<(String, u64)>> = BTreeMap::new();
    for file in files {
        let zip_bytes = zip_csv("rows.csv", &file.csv)?;
        let checksum = checksum_bytes(&file.file_name, &zip_bytes);
        let key = format!("{}{}", file.prefix, file.file_name);
        let checksum_key = format!("{key}.CHECKSUM");
        by_prefix
            .entry(file.prefix.clone())
            .or_default()
            .push((key.clone(), zip_bytes.len() as u64));
        by_prefix
            .entry(file.prefix.clone())
            .or_default()
            .push((checksum_key.clone(), checksum.len() as u64));
        map.insert(download_url(&key), zip_bytes);
        map.insert(download_url(&checksum_key), checksum);
    }
    for (listed, entries) in &by_prefix {
        map.insert(list_url(listed, "")?, listing(entries));
    }
    let request = RunRequest {
        datasets: vec![dataset],
        symbol: Symbol::new("BTCUSDT").map_err(|err| err.to_string())?,
        range: DayRange::new(from, to)?,
        data_dir: dir.to_path_buf(),
        jobs: 1,
        min_interval: Duration::from_millis(0),
        fetched_at: ReceivedTs::from_micros(UtcMicros::from_micros(10)),
    };
    let summary = run::run_vision(&request, &MapBlob::new(map))?
        .into_iter()
        .next()
        .ok_or("ingest returned no dataset")?
        .1;
    if summary.faults != 0 || summary.conflicts != 0 {
        return Err(format!("ingest failed: {summary}").into());
    }
    if summary.rows_inserted + summary.rows_unchanged == 0 {
        return Err(format!("ingest stored nothing: {summary}").into());
    }
    Ok(())
}

fn corrupt_open(
    dir: &Path,
    dataset: &str,
    day: CivilDay,
    open_us: i64,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut rows = store::load_klines(dir, dataset, "BTCUSDT", day)?;
    let Some(row) = rows
        .iter_mut()
        .find(|row| row.open_time.micros() == open_us)
    else {
        return Err(format!("no {dataset} row at {open_us} on {day}").into());
    };
    row.open_ticks = row.open_ticks.checked_add(1).ok_or("open ticks overflow")?;
    let path = store::fact_path(dir, dataset, "BTCUSDT", day);
    std::fs::remove_file(path)?;
    store::upsert_klines(dir, dataset, rows)?;
    Ok(())
}

fn corrupt_one_funding_gap(dir: &Path) -> Result<CivilDay, Box<dyn std::error::Error>> {
    let mut cursor = day(2026, 9, 1)?;
    for _ in 0..30 {
        let mut gaps = store::load_gaps(dir, "funding-rate", "BTCUSDT", cursor)?;
        if !gaps.is_empty() {
            gaps[0].detail.push_str(" corrupted");
            store::write_gaps(dir, "funding-rate", "BTCUSDT", cursor, &gaps)?;
            return Ok(cursor);
        }
        cursor = cursor.next();
    }
    Err("funding fixture stored no gap".into())
}

fn funding_csv(days: &[CivilDay]) -> String {
    let mut csv = String::from("calc_time,funding_interval_hours,last_funding_rate\n");
    for day in days {
        for hour in [0, 8, 16] {
            let millis = day.start_us().micros() / 1_000 + hour * 3_600_000;
            csv.push_str(&format!("{millis},8,0.00010000\n"));
        }
    }
    csv
}

fn funding_archive(days: &[CivilDay]) -> Result<ArchiveFile, Box<dyn std::error::Error>> {
    Ok(ArchiveFile {
        prefix: prefix(VisionDataset::FundingRate, Cadence::Monthly)?,
        file_name: "BTCUSDT-fundingRate-2026-09.zip".to_owned(),
        csv: funding_csv(days),
    })
}

fn legacy_funding_scope(dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let path = dir.join("manifest/objects.jsonl");
    let legacy = std::fs::read_to_string(&path)?
        .lines()
        .map(|line| -> Result<String, serde_json::Error> {
            let mut object: serde_json::Value = serde_json::from_str(line)?;
            object.as_object_mut().unwrap().remove("materialized_days");
            serde_json::to_string(&object)
        })
        .collect::<Result<Vec<_>, _>>()?
        .join("\n");
    std::fs::write(path, format!("{legacy}\n"))?;
    Ok(())
}

fn funding_gap_day_injection_is_detected(legacy: bool) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let september1 = day(2026, 9, 1)?;
    let september2 = day(2026, 9, 2)?;
    let september3 = day(2026, 9, 3)?;
    ingest(
        dir.path(),
        VisionDataset::FundingRate,
        september1,
        september2,
        &[funding_archive(&[september1])?],
    )?;
    if legacy {
        legacy_funding_scope(dir.path())?;
    }
    let filter = VerifyFilter::parse(&["funding-rate"], Some("2026-09-02"), Some("2026-09-02"))?;
    let before = run::verify(dir.path(), &filter)?;
    assert!(before.ok(), "{before}");
    let gaps = store::load_gaps(dir.path(), "funding-rate", "BTCUSDT", september2)?;
    assert!(!gaps.is_empty());
    let mut injected =
        store::load_funding(dir.path(), "funding-rate", "BTCUSDT", september1)?[0].clone();
    injected.raw_calc_time = september2.start_us();
    injected.funding_time = september2.start_us();
    injected.event_ts = EventTs::from_micros(september2.start_us());
    injected.available_ts = AvailableTs::from_micros(september2.start_us());
    store::upsert_funding(dir.path(), "funding-rate", vec![injected.clone()])?;
    assert_eq!(
        store::load_gaps(dir.path(), "funding-rate", "BTCUSDT", september2)?,
        gaps
    );
    for selected in [filter, VerifyFilter::all()] {
        let report = run::verify(dir.path(), &selected)?;
        assert!(
            !report.ok(),
            "injected row on an expected empty day was accepted: {report}"
        );
        assert!(
            report
                .fault_lines
                .iter()
                .any(|line| line == "unexpected funding parquet rows 2026-09-02"),
            "{report}"
        );
    }
    if !legacy {
        injected.raw_calc_time = september3.start_us();
        injected.funding_time = september3.start_us();
        injected.event_ts = EventTs::from_micros(september3.start_us());
        injected.available_ts = AvailableTs::from_micros(september3.start_us());
        store::upsert_funding(dir.path(), "funding-rate", vec![injected])?;
        let outside = run::verify(
            dir.path(),
            &VerifyFilter::parse(&["funding-rate"], Some("2026-09-03"), Some("2026-09-03"))?,
        )?;
        assert!(outside.ok(), "never-requested day was included: {outside}");
    }
    Ok(())
}

#[test]
fn funding_parquet_gap_day_injection_fails_with_recorded_scope(
) -> Result<(), Box<dyn std::error::Error>> {
    funding_gap_day_injection_is_detected(false)
}

#[test]
fn funding_parquet_gap_day_injection_fails_with_legacy_scope(
) -> Result<(), Box<dyn std::error::Error>> {
    funding_gap_day_injection_is_detected(true)
}

fn funding_parquet_change_is_detected(change: &str) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let september1 = day(2026, 9, 1)?;
    let mut files = [funding_archive(&[september1])?];
    if change == "extra" {
        files[0].csv.push_str(&format!(
            "{},8,0.00010000\n",
            september1.start_us().micros() / 1_000,
        ));
    }
    ingest(
        dir.path(),
        VisionDataset::FundingRate,
        september1,
        september1,
        &files,
    )?;
    let filter = VerifyFilter::parse(&["funding-rate"], Some("2026-09-01"), Some("2026-09-01"))?;
    let before = run::verify(dir.path(), &filter)?;
    assert!(before.ok(), "{before}");
    let path = store::fact_path(dir.path(), "funding-rate", "BTCUSDT", september1);
    let mut rows = store::load_funding(dir.path(), "funding-rate", "BTCUSDT", september1)?;
    match change {
        "partition" => rows.clear(),
        "row" => {
            rows.remove(0);
        }
        "rate" => rows[0].rate = Ratio::from_ticks(rows[0].rate.ticks() + 1),
        "available" => {
            rows[0].available_ts = AvailableTs::from_micros(UtcMicros::from_micros(
                rows[0].available_ts.micros().micros() + 1,
            ))
        }
        "event" => {
            rows[0].event_ts = EventTs::from_micros(UtcMicros::from_micros(
                rows[0].event_ts.micros().micros() + 1,
            ))
        }
        "schema" => rows[0].schema_version += 1,
        "extra" => {
            let mut extra = rows[0].clone();
            extra.funding_time =
                UtcMicros::from_micros(extra.funding_time.micros() + 14_400_000_000);
            rows.push(extra);
        }
        _ => return Err(format!("unknown funding change {change}").into()),
    }
    std::fs::remove_file(path)?;
    if !rows.is_empty() {
        store::upsert_funding(dir.path(), "funding-rate", rows)?;
    }

    for excluded in [
        VerifyFilter::parse(&["um-kline"], None, None)?,
        VerifyFilter::parse(&["funding-rate"], Some("2026-09-02"), Some("2026-09-02"))?,
    ] {
        let report = run::verify(dir.path(), &excluded)?;
        assert!(report.ok(), "{change} outside filter: {report}");
    }
    for selected in [filter, VerifyFilter::all()] {
        let report = run::verify(dir.path(), &selected)?;
        assert!(!report.ok(), "funding {change} was accepted: {report}");
        let prefix = match change {
            "partition" | "row" => "missing parquet row ",
            "extra" => "unexpected funding parquet rows ",
            _ => "conflict ",
        };
        assert!(
            report
                .fault_lines
                .iter()
                .any(|line| line.starts_with(prefix)),
            "{report}"
        );
    }
    Ok(())
}

#[test]
fn missing_funding_parquet_partition_fails_verify() -> Result<(), Box<dyn std::error::Error>> {
    funding_parquet_change_is_detected("partition")
}

#[test]
fn missing_funding_parquet_row_fails_verify() -> Result<(), Box<dyn std::error::Error>> {
    funding_parquet_change_is_detected("row")
}

#[test]
fn altered_funding_parquet_payload_fails_verify() -> Result<(), Box<dyn std::error::Error>> {
    for change in ["rate", "available", "event", "schema", "extra"] {
        funding_parquet_change_is_detected(change)?;
    }
    Ok(())
}

#[test]
fn funding_parquet_partial_month_scope_unions_and_retains_missing_days(
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let september1 = day(2026, 9, 1)?;
    let september2 = day(2026, 9, 2)?;
    let files = [funding_archive(&[september1, september2])?];
    ingest(
        dir.path(),
        VisionDataset::FundingRate,
        september1,
        september1,
        &files,
    )?;
    let partial = run::verify(dir.path(), &VerifyFilter::all())?;
    assert!(partial.ok(), "{partial}");
    let unmaterialized = run::verify(
        dir.path(),
        &VerifyFilter::parse(&["funding-rate"], Some("2026-09-02"), Some("2026-09-02"))?,
    )?;
    assert!(unmaterialized.ok(), "{unmaterialized}");

    ingest(
        dir.path(),
        VisionDataset::FundingRate,
        september2,
        september2,
        &files,
    )?;
    let extended = run::verify(dir.path(), &VerifyFilter::all())?;
    assert!(extended.ok(), "{extended}");
    std::fs::remove_file(store::fact_path(
        dir.path(),
        "funding-rate",
        "BTCUSDT",
        september1,
    ))?;
    let deleted = run::verify(dir.path(), &VerifyFilter::all())?;
    assert!(!deleted.ok(), "{deleted}");
    assert_eq!(deleted.fault_lines.len(), 3, "{deleted}");
    let intact = run::verify(
        dir.path(),
        &VerifyFilter::parse(&["funding-rate"], Some("2026-09-02"), Some("2026-09-02"))?,
    )?;
    assert!(intact.ok(), "{intact}");
    Ok(())
}

#[test]
fn funding_parquet_legacy_scope_remains_conservative() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let september1 = day(2026, 9, 1)?;
    let september2 = day(2026, 9, 2)?;
    let files = [funding_archive(&[september1, september2])?];
    ingest(
        dir.path(),
        VisionDataset::FundingRate,
        september1,
        september1,
        &files,
    )?;
    legacy_funding_scope(dir.path())?;

    for _ in 0..2 {
        let scoped = run::verify(
            dir.path(),
            &VerifyFilter::parse(&["funding-rate"], Some("2026-09-01"), Some("2026-09-01"))?,
        )?;
        assert!(scoped.ok(), "{scoped}");
        let full = run::verify(dir.path(), &VerifyFilter::all())?;
        assert!(!full.ok(), "unknown legacy scope was narrowed: {full}");
        assert_eq!(full.fault_lines.len(), 3, "{full}");
        ingest(
            dir.path(),
            VisionDataset::FundingRate,
            september1,
            september1,
            &files,
        )?;
    }
    Ok(())
}

fn conflict(open_us: i64) -> String {
    format!("conflict {open_us}")
}

fn scope_contains(report: &VerifyReport, phrase: &str) {
    let text = report.to_string();
    assert!(
        text.contains(phrase),
        "scope phrase {phrase:?} missing from {text}"
    );
}

#[test]
fn headed_and_headerless_archives_ingest_and_verify_as_the_same_market_facts(
) -> Result<(), Box<dyn std::error::Error>> {
    const HEADER: &str = "open_time,open,high,low,close,volume,close_time,quote_volume,count,taker_buy_volume,taker_buy_quote_volume,ignore\n";
    let june1 = day(2021, 6, 1)?;
    let bare = day_csv(june1, 1);
    let headed = format!("{HEADER}{bare}");
    for (monthly_csv, daily_csv, first_schema) in [(&bare, &headed, 1), (&headed, &bare, 2)] {
        let dir = tempfile::tempdir()?;
        let monthly = [ArchiveFile {
            prefix: prefix(VisionDataset::UmKline, Cadence::Monthly)?,
            file_name: "BTCUSDT-1m-2021-06.zip".to_owned(),
            csv: monthly_csv.clone(),
        }];
        let mut daily = [ArchiveFile {
            prefix: prefix(VisionDataset::UmKline, Cadence::Daily)?,
            file_name: "BTCUSDT-1m-2021-06-01.zip".to_owned(),
            csv: daily_csv.clone(),
        }];
        ingest(dir.path(), VisionDataset::UmKline, june1, june1, &monthly)?;
        let path = store::fact_path(dir.path(), "um-kline", "BTCUSDT", june1);
        let original = std::fs::read(&path)?;
        ingest(dir.path(), VisionDataset::UmKline, june1, june1, &daily)?;
        let report = run::verify(dir.path(), &VerifyFilter::all())?;
        assert!(report.ok(), "{report}");
        assert_eq!(report.summary.archives, 2);
        assert_eq!(std::fs::read(&path)?, original, "stored provenance changed");
        let stored = store::load_klines(dir.path(), "um-kline", "BTCUSDT", june1)?;
        assert_eq!(stored[0].schema_version, first_schema);
        let manifest = parangsae_collector::manifest::ManifestLog::open(dir.path())?;
        let mut source_schemas: Vec<_> =
            manifest.active().iter().map(|o| o.schema_version).collect();
        source_schemas.sort_unstable();
        assert_eq!(source_schemas, [1, 2]);

        daily[0].csv = daily_csv.replace(",10,10,10,10,", ",11,11,11,11,");
        let changed = ingest(dir.path(), VisionDataset::UmKline, june1, june1, &daily);
        assert!(changed.unwrap_err().to_string().contains("conflicts=1"));
        assert!(!run::verify(dir.path(), &VerifyFilter::all())?.ok());
        assert_eq!(
            std::fs::read(&path)?,
            original,
            "conflict overwrote stored facts"
        );
    }
    Ok(())
}

#[test]
fn dataset_filter_skips_other_datasets_and_still_fails_the_selected_one(
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let june1 = day(2021, 6, 1)?;
    let september = day(2026, 9, 1)?;
    ingest(
        dir.path(),
        VisionDataset::UmKline,
        june1,
        june1,
        &[ArchiveFile {
            prefix: prefix(VisionDataset::UmKline, Cadence::Daily)?,
            file_name: "BTCUSDT-1m-2021-06-01.zip".to_owned(),
            csv: fixture("um_kline_2021-06-01_headerless.csv")?,
        }],
    )?;
    ingest(
        dir.path(),
        VisionDataset::FundingRate,
        september,
        day(2026, 9, 30)?,
        &[ArchiveFile {
            prefix: prefix(VisionDataset::FundingRate, Cadence::Monthly)?,
            file_name: "BTCUSDT-fundingRate-2026-09.zip".to_owned(),
            csv: fixture("funding_2026-09-01.csv")?,
        }],
    )?;
    let open_us = june1.start_us().micros();
    corrupt_open(dir.path(), "um-kline", june1, open_us)?;
    let gap_day = corrupt_one_funding_gap(dir.path())?;

    let funding = run::verify(
        dir.path(),
        &VerifyFilter::parse(&["funding-rate"], None, None)?,
    )?;
    assert!(
        funding
            .fault_lines
            .iter()
            .all(|line| !line.starts_with("conflict ")),
        "um partition was checked: {}",
        funding.fault_lines.join("; ")
    );
    assert!(
        funding
            .fault_lines
            .iter()
            .any(|line| line == &format!("funding_slot gap mismatch {gap_day}")),
        "funding check inside the filter was skipped: {}",
        funding.fault_lines.join("; ")
    );
    assert_eq!(funding.summary.archives, 1, "{funding}");
    assert_eq!(funding.archives_in_scope, 1);
    assert_eq!(funding.archives_total, 2);
    assert_eq!(funding.datasets_label, "funding-rate");
    scope_contains(
        &funding,
        "datasets=funding-rate, days=all, archives=1/2 in scope",
    );
    let by_alias = run::verify(
        dir.path(),
        &VerifyFilter::parse(&["fundingRate"], None, None)?,
    )?;
    assert_eq!(by_alias.fault_lines, funding.fault_lines);
    assert_eq!(by_alias.summary.archives, funding.summary.archives);

    let um = run::verify(dir.path(), &VerifyFilter::parse(&["um-kline"], None, None)?)?;
    assert_eq!(um.fault_lines, vec![conflict(open_us)]);
    assert!(um
        .fault_lines
        .iter()
        .all(|line| !line.contains("funding_slot")));
    assert!(!um.ok());
    let by_folder = run::verify(dir.path(), &VerifyFilter::parse(&["klines"], None, None)?)?;
    assert_eq!(by_folder.fault_lines, um.fault_lines);
    Ok(())
}

#[test]
fn date_filter_compares_only_that_days_rows_and_still_fails_inside_the_range(
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let june1 = day(2021, 6, 1)?;
    let june2 = day(2021, 6, 2)?;
    let mut monthly = fixture("um_kline_2021-06-01_headerless.csv")?;
    if !monthly.ends_with('\n') {
        monthly.push('\n');
    }
    monthly.push_str(&day_csv(june2, 1));
    ingest(
        dir.path(),
        VisionDataset::UmKline,
        june1,
        june2,
        &[
            ArchiveFile {
                prefix: prefix(VisionDataset::UmKline, Cadence::Monthly)?,
                file_name: "BTCUSDT-1m-2021-06.zip".to_owned(),
                csv: monthly,
            },
            ArchiveFile {
                prefix: prefix(VisionDataset::UmKline, Cadence::Daily)?,
                file_name: "BTCUSDT-1m-2021-06-01.zip".to_owned(),
                csv: fixture("um_kline_2021-06-01_headerless.csv")?,
            },
        ],
    )?;
    let june1_open = june1.start_us().micros();
    let june2_open = june2.start_us().micros();
    corrupt_open(dir.path(), "um-kline", june1, june1_open)?;
    corrupt_open(dir.path(), "um-kline", june2, june2_open)?;

    let full = run::verify(dir.path(), &VerifyFilter::all())?;
    let full_lines = vec![
        conflict(june1_open),
        conflict(june2_open),
        conflict(june1_open),
    ];
    assert_eq!(full.fault_lines, full_lines, "{full}");
    assert_eq!(full.summary.faults, full_lines.len() as u64);
    assert_eq!(full.summary.conflicts, 0);
    assert_eq!(
        full.summary.first_fault.as_deref(),
        Some(full_lines[0].as_str())
    );
    assert_eq!(full.datasets_label, "all");
    assert_eq!(full.days_label, "all");
    assert_eq!(full.archives_in_scope, 2);
    assert_eq!(full.archives_total, 2);
    scope_contains(&full, "datasets=all, days=all, archives=2/2 in scope");

    let june1_only = run::verify(
        dir.path(),
        &VerifyFilter::parse(&[] as &[&str], Some("2021-06-01"), Some("2021-06-01"))?,
    )?;
    assert_eq!(
        june1_only.fault_lines,
        vec![conflict(june1_open), conflict(june1_open)],
        "in-range partition was not checked: {june1_only}"
    );
    assert!(june1_only
        .fault_lines
        .iter()
        .all(|line| line != &conflict(june2_open)));
    assert_eq!(june1_only.days_label, "2021-06-01..2021-06-01");
    assert_eq!(june1_only.archives_in_scope, 2);
    scope_contains(
        &june1_only,
        "datasets=all, days=2021-06-01..2021-06-01, archives=2/2 in scope",
    );

    let june2_only = run::verify(
        dir.path(),
        &VerifyFilter::parse(&[] as &[&str], Some("2021-06-02"), Some("2021-06-02"))?,
    )?;
    assert_eq!(
        june2_only.fault_lines,
        vec![conflict(june2_open)],
        "in-range monthly row was not checked: {june2_only}"
    );
    assert_eq!(june2_only.archives_in_scope, 1);
    assert_eq!(june2_only.archives_total, 2);

    let from_only = run::verify(
        dir.path(),
        &VerifyFilter::parse(&[] as &[&str], Some("2021-06-02"), None)?,
    )?;
    assert_eq!(from_only.fault_lines, vec![conflict(june2_open)]);
    assert_eq!(from_only.days_label, "2021-06-02..");

    let to_only = run::verify(
        dir.path(),
        &VerifyFilter::parse(&[] as &[&str], None, Some("2021-06-01"))?,
    )?;
    assert_eq!(
        to_only.fault_lines,
        vec![conflict(june1_open), conflict(june1_open)]
    );
    assert_eq!(to_only.days_label, "..2021-06-01");
    Ok(())
}

#[test]
fn partial_month_verify_checks_only_materialized_days_and_still_detects_deletion(
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let june1 = day(2021, 6, 1)?;
    let june2 = day(2021, 6, 2)?;
    let files = [ArchiveFile {
        prefix: prefix(VisionDataset::UmKline, Cadence::Monthly)?,
        file_name: "BTCUSDT-1m-2021-06.zip".to_owned(),
        csv: format!("{}{}", day_csv(june1, 1), day_csv(june2, 1)),
    }];
    ingest(dir.path(), VisionDataset::UmKline, june1, june1, &files)?;

    let scoped = VerifyFilter::parse(&["um-kline"], Some("2021-06-01"), Some("2021-06-01"))?;
    let control = run::verify(dir.path(), &scoped)?;
    assert!(control.ok(), "{control}");
    let full = run::verify(dir.path(), &VerifyFilter::all())?;
    assert!(
        full.ok(),
        "never-ingested June 2 is not deleted data: {full}"
    );
    assert_eq!(full.partition_loads, ["um-kline/BTCUSDT/2021-06-01"]);

    std::fs::remove_file(store::fact_path(dir.path(), "um-kline", "BTCUSDT", june1))?;
    for filter in [&scoped, &VerifyFilter::all()] {
        let deleted = run::verify(dir.path(), filter)?;
        assert!(!deleted.ok(), "{deleted}");
        assert_eq!(
            deleted.fault_lines,
            [format!("missing parquet row {}", june1.start_us().micros())]
        );
    }
    Ok(())
}

#[test]
fn partial_month_repeated_and_overlapping_ingests_preserve_coverage(
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let june1 = day(2021, 6, 1)?;
    let june2 = day(2021, 6, 2)?;
    let june3 = day(2021, 6, 3)?;
    let june4 = day(2021, 6, 4)?;
    let files = [ArchiveFile {
        prefix: prefix(VisionDataset::UmKline, Cadence::Monthly)?,
        file_name: "BTCUSDT-1m-2021-06.zip".to_owned(),
        csv: [june1, june2, june3, june4]
            .into_iter()
            .map(|day| day_csv(day, 1))
            .collect(),
    }];
    for (from, to) in [(june1, june1), (june3, june4), (june3, june4)] {
        ingest(dir.path(), VisionDataset::UmKline, from, to, &files)?;
    }
    let disjoint = run::verify(dir.path(), &VerifyFilter::all())?;
    assert!(disjoint.ok(), "{disjoint}");
    assert_eq!(
        disjoint.partition_loads,
        [
            "um-kline/BTCUSDT/2021-06-01",
            "um-kline/BTCUSDT/2021-06-03",
            "um-kline/BTCUSDT/2021-06-04",
        ]
    );

    ingest(dir.path(), VisionDataset::UmKline, june2, june3, &files)?;
    let overlapping = run::verify(dir.path(), &VerifyFilter::all())?;
    assert!(overlapping.ok(), "{overlapping}");
    assert_eq!(overlapping.partition_loads.len(), 4);

    corrupt_open(dir.path(), "um-kline", june1, june1.start_us().micros())?;
    std::fs::remove_file(store::fact_path(dir.path(), "um-kline", "BTCUSDT", june3))?;
    let damaged = run::verify(dir.path(), &VerifyFilter::all())?;
    assert_eq!(
        damaged.fault_lines,
        [
            conflict(june1.start_us().micros()),
            format!("missing parquet row {}", june3.start_us().micros()),
        ]
    );
    assert!(!damaged.ok());
    Ok(())
}

#[test]
fn partial_month_conflicting_ingest_does_not_hide_its_failed_partition(
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let june1 = day(2021, 6, 1)?;
    let june2 = day(2021, 6, 2)?;
    ingest(
        dir.path(),
        VisionDataset::UmKline,
        june2,
        june2,
        &[ArchiveFile {
            prefix: prefix(VisionDataset::UmKline, Cadence::Daily)?,
            file_name: "BTCUSDT-1m-2021-06-02.zip".to_owned(),
            csv: day_csv(june2, 1).replace(",10,10,10,10,", ",20,20,20,20,"),
        }],
    )?;
    let failed = ingest(
        dir.path(),
        VisionDataset::UmKline,
        june1,
        june2,
        &[ArchiveFile {
            prefix: prefix(VisionDataset::UmKline, Cadence::Monthly)?,
            file_name: "BTCUSDT-1m-2021-06.zip".to_owned(),
            csv: format!("{}{}", day_csv(june1, 1), day_csv(june2, 1)),
        }],
    );
    assert!(failed.unwrap_err().to_string().contains("conflicts=1"));
    let verified = run::verify(dir.path(), &VerifyFilter::all())?;
    assert_eq!(verified.fault_lines, [conflict(june2.start_us().micros())]);
    assert!(!verified.ok());
    Ok(())
}

#[test]
fn partial_month_interrupted_write_remains_in_the_verification_scope(
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let june1 = day(2021, 6, 1)?;
    let june2 = day(2021, 6, 2)?;
    let blocked = store::fact_path(dir.path(), "um-kline", "BTCUSDT", june2);
    std::fs::create_dir_all(&blocked)?;
    let failed = ingest(
        dir.path(),
        VisionDataset::UmKline,
        june1,
        june2,
        &[ArchiveFile {
            prefix: prefix(VisionDataset::UmKline, Cadence::Monthly)?,
            file_name: "BTCUSDT-1m-2021-06.zip".to_owned(),
            csv: format!("{}{}", day_csv(june1, 1), day_csv(june2, 1)),
        }],
    );
    assert!(failed.is_err());
    std::fs::remove_dir_all(blocked)?;
    let verified = run::verify(dir.path(), &VerifyFilter::all())?;
    assert_eq!(
        verified.fault_lines,
        [format!("missing parquet row {}", june2.start_us().micros())]
    );
    assert!(!verified.ok());
    Ok(())
}

#[test]
fn partial_month_legacy_manifest_does_not_infer_coverage_from_remaining_files(
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let june1 = day(2021, 6, 1)?;
    let june2 = day(2021, 6, 2)?;
    let files = [ArchiveFile {
        prefix: prefix(VisionDataset::UmKline, Cadence::Monthly)?,
        file_name: "BTCUSDT-1m-2021-06.zip".to_owned(),
        csv: format!("{}{}", day_csv(june1, 1), day_csv(june2, 1)),
    }];
    ingest(dir.path(), VisionDataset::UmKline, june1, june1, &files)?;
    let path = dir.path().join("manifest/objects.jsonl");
    let legacy = std::fs::read_to_string(&path)?
        .lines()
        .map(|line| -> Result<String, serde_json::Error> {
            let mut object: serde_json::Value = serde_json::from_str(line)?;
            object.as_object_mut().unwrap().remove("materialized_days");
            serde_json::to_string(&object)
        })
        .collect::<Result<Vec<_>, _>>()?
        .join("\n");
    std::fs::write(&path, format!("{legacy}\n"))?;
    std::fs::remove_file(store::fact_path(dir.path(), "um-kline", "BTCUSDT", june1))?;
    let deleted = run::verify(dir.path(), &VerifyFilter::all())?;
    assert_eq!(deleted.fault_lines.len(), 2);
    assert!(deleted.fault_lines.contains(&format!(
        "missing parquet row {}",
        june1.start_us().micros()
    )));

    ingest(dir.path(), VisionDataset::UmKline, june1, june1, &files)?;
    let legacy = run::verify(dir.path(), &VerifyFilter::all())?;
    assert_eq!(
        legacy.fault_lines,
        [format!("missing parquet row {}", june2.start_us().micros())]
    );
    assert!(!legacy.ok());
    Ok(())
}

#[test]
fn missing_in_scope_kline_partition_fails_verify() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let june1 = day(2021, 6, 1)?;
    ingest(
        dir.path(),
        VisionDataset::UmKline,
        june1,
        june1,
        &[ArchiveFile {
            prefix: prefix(VisionDataset::UmKline, Cadence::Daily)?,
            file_name: "BTCUSDT-1m-2021-06-01.zip".to_owned(),
            csv: fixture("um_kline_2021-06-01_headerless.csv")?,
        }],
    )?;
    let filter = VerifyFilter::parse(&["um-kline"], Some("2021-06-01"), Some("2021-06-01"))?;
    let before = run::verify(dir.path(), &filter)?;
    assert!(before.ok(), "{before}");
    assert!(before.fault_lines.is_empty(), "{before}");

    std::fs::remove_file(store::fact_path(dir.path(), "um-kline", "BTCUSDT", june1))?;

    let report = run::verify(dir.path(), &filter)?;
    let expected: Vec<String> = [0, 60_000_000, 120_000_000]
        .into_iter()
        .map(|offset| format!("missing parquet row {}", june1.start_us().micros() + offset))
        .collect();
    assert!(!report.ok(), "{report}");
    assert_eq!(report.fault_lines, expected);
    assert_eq!(report.summary.faults, 3);
    assert_eq!(
        report.summary.first_fault.as_deref(),
        Some(expected[0].as_str())
    );

    let output = Command::new(collect_bin())
        .args(["verify", "--data-dir"])
        .arg(dir.path())
        .output()?;
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("faults=3"), "{stdout}");
    assert!(stdout.contains(&expected[0]), "{stdout}");
    Ok(())
}

#[test]
fn each_fact_partition_loads_once() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let days = [day(2021, 6, 1)?, day(2021, 6, 2)?, day(2021, 6, 3)?];
    let mut monthly = String::new();
    let mut files = vec![ArchiveFile {
        prefix: prefix(VisionDataset::UmKline, Cadence::Monthly)?,
        file_name: "BTCUSDT-1m-2021-06.zip".to_owned(),
        csv: String::new(),
    }];
    for civil in days {
        let csv = day_csv(civil, 2);
        monthly.push_str(&csv);
        files.push(ArchiveFile {
            prefix: prefix(VisionDataset::UmKline, Cadence::Daily)?,
            file_name: format!("BTCUSDT-1m-{civil}.zip"),
            csv,
        });
    }
    files[0].csv = monthly;
    ingest(dir.path(), VisionDataset::UmKline, days[0], days[2], &files)?;
    let report = run::verify(dir.path(), &VerifyFilter::all())?;
    assert!(report.ok(), "{report}");
    assert_eq!(report.summary.archives, 4, "{report}");
    let mut loads = report.partition_loads.clone();
    loads.sort();
    let expected: Vec<String> = days
        .iter()
        .map(|civil| format!("um-kline/BTCUSDT/{civil}"))
        .collect();
    assert_eq!(loads, expected, "partitions were reloaded: {loads:?}");
    Ok(())
}

#[test]
fn unknown_dataset_and_reversed_range_are_usage_errors() -> Result<(), Box<dyn std::error::Error>> {
    match VerifyFilter::parse(&["not-a-dataset"], None, None) {
        Err(err) => {
            let text = err.to_string();
            assert!(text.contains("unknown dataset not-a-dataset"), "{text}");
        }
        Ok(filter) => {
            return Err(format!("unknown dataset was accepted: {filter:?}").into());
        }
    }
    match VerifyFilter::parse(&[] as &[&str], Some("2021-06-02"), Some("2021-06-01")) {
        Err(err) => {
            let text = err.to_string();
            assert!(text.contains("reversed"), "{text}");
        }
        Ok(filter) => {
            return Err(format!("reversed range was accepted: {filter:?}").into());
        }
    }
    match VerifyFilter::parse(&[] as &[&str], Some("2021-13-01"), None) {
        Err(err) => assert!(err.to_string().contains("bad date"), "{}", err),
        Ok(filter) => return Err(format!("bad date was accepted: {filter:?}").into()),
    }
    let all = VerifyFilter::parse(&["all"], None, None)?;
    assert_eq!(all, VerifyFilter::all());
    match VerifyFilter::parse(&["all", "funding-rate"], None, None) {
        Err(err) => assert!(err.to_string().contains("all"), "{err}"),
        Ok(filter) => return Err(format!("mixed all was accepted: {filter:?}").into()),
    }
    Ok(())
}

#[test]
fn cli_unknown_dataset_and_reversed_range_exit_2() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().to_str().ok_or("data dir is not utf-8")?;
    let bin = collect_bin();
    let unknown = Command::new(bin)
        .args(["verify", "--data-dir", path, "--dataset", "not-a-dataset"])
        .output()?;
    assert_usage(&unknown, "unknown dataset not-a-dataset")?;
    let reversed = Command::new(bin)
        .args([
            "verify",
            "--data-dir",
            path,
            "--from",
            "2021-06-02",
            "--to",
            "2021-06-01",
        ])
        .output()?;
    assert_usage(&reversed, "reversed")?;
    Ok(())
}

fn collect_bin() -> &'static str {
    if let Some(path) = option_env!("CARGO_BIN_EXE_parangsae_collect") {
        return path;
    }
    if let Some(path) = option_env!("CARGO_BIN_EXE_parangsae-collect") {
        return path;
    }
    panic!("parangsae-collect was not exposed to the integration test");
}

fn assert_usage(
    output: &std::process::Output,
    needle: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let stdout = String::from_utf8(output.stdout.clone())?;
    let stderr = String::from_utf8(output.stderr.clone())?;
    assert_eq!(
        output.status.code(),
        Some(2),
        "stdout {stdout} stderr {stderr}"
    );
    assert!(stderr.contains(needle), "{stderr}");
    assert!(
        !stdout.contains("faults="),
        "usage error printed a verify summary: {stdout}"
    );
    Ok(())
}
