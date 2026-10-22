//! Cucumber runner for `specs/features/collector`. A failed step exits non-zero.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "cucumber steps are not #[test] items, so clippy.toml does not cover them"
)]

use std::collections::HashMap;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use cucumber::{given, then, when, World as _};
use parangsae_collector::civil::{parse_day, CivilDay, DayRange, YearMonth};
use parangsae_collector::fetch::MapBlob;
use parangsae_collector::run::{self, RunRequest, VerifyFilter, VerifyReport};
use parangsae_collector::store;
use parangsae_collector::vision::{
    book_day_gaps, book_row_faults, classify, collapse_by, download_url, funding_window,
    kline_day_gaps, kline_row_faults, list_url, metrics_row_faults, missing_coverage, parse_book,
    parse_checksum, parse_funding, parse_klines, parse_metrics, require_sha256, sha256_bytes,
    Cadence, Classified, FundingFact, Gap, KlineFact, MetricsFact, TickDomain, VisionDataset,
    BOOK_GAP_LIMIT_US,
};
use parangsae_collector::Error;
use parangsae_core::{Amount, AvailableTs, EventTs, Qty, ReceivedTs, Symbol, TimeUnit, UtcMicros};

#[derive(Debug, Default)]
enum Loaded {
    #[default]
    Empty,
    Klines(Vec<KlineFact>),
    Funding(Vec<FundingFact>),
    Metrics(Vec<MetricsFact>),
}

#[derive(Debug, Default, cucumber::World)]
struct VisionWorld {
    loaded: Loaded,
    book: Vec<parangsae_collector::vision::BookFact>,
    check: Option<String>,
    faults: Vec<String>,
    level_schema: u32,
    names: Vec<(String, String)>,
    data_dir: Option<PathBuf>,
    open_ticks: i64,
    unchanged: u64,
    conflicts: u64,
    gap_fails: Option<bool>,
    day: Option<CivilDay>,
    close_unit: Option<TimeUnit>,
    verify_dir: Option<tempfile::TempDir>,
    verify_report: Option<VerifyReport>,
    verify_error: Option<String>,
}

fn symbol() -> Symbol {
    Symbol::new("BTCUSDT").expect("symbol")
}

fn received() -> ReceivedTs {
    ReceivedTs::from_micros(UtcMicros::from_micros(1))
}

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/vision")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

fn fixture_bytes(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/vision")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

fn note_error(world: &mut VisionWorld, err: Error) {
    world.check = err.check_name().map(str::to_owned);
    world.faults.clear();
    world.loaded = Loaded::Empty;
}

#[given(regex = r#"^the kline fixture "([^"]+)" as USD-M milliseconds$"#)]
fn given_um(world: &mut VisionWorld, name: String) {
    world.close_unit = Some(TimeUnit::Millis);
    match parse_klines(
        &fixture(&name),
        &symbol(),
        parangsae_collector::normalize::UM_KLINE_CLOCK,
        TickDomain::Price,
        received(),
    ) {
        Ok(parsed) => world.loaded = Loaded::Klines(parsed.rows),
        Err(err) => note_error(world, err),
    }
}

#[given(regex = r#"^the kline fixture "([^"]+)" as spot on (\d{4}-\d{2}-\d{2})$"#)]
fn given_spot(world: &mut VisionWorld, name: String, day_text: String) {
    let day = parse_day(&day_text).expect("day");
    world.close_unit = Some(parangsae_collector::normalize::spot_kline_clock(day).expected());
    match parse_klines(
        &fixture(&name),
        &symbol(),
        parangsae_collector::normalize::spot_kline_clock(day),
        TickDomain::Price,
        received(),
    ) {
        Ok(parsed) => world.loaded = Loaded::Klines(parsed.rows),
        Err(err) => note_error(world, err),
    }
}

#[given(regex = r#"^the kline fixture "([^"]+)" as spot microseconds$"#)]
fn given_spot_micros(world: &mut VisionWorld, name: String) {
    world.close_unit = Some(TimeUnit::Micros);
    match parse_klines(
        &fixture(&name),
        &symbol(),
        parangsae_collector::normalize::SPOT_KLINE_CLOCK,
        TickDomain::Price,
        received(),
    ) {
        Ok(parsed) => world.loaded = Loaded::Klines(parsed.rows),
        Err(err) => note_error(world, err),
    }
}

fn check_with(world: &mut VisionWorld, unit: TimeUnit) {
    let Loaded::Klines(rows) = &world.loaded else {
        panic!(
            "expected kline rows, check {}",
            world.check.as_deref().unwrap_or("none")
        );
    };
    world.faults = kline_row_faults(rows, unit)
        .into_iter()
        .map(|fault| fault.name.to_owned())
        .collect();
}

#[when("the kline rows are checked")]
fn check_klines(world: &mut VisionWorld) {
    let unit = world.close_unit.expect("declared close tick");
    check_with(world, unit);
}

#[when("the kline rows are checked as a one-millisecond close tick")]
fn check_millis_tick(world: &mut VisionWorld) {
    check_with(world, TimeUnit::Millis);
}

#[when("the kline rows are checked as a one-microsecond close tick")]
fn check_micros_tick(world: &mut VisionWorld) {
    check_with(world, TimeUnit::Micros);
}

#[then("there is no row fault")]
fn no_fault(world: &mut VisionWorld) {
    assert!(
        world.faults.is_empty(),
        "expected no fault, got {:?}",
        world.faults
    );
}

#[then(regex = r#"^the first fault is "([^"]+)"$"#)]
fn first_fault(world: &mut VisionWorld, name: String) {
    assert_eq!(
        world.faults.first().map(String::as_str),
        Some(name.as_str())
    );
}

#[then(regex = r#"^a fault is "([^"]+)"$"#)]
fn some_fault(world: &mut VisionWorld, name: String) {
    assert!(
        world.faults.iter().any(|fault| fault == &name),
        "{:?}",
        world.faults
    );
}

#[then(regex = r#"^parsing fails the "([^"]+)" check$"#)]
fn parse_fails(world: &mut VisionWorld, name: String) {
    assert_eq!(world.check.as_deref(), Some(name.as_str()));
}

#[given(regex = r#"^the listing fixture "([^"]+)""#)]
fn given_listing(world: &mut VisionWorld, name: String) {
    world.names.clear();
    let xml = fixture(&name);
    let page = parangsae_collector::vision::list_page(&xml).expect("listing");
    for object in page.objects {
        let file_name = object
            .key
            .rsplit('/')
            .next()
            .unwrap_or(&object.key)
            .to_owned();
        let kind = match classify(VisionDataset::UmKline, "BTCUSDT", &file_name).expect("classify")
        {
            Classified::Archive(_) => "archive",
            Classified::Checksum { .. } => "checksum",
            Classified::Quarantine { .. } => "quarantine",
        };
        world.names.push((file_name, kind.to_owned()));
    }
}

#[when("the names are classified for um-kline BTCUSDT")]
fn classified(_world: &mut VisionWorld) {}

#[then(regex = r#"^"([^"]+)" is quarantined$"#)]
fn quarantined(world: &mut VisionWorld, name: String) {
    assert!(world
        .names
        .iter()
        .any(|(file, kind)| file == &name && kind == "quarantine"));
}

#[then(regex = r#"^"([^"]+)" is an archive$"#)]
fn is_archive(world: &mut VisionWorld, name: String) {
    assert!(world
        .names
        .iter()
        .any(|(file, kind)| file == &name && kind == "archive"));
}

#[given(regex = r#"^the zip fixture "([^"]+)" and checksum "([^"]+)""#)]
fn given_zip(world: &mut VisionWorld, zip_name: String, checksum_name: String) {
    let bytes = fixture_bytes(&zip_name);
    let text = fixture(&checksum_name);
    match parse_checksum(&text, &zip_name).and_then(|digest| require_sha256(&bytes, digest)) {
        Ok(()) => world.check = None,
        Err(err) => world.check = err.check_name().map(str::to_owned),
    }
    let _ = sha256_bytes(&bytes);
}

#[then("the checksum check fails")]
fn checksum_fails(world: &mut VisionWorld) {
    assert_eq!(world.check.as_deref(), Some("checksum"));
}

#[then("the checksum matches")]
fn checksum_ok(world: &mut VisionWorld) {
    assert!(world.check.is_none(), "{:?}", world.check);
}

#[given(regex = r#"^the funding fixture "([^"]+)""#)]
fn given_funding(world: &mut VisionWorld, name: String) {
    match parse_funding(&fixture(&name), &symbol(), received()) {
        Ok(parsed) => world.loaded = Loaded::Funding(parsed.rows),
        Err(err) => note_error(world, err),
    }
}

#[then(regex = r"^the first funding raw time is (\d+) microseconds$")]
fn raw_funding(world: &mut VisionWorld, micros: i64) {
    let Loaded::Funding(rows) = &world.loaded else {
        panic!("funding")
    };
    assert_eq!(rows[0].raw_calc_time.micros(), micros);
}

#[then(regex = r"^the first funding slot is (\d+) microseconds$")]
fn slot_funding(world: &mut VisionWorld, micros: i64) {
    let Loaded::Funding(rows) = &world.loaded else {
        panic!("funding")
    };
    assert_eq!(rows[0].funding_time.micros(), micros);
    assert_eq!(
        rows[0].event_ts.micros().micros(),
        rows[0].raw_calc_time.micros()
    );
    assert_eq!(
        rows[0].available_ts.micros().micros(),
        rows[0].funding_time.micros()
    );
}

#[when("the September 2026 funding slots are checked")]
fn funding_slots(world: &mut VisionWorld) {
    let Loaded::Funding(rows) = &world.loaded else {
        panic!("funding")
    };
    let gaps = funding_window(rows, YearMonth::new(2026, 9).unwrap()).expect("slots");
    world.faults = gaps
        .into_iter()
        .map(|gap| gap.start.micros().to_string())
        .collect();
}

#[then(regex = r"^the funding row at calc_time (\d+) has (-?\d+) rate ticks$")]
fn funding_rate_ticks(world: &mut VisionWorld, calc_ms: i64, ticks: i64) {
    let Loaded::Funding(rows) = &world.loaded else {
        panic!("funding parse failed: {:?}", world.check);
    };
    let calc_us = calc_ms.checked_mul(1_000).expect("calc_time fits");
    let found = rows
        .iter()
        .find(|row| row.raw_calc_time.micros() == calc_us)
        .map(|row| row.rate.ticks());
    assert_eq!(found, Some(ticks));
}

#[then("January 2020 has no funding_slot gap")]
fn january_2020_funding_has_no_gap(world: &mut VisionWorld) {
    let Loaded::Funding(rows) = &world.loaded else {
        panic!("funding parse failed: {:?}", world.check);
    };
    let gaps = funding_window(rows, YearMonth::new(2020, 1).expect("month")).expect("slots");
    assert!(gaps.is_empty(), "{gaps:?}");
}

#[then(regex = r"^a funding gap covers (\d+) plus 8 hours$")]
fn funding_gap(world: &mut VisionWorld, start: i64) {
    let expected = start + 8 * 3_600_000_000;
    assert!(world
        .faults
        .iter()
        .any(|slot| slot == &expected.to_string()));
}

#[given(regex = r#"^the metrics fixture "([^"]+)""#)]
fn given_metrics(world: &mut VisionWorld, name: String) {
    match parse_metrics(&fixture(&name), &symbol(), received()) {
        Ok(parsed) => world.loaded = Loaded::Metrics(parsed.rows),
        Err(err) => note_error(world, err),
    }
}

#[when("the metrics rows are ordered")]
fn order_metrics(world: &mut VisionWorld) {
    let rows = match std::mem::take(&mut world.loaded) {
        Loaded::Metrics(rows) => rows,
        other @ (Loaded::Empty | Loaded::Klines(_) | Loaded::Funding(_)) => {
            panic!("metrics, got {other:?}")
        }
    };
    world.faults = metrics_row_faults(&rows)
        .into_iter()
        .map(|fault| fault.name.to_owned())
        .collect();
    world.loaded =
        Loaded::Metrics(collapse_by(rows, |row| row.create_time.micros()).expect("collapse"));
}

#[then("the metrics rows increase by create_time")]
fn metrics_sorted(world: &mut VisionWorld) {
    let Loaded::Metrics(rows) = &world.loaded else {
        panic!("metrics")
    };
    assert!(rows
        .windows(2)
        .all(|pair| pair[0].create_time < pair[1].create_time));
}

#[then("the metrics publication lag is unverified")]
fn lag_unverified(world: &mut VisionWorld) {
    let Loaded::Metrics(rows) = &world.loaded else {
        panic!("metrics")
    };
    assert!(rows.iter().all(|row| !row.available_lag_verified));
}

#[then("available_ts is create_time plus 5 minutes")]
fn metrics_available(world: &mut VisionWorld) {
    let Loaded::Metrics(rows) = &world.loaded else {
        panic!("metrics")
    };
    for row in rows {
        assert_eq!(
            row.available_ts.micros().micros(),
            row.create_time.micros() + 300_000_000
        );
    }
}

#[then(regex = r"^the metrics row count is (\d+)$")]
fn metrics_count(world: &mut VisionWorld, count: usize) {
    let Loaded::Metrics(rows) = &world.loaded else {
        panic!("metrics")
    };
    assert_eq!(rows.len(), count);
}

#[given(regex = r#"^the book fixture "([^"]+)""#)]
fn given_book(world: &mut VisionWorld, name: String) {
    world.faults.clear();
    match parse_book(&fixture(&name), &symbol(), received()) {
        Ok(parsed) => world.book = parsed.rows,
        Err(err) => note_error(world, err),
    }
}

#[when("the book rows are checked")]
fn check_book(world: &mut VisionWorld) {
    world.faults = book_row_faults(&mut world.book)
        .into_iter()
        .map(|fault| fault.name.to_owned())
        .collect();
    world.level_schema = world.book.first().map(|row| row.level_schema).unwrap_or(0);
}

#[then(regex = r"^the book level schema is (\d+)$")]
fn book_schema(world: &mut VisionWorld, schema: u32) {
    assert!(world.book.iter().all(|row| row.level_schema == schema));
}

#[when(regex = r"^the book gaps on (\d{4}-\d{2}-\d{2}) are checked$")]
fn book_gaps(world: &mut VisionWorld, day_text: String) {
    let day = parse_day(&day_text).expect("day");
    let _ = book_row_faults(&mut world.book);
    let gaps = book_day_gaps(&world.book, day);
    world.faults = gaps
        .into_iter()
        .map(|gap| (gap.end.micros() - gap.start.micros()).to_string())
        .collect();
}

#[then("a book gap is longer than 90 seconds")]
fn book_gap_long(world: &mut VisionWorld) {
    assert!(world
        .faults
        .iter()
        .any(|span| span.parse::<i64>().unwrap() > BOOK_GAP_LIMIT_US));
}

#[when(regex = r"^coverage for missing book day (\d{4}-\d{2}-\d{2}) is judged$")]
fn coverage(world: &mut VisionWorld, day_text: String) {
    let day = parse_day(&day_text).expect("day");
    let gap = missing_coverage(day, true);
    world.gap_fails = Some(gap.kind.fails_command());
}

#[then("the gap does not fail the command")]
fn gap_ok(world: &mut VisionWorld) {
    assert_eq!(world.gap_fails, Some(false));
}

#[then("the gap fails the command")]
fn gap_fails(world: &mut VisionWorld) {
    assert_eq!(world.gap_fails, Some(true));
}

#[given(regex = r"^a complete BTCUSDT 1m day on (\d{4}-\d{2}-\d{2})$")]
fn complete_day(world: &mut VisionWorld, day_text: String) {
    let day = parse_day(&day_text).expect("day");
    world.day = Some(day);
    let mut rows = Vec::new();
    for index in 0..1_440 {
        let open = day.start_us().micros() + index * 60_000_000;
        let close = open + 60_000_000 - 1_000;
        rows.push(bar(open, close, 1));
    }
    world.loaded = Loaded::Klines(rows);
}

#[then("that day has no kline gap")]
fn no_gap(world: &mut VisionWorld) {
    let day = world.day.expect("day");
    let Loaded::Klines(rows) = &world.loaded else {
        panic!("klines")
    };
    assert!(kline_day_gaps(rows, day).is_empty());
}

#[when("the last bar is removed")]
fn drop_last(world: &mut VisionWorld) {
    if let Loaded::Klines(rows) = &mut world.loaded {
        rows.pop();
    }
}

#[then("that day has a kline gap")]
fn has_gap(world: &mut VisionWorld) {
    let day = world.day.expect("day");
    let Loaded::Klines(rows) = &world.loaded else {
        panic!("klines")
    };
    let gaps: Vec<Gap> = kline_day_gaps(rows, day);
    assert_eq!(gaps.len(), 1);
}

#[given("a stored BTCUSDT kline in a fresh data directory")]
fn stored_kline(world: &mut VisionWorld) {
    let dir = tempfile::tempdir().expect("temp").keep();
    let row = bar(1_788_220_800_000_000, 1_788_220_859_999_000, 42);
    world.open_ticks = row.open_ticks;
    store::upsert_klines(&dir, "um-kline", vec![row]).expect("insert");
    world.data_dir = Some(dir);
}

#[when("the same kline is written again")]
fn write_same(world: &mut VisionWorld) {
    let dir = world.data_dir.as_ref().expect("dir");
    let counts = store::upsert_klines(
        dir,
        "um-kline",
        vec![bar(
            1_788_220_800_000_000,
            1_788_220_859_999_000,
            world.open_ticks,
        )],
    )
    .expect("rewrite");
    world.unchanged = counts.unchanged;
    world.conflicts = counts.conflicts;
}

#[then("the write counts as unchanged")]
fn unchanged(world: &mut VisionWorld) {
    assert_eq!(world.unchanged, 1);
    assert_eq!(world.conflicts, 0);
}

#[when("a different open is written for that key")]
fn write_different(world: &mut VisionWorld) {
    let dir = world.data_dir.as_ref().expect("dir");
    let counts = store::upsert_klines(
        dir,
        "um-kline",
        vec![bar(
            1_788_220_800_000_000,
            1_788_220_859_999_000,
            world.open_ticks + 1,
        )],
    )
    .expect("conflict");
    world.conflicts = counts.conflicts;
    world.unchanged = counts.unchanged;
}

#[then("the write counts as a conflict")]
fn conflicted(world: &mut VisionWorld) {
    assert_eq!(world.conflicts, 1);
}

#[then("the stored open is unchanged")]
fn stored_open(world: &mut VisionWorld) {
    let dir = world.data_dir.as_ref().expect("dir");
    let day = CivilDay::new(2026, 9, 1).unwrap();
    let rows = store::load_klines(dir, "um-kline", "BTCUSDT", day).expect("load");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].open_ticks, world.open_ticks);
}

fn verify_prefix(dataset: VisionDataset, cadence: Cadence) -> Result<String, String> {
    dataset
        .prefixes("BTCUSDT")
        .into_iter()
        .find(|(kind, _)| *kind == cadence)
        .map(|(_, text)| text)
        .ok_or_else(|| format!("missing prefix for {dataset}"))
}

fn verify_zip(csv: &str) -> Result<Vec<u8>, String> {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer = zip::ZipWriter::new(&mut cursor);
        writer
            .start_file("rows.csv", zip::write::SimpleFileOptions::default())
            .map_err(|err| err.to_string())?;
        writer
            .write_all(csv.as_bytes())
            .map_err(|err| err.to_string())?;
        writer.finish().map_err(|err| err.to_string())?;
    }
    Ok(cursor.into_inner())
}

fn verify_ingest(
    dir: &Path,
    dataset: VisionDataset,
    from: CivilDay,
    to: CivilDay,
    prefix: &str,
    file_name: &str,
    csv: &str,
) -> Result<(), String> {
    let zip_bytes = verify_zip(csv)?;
    let digest = sha256_bytes(&zip_bytes);
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    let checksum = format!("{hex}  {file_name}\n").into_bytes();
    let key = format!("{prefix}{file_name}");
    let checksum_key = format!("{key}.CHECKSUM");
    let mut map = HashMap::new();
    for (_, listed) in dataset.prefixes("BTCUSDT") {
        let url = list_url(&listed, "").map_err(|err| err.to_string())?;
        map.insert(
            url,
            b"<ListBucketResult><IsTruncated>false</IsTruncated></ListBucketResult>".to_vec(),
        );
    }
    let mut xml = String::from("<ListBucketResult><IsTruncated>false</IsTruncated>");
    for (entry, size) in [
        (&key, zip_bytes.len() as u64),
        (&checksum_key, checksum.len() as u64),
    ] {
        xml.push_str(&format!(
            "<Contents><Key>{entry}</Key><Size>{size}</Size></Contents>"
        ));
    }
    xml.push_str("</ListBucketResult>");
    let list = list_url(prefix, "").map_err(|err| err.to_string())?;
    map.insert(list, xml.into_bytes());
    map.insert(download_url(&key), zip_bytes);
    map.insert(download_url(&checksum_key), checksum);
    let request = RunRequest {
        datasets: vec![dataset],
        symbol: symbol(),
        range: DayRange::new(from, to).map_err(|err| err.to_string())?,
        data_dir: dir.to_path_buf(),
        jobs: 1,
        min_interval: Duration::from_millis(0),
        fetched_at: received(),
    };
    let summary = run::run_vision(&request, &MapBlob::new(map))
        .map_err(|err| err.to_string())?
        .into_iter()
        .next()
        .ok_or_else(|| "ingest returned no dataset".to_owned())?
        .1;
    if summary.faults != 0 || summary.conflicts != 0 || summary.rows_inserted == 0 {
        return Err(format!("ingest failed: {summary}"));
    }
    Ok(())
}

fn run_verify(world: &mut VisionWorld, filter: Result<VerifyFilter, Error>) {
    world.verify_report = None;
    world.verify_error = None;
    let filter = match filter {
        Ok(filter) => filter,
        Err(err) => {
            world.verify_error = Some(err.to_string());
            return;
        }
    };
    let Some(dir) = world.verify_dir.as_ref() else {
        panic!("verify store was not prepared");
    };
    match run::verify(dir.path(), &filter) {
        Ok(report) => world.verify_report = Some(report),
        Err(err) => world.verify_error = Some(err.to_string()),
    }
}

#[given("a temp store with the 2021-06-01 um-kline fixture and the September 2026 funding fixture")]
fn given_verify_store(world: &mut VisionWorld) {
    let dir = tempfile::tempdir().expect("temp");
    let june1 = CivilDay::new(2021, 6, 1).expect("june");
    let september = CivilDay::new(2026, 9, 1).expect("september");
    let september_end = CivilDay::new(2026, 9, 30).expect("september end");
    verify_ingest(
        dir.path(),
        VisionDataset::UmKline,
        june1,
        june1,
        &verify_prefix(VisionDataset::UmKline, Cadence::Daily).expect("prefix"),
        "BTCUSDT-1m-2021-06-01.zip",
        &fixture("um_kline_2021-06-01_headerless.csv"),
    )
    .expect("um ingest");
    verify_ingest(
        dir.path(),
        VisionDataset::FundingRate,
        september,
        september_end,
        &verify_prefix(VisionDataset::FundingRate, Cadence::Monthly).expect("prefix"),
        "BTCUSDT-fundingRate-2026-09.zip",
        &fixture("funding_2026-09-01.csv"),
    )
    .expect("funding ingest");
    world.verify_dir = Some(dir);
}

#[given("the stored um-kline open on 2021-06-01 differs from the archive")]
fn corrupt_stored_um(world: &mut VisionWorld) {
    let dir = world.verify_dir.as_ref().expect("store").path();
    let june1 = CivilDay::new(2021, 6, 1).expect("june");
    let mut rows = store::load_klines(dir, "um-kline", "BTCUSDT", june1).expect("rows");
    let open_us = june1.start_us().micros();
    let row = rows
        .iter_mut()
        .find(|row| row.open_time.micros() == open_us)
        .expect("opening bar");
    row.open_ticks = row.open_ticks.checked_add(1).expect("open ticks");
    let path = store::fact_path(dir, "um-kline", "BTCUSDT", june1);
    std::fs::remove_file(path).expect("remove partition");
    store::upsert_klines(dir, "um-kline", rows).expect("rewrite");
}

#[when(regex = r#"^verify is limited to dataset "([^"]+)"$"#)]
fn when_verify_dataset(world: &mut VisionWorld, name: String) {
    run_verify(world, VerifyFilter::parse(&[name], None, None));
}

#[when(
    regex = r#"^verify is limited to dataset "([^"]+)" from "(\d{4}-\d{2}-\d{2})" to "(\d{4}-\d{2}-\d{2})"$"#
)]
fn when_verify_dataset_range(world: &mut VisionWorld, name: String, from: String, to: String) {
    run_verify(
        world,
        VerifyFilter::parse(std::slice::from_ref(&name), Some(&from), Some(&to)),
    );
}

#[when(regex = r#"^verify is asked for dataset "([^"]+)"$"#)]
fn when_verify_asked(world: &mut VisionWorld, name: String) {
    run_verify(world, VerifyFilter::parse(&[name], None, None));
}

#[when(regex = r#"^verify is limited from "(\d{4}-\d{2}-\d{2})" to "(\d{4}-\d{2}-\d{2})"$"#)]
fn when_verify_range(world: &mut VisionWorld, from: String, to: String) {
    run_verify(
        world,
        VerifyFilter::parse(&[] as &[&str], Some(&from), Some(&to)),
    );
}

#[then("verify reports no faults")]
fn then_verify_clean(world: &mut VisionWorld) {
    let report = world
        .verify_report
        .as_ref()
        .unwrap_or_else(|| panic!("verify error: {:?}", world.verify_error));
    assert!(
        report.fault_lines.is_empty(),
        "{}",
        report.fault_lines.join("; ")
    );
    assert!(report.ok(), "{report}");
}

#[then("verify checked only the funding-rate archives")]
fn then_funding_only(world: &mut VisionWorld) {
    let report = world.verify_report.as_ref().expect("report");
    assert_eq!(report.summary.archives, 1, "{report}");
    assert_eq!(report.archives_in_scope, 1);
    assert_eq!(report.archives_total, 2);
    assert_eq!(report.datasets_label, "funding-rate");
    assert!(report
        .to_string()
        .contains("datasets=funding-rate, days=all, archives=1/2 in scope"));
}

#[then("verify reports a conflict fault")]
fn then_verify_conflict(world: &mut VisionWorld) {
    let report = world
        .verify_report
        .as_ref()
        .unwrap_or_else(|| panic!("verify error: {:?}", world.verify_error));
    assert!(
        report
            .fault_lines
            .iter()
            .any(|line| line.starts_with("conflict ")),
        "{report}"
    );
    assert!(!report.ok());
}

#[then("verify rejects the request as usage")]
fn then_verify_usage(world: &mut VisionWorld) {
    assert!(world.verify_report.is_none(), "{:?}", world.verify_report);
    let err = world.verify_error.as_deref().expect("usage error");
    assert!(
        err.contains("unknown dataset") || err.contains("reversed"),
        "{err}"
    );
}

fn bar(open: i64, close: i64, price: i64) -> KlineFact {
    let open_time = UtcMicros::from_micros(open);
    let close_time = UtcMicros::from_micros(close);
    KlineFact {
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
        received_ts: received(),
        schema_version: 1,
        tick_domain: TickDomain::Price,
    }
}

#[tokio::main]
async fn main() {
    let features = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../specs/features/collector");
    VisionWorld::cucumber().run_and_exit(features).await;
}
