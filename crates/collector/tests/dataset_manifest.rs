//! Cucumber runner for `specs/features/collector/dataset_manifest.feature`.

use std::path::PathBuf;

use cucumber::{given, then, when, World as _};
use parangsae_collector::civil::{CivilDay, DayRange, YearMonth};
use parangsae_collector::run::Provenance;
use parangsae_collector::store::{self, fact_path};
use parangsae_collector::version::{
    directory_digest, migrate_receipts, publish, DatasetId, PublishRequest,
};
use parangsae_collector::vision::{
    funding_window, missing_coverage, parse_funding, parse_klines, FundingFact, TickDomain,
};
use parangsae_core::{AvailableTs, Receipt, RunId, Symbol, UtcMicros};

#[derive(Debug, Default, cucumber::World)]
struct ManifestWorld {
    dir: Option<PathBuf>,
    day: Option<CivilDay>,
    dataset: Option<String>,
    check: Option<String>,
    funding: Option<FundingFact>,
    rejected: bool,
    digest: Option<String>,
    publish_error: Option<String>,
}

fn symbol() -> Symbol {
    Symbol::new("BTCUSDT").expect("symbol")
}

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/vision")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

#[given(regex = r#"^an archive ingest of "([^"]+)" with run id "([0-9a-f]{32})"$"#)]
fn given_archive(world: &mut ManifestWorld, name: String, run_hex: String) {
    let run = RunId::parse_hex(&run_hex).expect("run id");
    let parsed = parse_klines(
        &fixture(&name),
        &symbol(),
        parangsae_collector::normalize::UM_KLINE_CLOCK,
        TickDomain::Price,
        Receipt::Archive {
            fetched_at_run: run,
        },
        true,
    )
    .expect("parse");
    let day = parangsae_collector::civil::day_of(parsed.rows[0].open_time).expect("day");
    let dir = tempfile::tempdir().expect("temp").keep();
    store::upsert_klines(&dir, "um-kline", parsed.rows).expect("upsert");
    world.dir = Some(dir);
    world.day = Some(day);
    world.dataset = Some("um-kline".to_owned());
}

#[then(regex = r#"^every stored kline receipt is archive run "([0-9a-f]{32})"$"#)]
fn then_run(world: &mut ManifestWorld, run_hex: String) {
    let run = RunId::parse_hex(&run_hex).expect("run id");
    let rows = stored(world);
    assert!(!rows.is_empty());
    assert!(rows
        .iter()
        .all(|row| row.receipt.archive_run() == Some(run)));
}

#[then("no stored kline receipt exposes a time")]
fn then_no_time(world: &mut ManifestWorld) {
    let rows = stored(world);
    assert!(rows
        .iter()
        .all(|row| row.receipt.live_received_ts().is_none()));
}

#[given(regex = r#"^a v1 parquet partition for dataset "([^"]+)""#)]
fn given_v1(world: &mut ManifestWorld, dataset: String) {
    let run = RunId::from_bytes([2; 16]);
    let parsed = parse_klines(
        &fixture("um_kline_2021-06-01_headerless.csv"),
        &symbol(),
        parangsae_collector::normalize::UM_KLINE_CLOCK,
        TickDomain::Price,
        Receipt::Archive {
            fetched_at_run: run,
        },
        true,
    )
    .expect("parse");
    let day = parangsae_collector::civil::day_of(parsed.rows[0].open_time).expect("day");
    let dir = tempfile::tempdir().expect("temp").keep();
    let path = fact_path(&dir, &dataset, "BTCUSDT", day);
    let received = vec![1_i64; parsed.rows.len()];
    store::write_v1_klines(&path, &parsed.rows, &received).expect("v1");
    world.digest = Some(directory_digest(&dir).expect("digest"));
    world.dir = Some(dir);
    world.day = Some(day);
    world.dataset = Some(dataset);
}

#[when("receipts are migrated without --apply")]
fn when_dry_run(world: &mut ManifestWorld) {
    let dir = world.dir.as_ref().expect("dir");
    let provenance = Provenance {
        git_sha: "0123456789abcdef0123456789abcdef01234567".to_owned(),
        dirty: false,
    };
    migrate_receipts(dir, false, &provenance).expect("dry run");
}

#[then("the data directory is unchanged")]
fn then_unchanged(world: &mut ManifestWorld) {
    let dir = world.dir.as_ref().expect("dir");
    let now = directory_digest(dir).expect("digest");
    assert_eq!(world.digest.as_deref(), Some(now.as_str()));
}

#[when("the partition is loaded")]
fn when_loaded(world: &mut ManifestWorld) {
    let dir = world.dir.as_ref().expect("dir");
    let day = world.day.expect("day");
    let dataset = world.dataset.as_deref().expect("dataset");
    match store::load_klines(dir, dataset, "BTCUSDT", day) {
        Ok(_) => world.check = None,
        Err(err) => world.check = err.check_name().map(str::to_owned),
    }
}

#[then(regex = r#"^the check is "([^"]+)""#)]
fn then_check(world: &mut ManifestWorld, name: String) {
    assert_eq!(world.check.as_deref(), Some(name.as_str()));
}

#[given(regex = r#"^a funding row from "([^"]+)" with available_ts before its event_ts$"#)]
fn given_early_funding(world: &mut ManifestWorld, name: String) {
    let parsed = parse_funding(
        &fixture(&name),
        &symbol(),
        Receipt::Archive {
            fetched_at_run: RunId::from_bytes([3; 16]),
        },
    )
    .expect("parse");
    let mut row = parsed.rows.into_iter().next().expect("row");
    let event_us = row.event_ts.micros().micros();
    row.available_ts = AvailableTs::from_micros(UtcMicros::from_micros(event_us - 1));
    let day = parangsae_collector::civil::day_of(row.funding_time).expect("day");
    let dir = tempfile::tempdir().expect("temp").keep();
    world.dir = Some(dir);
    world.day = Some(day);
    world.dataset = Some("funding-rate".to_owned());
    world.funding = Some(row);
    world.rejected = false;
}

#[then(regex = r#"^the funding archive is rejected with fault "([^"]+)""#)]
fn then_funding_rejected(world: &mut ManifestWorld, name: String) {
    let row = world.funding.clone().expect("funding row");
    let day = world.day.expect("day");
    let month = YearMonth::new(day.year(), day.month()).expect("month");
    match funding_window(std::slice::from_ref(&row), month) {
        Err(faults) => {
            assert!(faults.iter().any(|fault| fault.name == name), "{faults:?}");
            world.rejected = true;
            world.check = Some(name);
        }
        Ok(_) => panic!("archive was accepted"),
    }
    let dir = world.dir.as_ref().expect("dir");
    let day = world.day.expect("day");
    let path = fact_path(dir, "funding-rate", "BTCUSDT", day);
    assert!(!path.exists(), "rejected archive must not be written");
}

#[then("the row available_ts is left before its event_ts")]
fn then_not_clamped(world: &mut ManifestWorld) {
    assert!(world.rejected);
    let row = world.funding.as_ref().expect("funding row");
    assert!(row.available_ts.micros() < row.event_ts.micros());
}

#[given("a gap cache that says missing_file for that day")]
fn given_stale_gaps(world: &mut ManifestWorld) {
    let dir = world.dir.as_ref().expect("dir");
    let day = world.day.expect("day");
    let dataset = world.dataset.as_deref().expect("dataset");
    let gap = missing_coverage(day, false);
    store::write_gaps(dir, dataset, "BTCUSDT", day, &[gap]).expect("gaps");
}

#[when("the day is published")]
fn when_published(world: &mut ManifestWorld) {
    let dir = world.dir.clone().expect("dir");
    let day = world.day.expect("day");
    let dataset = DatasetId::parse("binance-vision.um.um-kline.BTCUSDT").expect("dataset id");
    let range = DayRange::new(day, day).expect("range");
    let provenance = Provenance {
        git_sha: "0123456789abcdef0123456789abcdef01234567".to_owned(),
        dirty: false,
    };
    let run = RunId::from_bytes([4; 16]);
    world.publish_error = match publish(&PublishRequest {
        data_dir: &dir,
        dataset: &dataset,
        range,
        provenance: &provenance,
        run,
        built_at: UtcMicros::from_micros(1),
    }) {
        Ok(_) => None,
        Err(err) => Some(err.name().to_owned()),
    };
}

#[then(regex = r#"^publish fails with "([^"]+)""#)]
fn then_publish_fails(world: &mut ManifestWorld, name: String) {
    assert_eq!(world.publish_error.as_deref(), Some(name.as_str()));
}

fn stored(world: &ManifestWorld) -> Vec<parangsae_collector::vision::KlineFact> {
    let dir = world.dir.as_ref().expect("dir");
    let day = world.day.expect("day");
    let dataset = world.dataset.as_deref().expect("dataset");
    store::load_klines(dir, dataset, "BTCUSDT", day).expect("load")
}

#[tokio::main]
async fn main() {
    let feature = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../specs/features/collector/dataset_manifest.feature");
    ManifestWorld::cucumber().run_and_exit(feature).await;
}
