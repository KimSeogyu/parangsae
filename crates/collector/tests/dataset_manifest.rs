//! Cucumber runner for `specs/features/collector/dataset_manifest.feature`.

use std::path::PathBuf;

use cucumber::{given, then, when, World as _};
use parangsae_collector::civil::CivilDay;
use parangsae_collector::store::{self, fact_path};
use parangsae_collector::vision::{parse_klines, TickDomain};
use parangsae_core::{Receipt, RunId, Symbol};

#[derive(Debug, Default, cucumber::World)]
struct ManifestWorld {
    dir: Option<PathBuf>,
    day: Option<CivilDay>,
    dataset: Option<String>,
    check: Option<String>,
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
    world.dir = Some(dir);
    world.day = Some(day);
    world.dataset = Some(dataset);
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
