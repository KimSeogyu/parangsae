//! Cucumber runner for `specs/features/collector/dataset_manifest.feature`.

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use cucumber::{given, then, when, World as _};
use parangsae_collector::civil::{CivilDay, DayRange, YearMonth};
use parangsae_collector::run::Provenance;
use parangsae_collector::store::{self, fact_path};
use parangsae_collector::version::{
    directory_digest, migrate_receipts, parse_version_id, preflight, publish, DatasetId, FoldSpec,
    ManifestDigest, PreflightError, PublishRequest, Requirement, Table, VersionIdError,
    VersionRoot, VersionStore,
};
use parangsae_collector::vision::{
    funding_window, missing_coverage, parse_funding, parse_klines, FundingFact, TickDomain,
};
use parangsae_core::{AvailableTs, Qty, Receipt, RunId, Symbol, UtcMicros};

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
    version_root: Option<String>,
    manifest_digest: Option<String>,
    load_error: Option<String>,
    version_error: Option<String>,
    preflight_folds: Vec<(String, Option<String>)>,
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
        Ok(published) => {
            world.version_root = Some(published.root.to_hex());
            world.manifest_digest = Some(published.manifest_digest.to_hex());
            None
        }
        Err(err) => Some(err.name().to_owned()),
    };
}

#[then(regex = r#"^publish fails with "([^"]+)""#)]
fn then_publish_fails(world: &mut ManifestWorld, name: String) {
    assert_eq!(world.publish_error.as_deref(), Some(name.as_str()));
}

#[given(regex = r#"^a published um-kline version of "([^"]+)""#)]
fn given_published(world: &mut ManifestWorld, name: String) {
    let run = RunId::from_bytes([1; 16]);
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
    let dataset = DatasetId::parse("binance-vision.um.um-kline.BTCUSDT").expect("dataset id");
    let range = DayRange::new(day, day).expect("range");
    let provenance = Provenance {
        git_sha: "0123456789abcdef0123456789abcdef01234567".to_owned(),
        dirty: false,
    };
    let published = publish(&PublishRequest {
        data_dir: &dir,
        dataset: &dataset,
        range,
        provenance: &provenance,
        run: RunId::from_bytes([4; 16]),
        built_at: UtcMicros::from_micros(1),
    })
    .expect("publish");
    world.version_root = Some(published.root.to_hex());
    world.manifest_digest = Some(published.manifest_digest.to_hex());
    world.dir = Some(dir);
    world.day = Some(day);
    world.dataset = Some("um-kline".to_owned());
}

#[when("the snapshot volume tick is increased")]
fn when_volume_increased(world: &mut ManifestWorld) {
    let dir = world.dir.clone().expect("dir");
    let day = world.day.expect("day");
    let path = snapshot_file(&dir);
    let mut permissions = std::fs::metadata(&path).expect("meta").permissions();
    permissions.set_mode(0o644);
    std::fs::set_permissions(&path, permissions).expect("chmod");
    let mut rows = store::load_klines(&dir, "um-kline", "BTCUSDT", day).expect("rows");
    let bumped = rows[0].volume.ticks().saturating_add(1);
    rows[0].volume = Qty::from_ticks(bumped).expect("qty");
    store::write_klines(&path, "um-kline", &rows).expect("rewrite snapshot");
    world.load_error = load_published(world).err().map(|err| err.name().to_owned());
}

#[when("the version is loaded")]
fn when_version_loaded(world: &mut ManifestWorld) {
    world.load_error = load_published(world).err().map(|err| err.name().to_owned());
}

#[when(regex = r#"^the version id "([^"]+)" is parsed$"#)]
fn when_version_parsed(world: &mut ManifestWorld, text: String) {
    world.version_error = match parse_version_id(&text) {
        Ok(_) => None,
        Err(VersionIdError::Alias) => Some("Alias".to_owned()),
        Err(VersionIdError::Malformed) => Some("Malformed".to_owned()),
    };
}

#[when(regex = r#"^the version is loaded on (\d{4}-\d{2}-\d{2})$"#)]
fn when_loaded_on(world: &mut ManifestWorld, date: String) {
    let mut parts = date.split('-');
    let year: i32 = parts.next().expect("year").parse().expect("year");
    let month: u8 = parts.next().expect("month").parse().expect("month");
    let day: u8 = parts.next().expect("day").parse().expect("day");
    let day = CivilDay::new(year, month, day).expect("day");
    world.load_error = load_on(world, day).err().map(|err| err.name().to_owned());
}

#[when("a value conflict is appended for an in-range key")]
fn when_unlisted(world: &mut ManifestWorld) {
    let dir = world.dir.as_ref().expect("dir");
    let day = world.day.expect("day");
    let rows = store::load_klines(dir, "um-kline", "BTCUSDT", day).expect("rows");
    let key = rows[0].open_time.micros();
    let path = dir.join("conflicts").join("um-kline.jsonl");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("conflicts dir");
    }
    let line = format!(
        "{{\"dataset\":\"um-kline\",\"key\":\"{key}\",\"rejected\":\"open=2\",\"stored\":\"open=1\",\"symbol\":\"BTCUSDT\"}}\n"
    );
    std::fs::write(&path, line).expect("conflict");
    world.load_error = load_published(world).err().map(|err| err.name().to_owned());
}

#[given("a value conflict is recorded for the first stored row")]
fn given_listed_conflict(world: &mut ManifestWorld) {
    let rows = stored(world);
    let key = rows[0].open_time.micros();
    let dir = world.dir.as_ref().expect("dir");
    let path = dir.join("conflicts").join("um-kline.jsonl");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("conflicts dir");
    }
    let line = format!(
        "{{\"dataset\":\"um-kline\",\"key\":\"{key}\",\"rejected\":\"open=2\",\"stored\":\"open=1\",\"symbol\":\"BTCUSDT\"}}\n"
    );
    std::fs::write(&path, line).expect("conflict");
}

#[when("preflight checks a fold that ends before the open gap and a fold that enters it")]
fn when_gap_folds(world: &mut ManifestWorld) {
    let verified = load_published(world).expect("load");
    let gap = verified
        .open_gaps()
        .iter()
        .find(|gap| gap.kind == "grid")
        .expect("grid gap")
        .clone();
    let id = DatasetId::parse("binance-vision.um.um-kline.BTCUSDT").expect("dataset id");
    let mut required = BTreeMap::new();
    required.insert(id.clone(), Requirement::Required);
    let mut tables = BTreeMap::new();
    tables.insert(id, &verified);
    let before = FoldSpec {
        fold_id: "before".to_owned(),
        warmup_start_us: gap.start_us - 60_000_000,
        window_start_us: gap.start_us - 60_000_000,
        window_end_us: gap.start_us,
        warmup_rows: BTreeMap::new(),
    };
    let inside = FoldSpec {
        fold_id: "inside".to_owned(),
        warmup_start_us: gap.start_us,
        window_start_us: gap.start_us,
        window_end_us: gap.start_us + 1,
        warmup_rows: BTreeMap::new(),
    };
    world.preflight_folds = preflight(&required, &tables, &[before, inside])
        .into_iter()
        .map(|stamp| {
            (
                stamp.fold_id,
                stamp.data_abort.map(|err| err.name().to_owned()),
            )
        })
        .collect();
}

#[when("preflight requires one warm-up row before the second stored row")]
fn when_warmup_fold(world: &mut ManifestWorld) {
    assert!(
        world.publish_error.is_none(),
        "publish failed: {:?}",
        world.publish_error
    );
    let verified = load_published(world).expect("load");
    let Table::Kline(rows) = verified.value() else {
        panic!("published table is not um-kline");
    };
    let gap_start = verified
        .open_gaps()
        .iter()
        .find(|gap| gap.kind == "grid")
        .expect("grid gap")
        .start_us;
    let id = DatasetId::parse("binance-vision.um.um-kline.BTCUSDT").expect("dataset id");
    assert!(verified.is_excluded(rows[0].open_time.micros()));
    let mut required = BTreeMap::new();
    required.insert(id.clone(), Requirement::Required);
    let mut tables = BTreeMap::new();
    tables.insert(id.clone(), &verified);
    let mut warmup_rows = BTreeMap::new();
    warmup_rows.insert(id, 1);
    let spec = FoldSpec {
        fold_id: "warm".to_owned(),
        warmup_start_us: rows[0].available_ts.micros().micros(),
        window_start_us: rows[1].available_ts.micros().micros(),
        window_end_us: gap_start,
        warmup_rows,
    };
    let stamps = preflight(&required, &tables, &[spec]);
    match &stamps[0].data_abort {
        Some(PreflightError::InsufficientWarmup {
            have_rows: 0,
            need_rows: 1,
            ..
        }) => {}
        other => panic!("warm-up count: {other:?}"),
    }
    world.preflight_folds = stamps
        .into_iter()
        .map(|stamp| {
            (
                stamp.fold_id,
                stamp.data_abort.map(|err| err.name().to_owned()),
            )
        })
        .collect();
}

#[then(regex = r#"^only the fold that enters the gap aborts with "([^"]+)""#)]
fn then_only_inside(world: &mut ManifestWorld, name: String) {
    assert_eq!(
        world.preflight_folds,
        vec![
            ("before".to_owned(), None),
            ("inside".to_owned(), Some(name)),
        ]
    );
}

#[then(regex = r#"^the fold aborts with "([^"]+)""#)]
fn then_fold_aborts(world: &mut ManifestWorld, name: String) {
    assert_eq!(world.preflight_folds.len(), 1);
    assert_eq!(world.preflight_folds[0].1.as_deref(), Some(name.as_str()));
}

#[then(regex = r#"^load fails with "([^"]+)""#)]
fn then_load_fails(world: &mut ManifestWorld, name: String) {
    assert_eq!(world.load_error.as_deref(), Some(name.as_str()));
}

#[then(regex = r#"^the version id error is "([^"]+)""#)]
fn then_version_error(world: &mut ManifestWorld, name: String) {
    assert_eq!(world.version_error.as_deref(), Some(name.as_str()));
}

fn snapshot_file(dir: &Path) -> PathBuf {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir.join("snapshots"))
        .expect("snapshots")
        .flatten()
    {
        let path = entry.path().join("part.parquet");
        if path.is_file() {
            files.push(path);
        }
    }
    assert_eq!(files.len(), 1);
    files.remove(0)
}

fn load_published(
    world: &ManifestWorld,
) -> Result<
    parangsae_collector::version::Verified<parangsae_collector::version::Table>,
    parangsae_collector::version::LoadError,
> {
    let day = world.day.expect("day");
    load_on(world, day)
}

fn load_on(
    world: &ManifestWorld,
    day: CivilDay,
) -> Result<
    parangsae_collector::version::Verified<parangsae_collector::version::Table>,
    parangsae_collector::version::LoadError,
> {
    let dir = world.dir.as_ref().expect("dir");
    let dataset = DatasetId::parse("binance-vision.um.um-kline.BTCUSDT").expect("dataset id");
    let root = VersionRoot::parse_hex(world.version_root.as_deref().expect("root")).expect("root");
    let digest = ManifestDigest::parse_hex(world.manifest_digest.as_deref().expect("digest"))
        .expect("digest");
    let range = DayRange::new(day, day).expect("range");
    VersionStore::open(dir)?.load(&dataset, root, digest, range)
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
